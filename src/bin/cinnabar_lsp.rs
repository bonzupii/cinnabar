// cinnabar-lsp: a Language Server Protocol server over the compiler's
// attached facts.
//
// The server is a thin JSON-RPC shell around `cinnabar::analysis`: every
// request re-runs the real front-end pipeline over the open buffers (via
// the module loader's overlay) and answers from the attachments the
// pipeline computed.  There is no second implementation of name
// resolution, type inference, or borrow checking anywhere in this file.
//
// Protocol payloads are built as raw JSON values: the server speaks the
// small, stable subset of the protocol it implements, and the analysis
// layer keeps every language-aware decision.

use cinnabar::analysis::{
    analyze, completions, definition, file_id_of, file_text_of, hover, offset_to_position,
    position_to_offset, references, signature_help, Analysis, COMPLETE_FIELD, COMPLETE_KEYWORD,
    COMPLETE_LOCAL,
};
use cinnabar::ast::{
    NO_FILE, SYM_CONST, SYM_ENUM, SYM_FUN, SYM_IMPL_METHOD, SYM_MODULE, SYM_NATIVE_FUN,
    SYM_STRUCT, SYM_TRAIT, SYM_TRAIT_METHOD, SYM_TYPE, SYM_VARIANT,
};
use cinnabar::project;
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

const DIAGNOSTIC_DEBOUNCE_MS: u64 = 75;

struct AnalysisResult {
    path: String,
    generation: i64,
    analysis: Analysis,
}

struct ServerState {
    // Open documents as (file-system path, buffer text): the module
    // loader's overlay, so unsaved edits are analyzed like saved files.
    docs: Vec<(String, String)>,
    // URIs we last published diagnostics for, so stale ones are cleared.
    published: Vec<(String, Vec<String>)>,
    // Analysis roots and the root that owns each open document.  Secondary
    // modules stay in their entry file's graph when editors open them.
    roots: Vec<String>,
    doc_entries: Vec<(String, String)>,
    // Per-document edit generations and pending debounce deadlines.  A
    // completed analysis is published only when its generation is still
    // current, which makes superseded full checks harmless.
    generations: Vec<(String, i64)>,
    pending: Vec<(String, i64, Instant)>,
    // At most one full front-end run is active.  Newer generations remain
    // pending until it finishes, so sustained editing cannot accumulate
    // detached compiler threads.
    running_path: Option<String>,
    analysis_tx: Sender<AnalysisResult>,
    analysis_rx: Receiver<AnalysisResult>,
    // The one authoritative attached-fact snapshot for each root's current
    // generation. Positional requests consume this analysis; they never
    // invoke the compiler themselves.
    completed: Vec<AnalysisResult>,
}

// A fatal transport failure surfaces through main's Result: there is no
// usable channel left to log through when the JSON-RPC connection itself is
// gone.
fn main() -> Result<(), String> {
    let (connection, io_threads) = Connection::stdio();
    let capabilities = json!({
        "textDocumentSync": 1,
        "hoverProvider": true,
        "definitionProvider": true,
        "referencesProvider": true,
        "completionProvider": { "triggerCharacters": ["."] },
        "signatureHelpProvider": { "triggerCharacters": ["(", ","] },
        "codeLensProvider": { "resolveProvider": false }
    });
    // lsp-server wraps this value in the InitializeResult's `capabilities`
    // field itself.
    let init_params = connection
        .initialize(capabilities)
        .map_err(|err| format!("initialize failed: {}", err))?;
    let client = init_params
        .get("clientInfo")
        .and_then(|info| info.get("name"))
        .and_then(|name| name.as_str())
        .map(|name| name.to_string())
        .unwrap_or_else(|| "unnamed client".to_string());
    log_message(&connection, &format!("cinnabar-lsp ready for {}", client))?;
    let (analysis_tx, analysis_rx) = channel();
    let mut state = ServerState {
        docs: Vec::new(),
        published: Vec::new(),
        roots: Vec::new(),
        doc_entries: Vec::new(),
        generations: Vec::new(),
        pending: Vec::new(),
        running_path: None,
        analysis_tx,
        analysis_rx,
        completed: Vec::new(),
    };
    main_loop(&connection, &mut state)?;
    // The transport threads terminate when every channel endpoint is gone.
    // Release the server-side endpoints before waiting for those threads;
    // otherwise a clean shutdown deadlocks with the writer waiting on the
    // still-live sender held by `connection`.
    drop(connection);
    io_threads.join().map_err(|err| format!("io threads: {}", err))?;
    Ok(())
}

// Server-side logging goes through the protocol (`window/logMessage`, type
// 3 = Info), never through the process's own stdio, which the JSON-RPC
// transport owns.
fn log_message(connection: &Connection, message: &str) -> Result<(), String> {
    let note = Notification {
        method: "window/logMessage".to_string(),
        params: json!({ "type": 3, "message": message }),
    };
    connection
        .sender
        .send(Message::Notification(note))
        .map_err(|err| format!("send log message: {}", err))
}

fn main_loop(connection: &Connection, state: &mut ServerState) -> Result<(), String> {
    loop {
        publish_completed(connection, state)?;
        start_due_analyses(state);
        let received = connection.receiver.recv_timeout(Duration::from_millis(25));
        match received {
            Ok(msg) => match msg {
            Message::Request(req) => {
                match connection.handle_shutdown(&req) {
                    Ok(true) => return Ok(()),
                    Ok(false) => dispatch_request(connection, state, req)?,
                    Err(err) => return Err(format!("shutdown handling: {}", err)),
                }
            }
            Message::Notification(note) => handle_notification(connection, state, note)?,
            Message::Response(resp) => {
                log_message(
                    connection,
                    &format!("cinnabar-lsp: ignoring unexpected response to request {:?}", resp.id),
                )?;
            }
            },
            Err(err) => {
                if !err.is_timeout() {
                    return Ok(());
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

fn dispatch_request(connection: &Connection, state: &ServerState, req: Request) -> Result<(), String> {
    let method = req.method.clone();
    if method == "textDocument/hover" {
        let result = on_hover(state, &req.params);
        return send_ok(connection, req.id, result);
    }
    if method == "textDocument/definition" {
        let result = on_definition(state, &req.params);
        return send_ok(connection, req.id, result);
    }
    if method == "textDocument/references" {
        let result = on_references(state, &req.params);
        return send_ok(connection, req.id, result);
    }
    if method == "textDocument/completion" {
        let result = on_completion(state, &req.params);
        return send_ok(connection, req.id, result);
    }
    if method == "textDocument/signatureHelp" {
        let result = on_signature_help(state, &req.params);
        return send_ok(connection, req.id, result);
    }
    if method == "textDocument/codeLens" {
        let result = on_code_lens(state, &req.params);
        return send_ok(connection, req.id, result);
    }
    let resp = Response::new_err(
        req.id,
        lsp_server::ErrorCode::MethodNotFound as i32,
        format!("method '{}' is not supported by cinnabar-lsp", method),
    );
    connection
        .sender
        .send(Message::Response(resp))
        .map_err(|err| format!("send response: {}", err))
}

fn send_ok(connection: &Connection, id: RequestId, result: Value) -> Result<(), String> {
    connection
        .sender
        .send(Message::Response(Response::new_ok(id, result)))
        .map_err(|err| format!("send response: {}", err))
}

// The (path, file id, byte offset, analysis) behind a positional request,
// or None when the params don't name a file this server can analyze.
fn positional<'state>(state: &'state ServerState, params: &Value) -> Option<(&'state Analysis, i64, i64)> {
    let uri = params.get("textDocument")?.get("uri")?.as_str()?;
    let path = uri_to_path(uri)?;
    let line = params.get("position")?.get("line")?.as_i64()?;
    let character = params.get("position")?.get("character")?.as_i64()?;
    let entry = entry_of_doc(state, &path);
    let analysis = completed_analysis(state, &entry)?;
    let file = file_id_of(analysis, &path);
    if file == NONE_ID {
        return None;
    }
    let text = file_text_of(analysis, file);
    let offset = position_to_offset(&text, line, character);
    Some((analysis, file, offset))
}

const NONE_ID: i64 = -1;

fn range_json(analysis: &Analysis, file: i64, start: i64, end: i64) -> Value {
    let text = file_text_of(analysis, file);
    let (sl, sc) = offset_to_position(&text, start);
    let (el, ec) = offset_to_position(&text, end);
    json!({
        "start": { "line": sl, "character": sc },
        "end": { "line": el, "character": ec }
    })
}

fn location_json(analysis: &Analysis, file: i64, start: i64, end: i64) -> Option<Value> {
    let path = analysis.files.get(file as usize).map(|pair| pair.0.clone())?;
    let uri = path_to_uri(&path);
    Some(json!({ "uri": uri, "range": range_json(analysis, file, start, end) }))
}

fn on_hover(state: &ServerState, params: &Value) -> Value {
    let (analysis, file, offset) = match positional(state, params) {
        Some(found) => found,
        None => return Value::Null,
    };
    match hover(analysis, file, offset) {
        Some((markdown, span)) => json!({
            "contents": { "kind": "markdown", "value": markdown },
            "range": range_json(analysis, span.0, span.1, span.2)
        }),
        None => Value::Null,
    }
}

fn on_definition(state: &ServerState, params: &Value) -> Value {
    let (analysis, file, offset) = match positional(state, params) {
        Some(found) => found,
        None => return Value::Null,
    };
    match definition(analysis, file, offset) {
        Some(span) => match location_json(analysis, span.0, span.1, span.2) {
            Some(location) => location,
            None => Value::Null,
        },
        None => Value::Null,
    }
}

fn on_references(state: &ServerState, params: &Value) -> Value {
    let (analysis, file, offset) = match positional(state, params) {
        Some(found) => found,
        None => return Value::Null,
    };
    let spans = references(analysis, file, offset);
    let mut out: Vec<Value> = Vec::new();
    let mut idx = 0usize;
    while idx < spans.len() {
        match spans.get(idx) {
            Some(span) => {
                if let Some(location) = location_json(analysis, span.0, span.1, span.2) {
                    out.push(location);
                }
            }
            None => break,
        }
        idx += 1;
    }
    Value::Array(out)
}

fn completion_kind_code(kind: i64) -> i64 {
    // LSP CompletionItemKind numbers.
    if kind == SYM_FUN || kind == SYM_NATIVE_FUN || kind == SYM_IMPL_METHOD || kind == SYM_TRAIT_METHOD {
        3 // Function
    } else if kind == SYM_STRUCT {
        22 // Struct
    } else if kind == SYM_ENUM {
        13 // Enum
    } else if kind == SYM_VARIANT {
        20 // EnumMember
    } else if kind == SYM_TRAIT {
        8 // Interface
    } else if kind == SYM_TYPE {
        7 // Class
    } else if kind == SYM_CONST {
        21 // Constant
    } else if kind == SYM_MODULE {
        9 // Module
    } else if kind == COMPLETE_LOCAL {
        6 // Variable
    } else if kind == COMPLETE_KEYWORD {
        14 // Keyword
    } else if kind == COMPLETE_FIELD {
        5 // Field
    } else {
        1 // Text
    }
}

fn on_completion(state: &ServerState, params: &Value) -> Value {
    let (analysis, file, offset) = match positional(state, params) {
        Some(found) => found,
        None => return Value::Null,
    };
    let items = completions(analysis, file, offset);
    let mut out: Vec<Value> = Vec::new();
    let mut idx = 0usize;
    while idx < items.len() {
        match items.get(idx) {
            Some(item) => out.push(json!({
                "label": item.0,
                "kind": completion_kind_code(item.1)
            })),
            None => break,
        }
        idx += 1;
    }
    Value::Array(out)
}

fn on_signature_help(state: &ServerState, params: &Value) -> Value {
    let (analysis, file, offset) = match positional(state, params) {
        Some(found) => found,
        None => return Value::Null,
    };
    match signature_help(analysis, file, offset) {
        Some(info) => {
            let mut params_json: Vec<Value> = Vec::new();
            let mut idx = 0usize;
            while idx < info.params.len() {
                match info.params.get(idx) {
                    Some(label) => params_json.push(json!({ "label": label })),
                    None => break,
                }
                idx += 1;
            }
            json!({
                "signatures": [{ "label": info.label, "parameters": params_json }],
                "activeSignature": 0,
                "activeParameter": info.active
            })
        }
        None => Value::Null,
    }
}

fn on_code_lens(state: &ServerState, params: &Value) -> Value {
    let uri = match params
        .get("textDocument")
        .and_then(|doc| doc.get("uri"))
        .and_then(|value| value.as_str())
    {
        Some(value) => value,
        None => return Value::Array(Vec::new()),
    };
    let path = match uri_to_path(uri) {
        Some(value) => value,
        None => return Value::Array(Vec::new()),
    };
    let entry = entry_of_doc(state, &path);
    let analysis = match completed_analysis(state, &entry) {
        Some(value) => value,
        None => return Value::Array(Vec::new()),
    };
    let file = file_id_of(analysis, &path);
    if file == NONE_ID {
        return Value::Array(Vec::new());
    }
    let mut lenses: Vec<Value> = Vec::new();
    let mut idx = 0usize;
    while idx < analysis.notes.len() {
        match analysis.notes.get(idx) {
            Some(note) => {
                if note.2 == file {
                    lenses.push(json!({
                        "range": range_json(analysis, note.2, note.3, note.4),
                        "command": {
                            "title": note.1,
                            "command": "cinnabar.showExplanation",
                            "arguments": [note.1]
                        }
                    }));
                }
            }
            None => break,
        }
        idx += 1;
    }
    Value::Array(lenses)
}

// ---------------------------------------------------------------------------
// Notifications / document sync
// ---------------------------------------------------------------------------

fn handle_notification(
    connection: &Connection,
    state: &mut ServerState,
    note: Notification,
) -> Result<(), String> {
    let method = note.method.clone();
    if method == "textDocument/didOpen" {
        let uri_path = note
            .params
            .get("textDocument")
            .and_then(|doc| doc.get("uri"))
            .and_then(|uri| uri.as_str())
            .and_then(uri_to_path);
        let text = note
            .params
            .get("textDocument")
            .and_then(|doc| doc.get("text"))
            .and_then(|text| text.as_str())
            .map(|text| text.to_string());
        if let (Some(path), Some(content)) = (uri_path, text) {
            set_doc(state, &path, content);
            let entry = register_document(state, &path);
            schedule_analysis(state, &entry, DIAGNOSTIC_DEBOUNCE_MS);
        }
        return Ok(());
    }
    if method == "textDocument/didChange" {
        let uri_path = note
            .params
            .get("textDocument")
            .and_then(|doc| doc.get("uri"))
            .and_then(|uri| uri.as_str())
            .and_then(uri_to_path);
        // Full-document sync: the last content change carries the whole text.
        let text = note
            .params
            .get("contentChanges")
            .and_then(|changes| changes.as_array())
            .and_then(|changes| changes.last())
            .and_then(|change| change.get("text"))
            .and_then(|text| text.as_str())
            .map(|text| text.to_string());
        if let (Some(path), Some(content)) = (uri_path, text) {
            set_doc(state, &path, content);
            let entry = entry_of_doc(state, &path);
            schedule_analysis(state, &entry, DIAGNOSTIC_DEBOUNCE_MS);
        }
        return Ok(());
    }
    if method == "textDocument/didSave" {
        let uri_path = note
            .params
            .get("textDocument")
            .and_then(|doc| doc.get("uri"))
            .and_then(|uri| uri.as_str())
            .and_then(uri_to_path);
        if let Some(path) = uri_path {
            let entry = entry_of_doc(state, &path);
            schedule_analysis(state, &entry, 0);
        }
        return Ok(());
    }
    if method == "textDocument/didClose" {
        let uri_path = note
            .params
            .get("textDocument")
            .and_then(|doc| doc.get("uri"))
            .and_then(|uri| uri.as_str())
            .and_then(uri_to_path);
        if let Some(path) = uri_path {
            let entry = entry_of_doc(state, &path);
            remove_doc(state, &path);
            remove_doc_entry(state, &path);
            if entry == path {
                close_root(connection, state, &entry)?;
            } else {
                let cleared: Vec<Value> = Vec::new();
                send_diagnostics(connection, &path_to_uri(&path), cleared)?;
                schedule_analysis(state, &entry, 0);
            }
        }
        return Ok(());
    }
    // initialized, setTrace, cancelRequest, exit and anything else need no
    // action from this server.
    Ok(())
}

fn entry_of_doc(state: &ServerState, path: &str) -> String {
    let mut idx = 0usize;
    while idx < state.doc_entries.len() {
        match state.doc_entries.get(idx) {
            Some(pair) => {
                if pair.0 == path {
                    return pair.1.clone();
                }
            }
            None => break,
        }
        idx += 1;
    }
    path.to_string()
}

fn register_document(state: &mut ServerState, path: &str) -> String {
    let existing = entry_of_doc(state, path);
    if existing != path {
        return existing;
    }
    if state.roots.contains(&path.to_string()) {
        return path.to_string();
    }
    if let Some(entry_path) = project::entry_for_source(std::path::Path::new(path)) {
        let entry = entry_path.to_string_lossy().to_string();
        state.doc_entries.push((path.to_string(), entry.clone()));
        if !state.roots.contains(&entry) {
            state.roots.push(entry.clone());
        }
        return entry;
    }
    let uri = path_to_uri(path);
    let mut idx = 0usize;
    while idx < state.published.len() {
        match state.published.get(idx) {
            Some(record) => {
                if record.1.contains(&uri) {
                    let entry = record.0.clone();
                    state.doc_entries.push((path.to_string(), entry.clone()));
                    return entry;
                }
            }
            None => break,
        }
        idx += 1;
    }
    let entry = path.to_string();
    state.roots.push(entry.clone());
    state.doc_entries.push((entry.clone(), entry.clone()));
    entry
}

// Reconcile open-order differences from the compiler's actual module graph.
// If this entry contains roots that were opened earlier as standalone files,
// it becomes their owner.  No import or path semantics are reconstructed here:
// membership comes exclusively from module_loader's analyzed file set.
fn reconcile_root_graph(state: &mut ServerState, entry: &str, analysis: &Analysis) {
    let mut adopted: Vec<String> = Vec::new();
    let mut idx = 0usize;
    while idx < state.roots.len() {
        match state.roots.get(idx) {
            Some(root) => {
                if root != entry && file_id_of(analysis, root) != NONE_ID {
                    adopted.push(root.clone());
                }
            }
            None => break,
        }
        idx += 1;
    }
    idx = 0;
    while idx < adopted.len() {
        match adopted.get(idx) {
            Some(root) => invalidate_analysis(state, root),
            None => break,
        }
        idx += 1;
    }
    idx = 0;
    while idx < state.doc_entries.len() {
        let owner = match state.doc_entries.get(idx) {
            Some(pair) => pair.1.clone(),
            None => break,
        };
        if adopted.contains(&owner)
            && let Some(pair) = state.doc_entries.get_mut(idx)
        {
            pair.1 = entry.to_string();
        }
        idx += 1;
    }
    let mut roots: Vec<String> = Vec::new();
    while let Some(root) = state.roots.pop() {
        if !adopted.contains(&root) {
            roots.push(root);
        }
    }
    state.roots = roots;
    if !adopted.is_empty() {
        transfer_publications(state, entry, &adopted);
    }
}

fn transfer_publications(state: &mut ServerState, entry: &str, adopted: &[String]) {
    let mut records: Vec<(String, Vec<String>)> = Vec::new();
    let mut owned: Vec<String> = Vec::new();
    while let Some(record) = state.published.pop() {
        if record.0 == entry || adopted.contains(&record.0) {
            let mut uri_idx = 0usize;
            while uri_idx < record.1.len() {
                match record.1.get(uri_idx) {
                    Some(uri) => {
                        if !owned.contains(uri) {
                            owned.push(uri.clone());
                        }
                    }
                    None => break,
                }
                uri_idx += 1;
            }
        } else {
            records.push(record);
        }
    }
    records.push((entry.to_string(), owned));
    state.published = records;
}

fn remove_doc_entry(state: &mut ServerState, path: &str) {
    let mut kept: Vec<(String, String)> = Vec::new();
    while let Some(pair) = state.doc_entries.pop() {
        if pair.0 != path {
            kept.push(pair);
        }
    }
    state.doc_entries = kept;
}

fn close_root(connection: &Connection, state: &mut ServerState, entry: &str) -> Result<(), String> {
    invalidate_analysis(state, entry);
    let mut roots: Vec<String> = Vec::new();
    while let Some(root) = state.roots.pop() {
        if root != entry {
            roots.push(root);
        }
    }
    state.roots = roots;
    let mut remap: Vec<String> = Vec::new();
    let mut mappings: Vec<(String, String)> = Vec::new();
    while let Some(pair) = state.doc_entries.pop() {
        if pair.1 == entry {
            if pair.0 != entry {
                remap.push(pair.0);
            }
        } else {
            mappings.push(pair);
        }
    }
    state.doc_entries = mappings;
    let mut records: Vec<(String, Vec<String>)> = Vec::new();
    let mut closing_uris: Vec<String> = Vec::new();
    while let Some(record) = state.published.pop() {
        if record.0 == entry {
            closing_uris = record.1;
        } else {
            records.push(record);
        }
    }
    let mut idx = 0usize;
    while idx < closing_uris.len() {
        match closing_uris.get(idx) {
            Some(uri) => {
                if !uri_owned_by(&records, uri) {
                    let cleared: Vec<Value> = Vec::new();
                    send_diagnostics(connection, uri, cleared)?;
                }
            }
            None => break,
        }
        idx += 1;
    }
    state.published = records;
    while let Some(path) = remap.pop() {
        let new_entry = register_document(state, &path);
        schedule_analysis(state, &new_entry, 0);
    }
    Ok(())
}

fn current_generation(state: &ServerState, path: &str) -> i64 {
    let mut idx = 0usize;
    while idx < state.generations.len() {
        match state.generations.get(idx) {
            Some(entry) => {
                if entry.0 == path {
                    return entry.1;
                }
            }
            None => break,
        }
        idx += 1;
    }
    0
}

// A completed analysis is valid only for the same root generation that
// produced it. Document edits advance that generation before scheduling the
// next worker, so stale compiler facts can never answer a positional request.
fn completed_analysis<'state>(state: &'state ServerState, path: &str) -> Option<&'state Analysis> {
    let generation = current_generation(state, path);
    let mut idx = 0usize;
    while idx < state.completed.len() {
        match state.completed.get(idx) {
            Some(result) => {
                if result.path == path && result.generation == generation {
                    return Some(&result.analysis);
                }
            }
            None => break,
        }
        idx += 1;
    }
    None
}

fn retain_completed_analysis(state: &mut ServerState, result: AnalysisResult) {
    let mut idx = 0usize;
    while idx < state.completed.len() {
        let matches_path = match state.completed.get(idx) {
            Some(existing) => existing.path == result.path,
            None => false,
        };
        if matches_path {
            state.completed.remove(idx);
            break;
        }
        idx += 1;
    }
    state.completed.push(result);
}

fn discard_completed_analysis(state: &mut ServerState, path: &str) {
    let mut idx = 0usize;
    while idx < state.completed.len() {
        let matches_path = match state.completed.get(idx) {
            Some(existing) => existing.path == path,
            None => false,
        };
        if matches_path {
            state.completed.remove(idx);
            return;
        }
        idx += 1;
    }
}

fn advance_generation(state: &mut ServerState, path: &str) -> i64 {
    let mut idx = 0usize;
    while idx < state.generations.len() {
        let matches_path = match state.generations.get(idx) {
            Some(entry) => entry.0 == path,
            None => false,
        };
        if matches_path
            && let Some(entry) = state.generations.get_mut(idx)
        {
                entry.1 += 1;
                return entry.1;
        }
        idx += 1;
    }
    state.generations.push((path.to_string(), 1));
    1
}

fn invalidate_analysis(state: &mut ServerState, path: &str) {
    advance_generation(state, path);
    discard_completed_analysis(state, path);
    let mut idx = 0usize;
    while idx < state.pending.len() {
        let matches_path = match state.pending.get(idx) {
            Some(entry) => entry.0 == path,
            None => false,
        };
        if matches_path {
            state.pending.remove(idx);
        } else {
            idx += 1;
        }
    }
}

fn schedule_analysis(state: &mut ServerState, path: &str, delay_ms: u64) {
    invalidate_analysis(state, path);
    let generation = current_generation(state, path);
    state.pending.push((
        path.to_string(),
        generation,
        Instant::now() + Duration::from_millis(delay_ms),
    ));
}

fn start_due_analyses(state: &mut ServerState) {
    if state.running_path.is_some() {
        return;
    }
    let now = Instant::now();
    let mut due_idx: Option<usize> = None;
    let mut idx = 0usize;
    while idx < state.pending.len() {
        let due = match state.pending.get(idx) {
            Some(entry) => entry.2 <= now,
            None => false,
        };
        if due {
            due_idx = Some(idx);
            break;
        }
        idx += 1;
    }
    if let Some(selected_idx) = due_idx {
        let selected = state.pending.remove(selected_idx);
        let path = selected.0;
        let generation = selected.1;
        let docs = state.docs.clone();
        let tx = state.analysis_tx.clone();
        state.running_path = Some(path.clone());
        std::thread::spawn(move || {
            let analysis = analyze(&path, &docs);
            tx.send(AnalysisResult { path, generation, analysis }).is_ok()
        });
    }
}

fn publish_completed(connection: &Connection, state: &mut ServerState) -> Result<(), String> {
    while let Ok(result) = state.analysis_rx.try_recv() {
        let finished_running = state
            .running_path
            .as_ref()
            .is_some_and(|path| path == &result.path);
        if finished_running {
            state.running_path = None;
        }
        if current_generation(state, &result.path) == result.generation {
            publish_analysis(connection, state, &result.path, &result.analysis)?;
            retain_completed_analysis(state, result);
        }
    }
    Ok(())
}

fn set_doc(state: &mut ServerState, path: &str, content: String) {
    let mut idx = 0usize;
    while idx < state.docs.len() {
        let matches_path = match state.docs.get(idx) {
            Some(entry) => entry.0 == path,
            None => false,
        };
        if matches_path {
            if let Some(entry) = state.docs.get_mut(idx) {
                entry.1 = content;
            }
            return;
        }
        idx += 1;
    }
    state.docs.push((path.to_string(), content));
}

fn remove_doc(state: &mut ServerState, path: &str) {
    let mut kept: Vec<(String, String)> = Vec::new();
    while let Some(entry) = state.docs.pop() {
        if entry.0 != path {
            kept.push(entry);
        }
    }
    state.docs = kept;
}

fn severity_error() -> i64 {
    1
}

fn publish_analysis(
    connection: &Connection,
    state: &mut ServerState,
    entry_path: &str,
    analysis: &Analysis,
) -> Result<(), String> {
    reconcile_root_graph(state, entry_path, analysis);
    let mut fresh: Vec<String> = Vec::new();
    let mut file = 0i64;
    while (file as usize) < analysis.files.len() {
        let path = match analysis.files.get(file as usize) {
            Some(pair) => pair.0.clone(),
            None => break,
        };
        let uri = path_to_uri(&path);
        let diags = file_diagnostics(analysis, file);
        send_diagnostics(connection, &uri, diags)?;
        fresh.push(uri);
        file += 1;
    }
    // Replace only this root's publication set.  Another open root may own
    // the same URI, so a stale URI is cleared only when no remaining root
    // still publishes it.
    let mut prior: Vec<String> = Vec::new();
    let mut records: Vec<(String, Vec<String>)> = Vec::new();
    while let Some(record) = state.published.pop() {
        if record.0 == entry_path {
            prior = record.1;
        } else {
            records.push(record);
        }
    }
    let mut idx = 0usize;
    while idx < prior.len() {
        match prior.get(idx) {
            Some(uri) => {
                if !fresh.contains(uri) && !uri_owned_by(&records, uri) {
                    let cleared: Vec<Value> = Vec::new();
                    send_diagnostics(connection, uri, cleared)?;
                }
            }
            None => break,
        }
        idx += 1;
    }
    records.push((entry_path.to_string(), fresh));
    state.published = records;
    Ok(())
}

fn uri_owned_by(records: &[(String, Vec<String>)], uri: &str) -> bool {
    let mut idx = 0usize;
    while idx < records.len() {
        match records.get(idx) {
            Some(record) => {
                if record.1.contains(&uri.to_string()) {
                    return true;
                }
            }
            None => break,
        }
        idx += 1;
    }
    false
}

fn file_diagnostics(analysis: &Analysis, file: i64) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut idx = 0usize;
    while idx < analysis.errors.len() {
        let diag = match analysis.errors.get(idx) {
            Some(diag) => diag,
            None => break,
        };
        if diag.1 == file {
            let mut related: Vec<Value> = Vec::new();
            let mut note_idx = 0usize;
            while note_idx < analysis.notes.len() {
                match analysis.notes.get(note_idx) {
                    Some(note) => {
                        if note.0 == idx as i64
                            && note.2 != NO_FILE
                            && let Some(location) = location_json(analysis, note.2, note.3, note.4)
                        {
                            related.push(json!({ "location": location, "message": note.1 }));
                        }
                    }
                    None => break,
                }
                note_idx += 1;
            }
            let mut diag_json = json!({
                "range": range_json(analysis, file, diag.2, diag.3),
                "severity": severity_error(),
                "source": "cinnabar",
                "message": diag.0
            });
            if !related.is_empty()
                && let Some(object) = diag_json.as_object_mut()
            {
                object.insert("relatedInformation".to_string(), Value::Array(related));
            }
            out.push(diag_json);
        }
        idx += 1;
    }
    out
}

fn send_diagnostics(connection: &Connection, uri: &str, diagnostics: Vec<Value>) -> Result<(), String> {
    let note = Notification {
        method: "textDocument/publishDiagnostics".to_string(),
        params: json!({ "uri": uri, "diagnostics": diagnostics }),
    };
    connection
        .sender
        .send(Message::Notification(note))
        .map_err(|err| format!("send diagnostics: {}", err))
}

// ---------------------------------------------------------------------------
// file:// URI <-> path
// ---------------------------------------------------------------------------

fn hex_value(byte: u8) -> Option<u8> {
    if byte.is_ascii_digit() {
        return Some(byte - b'0');
    }
    if byte.is_ascii_hexdigit() && byte.is_ascii_lowercase() {
        return Some(byte - b'a' + 10);
    }
    if byte.is_ascii_hexdigit() && byte.is_ascii_uppercase() {
        return Some(byte - b'A' + 10);
    }
    None
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut idx = 0usize;
    while idx < bytes.len() {
        let byte = match bytes.get(idx) {
            Some(value) => *value,
            None => break,
        };
        if byte == b'%' && idx + 2 <= bytes.len() {
            let high = bytes.get(idx + 1).and_then(|value| hex_value(*value));
            let low = bytes.get(idx + 2).and_then(|value| hex_value(*value));
            if let (Some(hi), Some(lo)) = (high, low) {
                out.push(hi * 16 + lo);
                idx += 3;
                continue;
            }
        }
        out.push(byte);
        idx += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn percent_encode_path(path: &str) -> String {
    let mut out = String::new();
    for byte in path.as_bytes() {
        let ch = *byte as char;
        if byte.is_ascii_alphanumeric() || "/:._~-".contains(ch) {
            out.push(ch);
        } else if ch == '\\' {
            out.push('/');
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}

/// A file:// URI to a local file-system path, or None for other schemes.
fn uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    // Strip an empty authority; a non-empty authority (remote host) is not
    // a local file.  "file:///..." leaves the absolute path (minus its
    // leading slash on Windows drive paths).
    let path_part = rest.strip_prefix('/')?;
    let decoded = percent_decode(path_part);
    // Windows drive path: "C:/..." after decoding.  A path that does not
    // look like a drive keeps its leading slash (POSIX).
    let is_drive = {
        let bytes = decoded.as_bytes();
        bytes.len() >= 2
            && bytes.first().is_some_and(|byte| byte.is_ascii_alphabetic())
            && bytes.get(1).is_some_and(|byte| *byte == b':')
    };
    if is_drive {
        Some(decoded)
    } else {
        Some(format!("/{}", decoded))
    }
}

/// A local file-system path to a file:// URI.
fn path_to_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{}", percent_encode_path(&normalized))
    } else {
        format!("file:///{}", percent_encode_path(&normalized))
    }
}

#[cfg(test)]
mod tests {
    use super::{path_to_uri, start_due_analyses, uri_to_path, AnalysisResult, ServerState};
    use std::sync::mpsc::channel;
    use std::time::Instant;

    #[test]
    fn analysis_scheduler_allows_only_one_active_frontend() {
        let (analysis_tx, analysis_rx) = channel::<AnalysisResult>();
        let now = Instant::now();
        let mut state = ServerState {
            docs: Vec::new(),
            published: Vec::new(),
            roots: Vec::new(),
            doc_entries: Vec::new(),
            generations: vec![("first.cnb".to_string(), 1), ("second.cnb".to_string(), 1)],
            pending: vec![
                ("first.cnb".to_string(), 1, now),
                ("second.cnb".to_string(), 1, now),
            ],
            running_path: None,
            analysis_tx,
            analysis_rx,
            completed: Vec::new(),
        };

        start_due_analyses(&mut state);
        assert!(state.running_path.is_some());
        assert_eq!(state.pending.len(), 1);

        start_due_analyses(&mut state);
        assert!(state.running_path.is_some());
        assert_eq!(state.pending.len(), 1, "a second full analysis started concurrently");
    }

    #[test]
    fn windows_file_uri_roundtrips_reserved_characters() {
        let path = "C:\\Users\\Cinnabar Dev\\source#one.cnb";
        let uri = path_to_uri(path);
        assert_eq!(uri, "file:///C:/Users/Cinnabar%20Dev/source%23one.cnb");
        assert_eq!(uri_to_path(&uri), Some("C:/Users/Cinnabar Dev/source#one.cnb".to_string()));
    }

    #[test]
    fn posix_file_uri_roundtrips_reserved_characters() {
        let path = "/tmp/Cinnabar Dev/source#one.cnb";
        let uri = path_to_uri(path);
        assert_eq!(uri, "file:///tmp/Cinnabar%20Dev/source%23one.cnb");
        assert_eq!(uri_to_path(&uri), Some(path.to_string()));
    }

    #[test]
    fn remote_file_authority_is_not_treated_as_local() {
        assert_eq!(uri_to_path("file://server/share/source.cnb"), None);
    }
}
