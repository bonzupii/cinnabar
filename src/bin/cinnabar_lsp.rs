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
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use serde_json::{json, Value};

struct ServerState {
    // Open documents as (file-system path, buffer text): the module
    // loader's overlay, so unsaved edits are analyzed like saved files.
    docs: Vec<(String, String)>,
    // URIs we last published diagnostics for, so stale ones are cleared.
    published: Vec<String>,
}

fn main() {
    if let Err(message) = run() {
        eprintln!("cinnabar-lsp: {}", message);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let (connection, io_threads) = Connection::stdio();
    let capabilities = json!({
        "textDocumentSync": 1,
        "hoverProvider": true,
        "definitionProvider": true,
        "referencesProvider": true,
        "completionProvider": { "triggerCharacters": ["."] },
        "signatureHelpProvider": { "triggerCharacters": ["(", ","] }
    });
    // lsp-server wraps this value in the InitializeResult's `capabilities`
    // field itself.
    let init_result = connection
        .initialize(capabilities)
        .map_err(|err| format!("initialize failed: {}", err))?;
    // The client's initialize params carry nothing this server configures
    // itself by yet; acknowledge receipt in the log for debuggability.
    if init_result.is_null() {
        eprintln!("cinnabar-lsp: client sent null initialize params");
    }
    let mut state = ServerState { docs: Vec::new(), published: Vec::new() };
    main_loop(&connection, &mut state)?;
    io_threads.join().map_err(|err| format!("io threads: {}", err))?;
    Ok(())
}

fn main_loop(connection: &Connection, state: &mut ServerState) -> Result<(), String> {
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                match connection.handle_shutdown(&req) {
                    Ok(true) => return Ok(()),
                    Ok(false) => dispatch_request(connection, state, req)?,
                    Err(err) => return Err(format!("shutdown handling: {}", err)),
                }
            }
            Message::Notification(note) => handle_notification(connection, state, note)?,
            Message::Response(resp) => {
                eprintln!("cinnabar-lsp: ignoring unexpected response to request {:?}", resp.id);
            }
        }
    }
    Ok(())
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
fn positional(state: &ServerState, params: &Value) -> Option<(Analysis, i64, i64)> {
    let uri = params.get("textDocument")?.get("uri")?.as_str()?;
    let path = uri_to_path(uri)?;
    let line = params.get("position")?.get("line")?.as_i64()?;
    let character = params.get("position")?.get("character")?.as_i64()?;
    let analysis = analyze(&path, &state.docs);
    let file = file_id_of(&analysis, &path);
    if file == NONE_ID {
        return None;
    }
    let text = file_text_of(&analysis, file);
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
    match hover(&analysis, file, offset) {
        Some((markdown, span)) => json!({
            "contents": { "kind": "markdown", "value": markdown },
            "range": range_json(&analysis, span.0, span.1, span.2)
        }),
        None => Value::Null,
    }
}

fn on_definition(state: &ServerState, params: &Value) -> Value {
    let (analysis, file, offset) = match positional(state, params) {
        Some(found) => found,
        None => return Value::Null,
    };
    match definition(&analysis, file, offset) {
        Some(span) => match location_json(&analysis, span.0, span.1, span.2) {
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
    let spans = references(&analysis, file, offset);
    let mut out: Vec<Value> = Vec::new();
    let mut idx = 0usize;
    while idx < spans.len() {
        match spans.get(idx) {
            Some(span) => {
                if let Some(location) = location_json(&analysis, span.0, span.1, span.2) {
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
    let items = completions(&analysis, file, offset);
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
    match signature_help(&analysis, file, offset) {
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
            publish_diagnostics(connection, state, &path)?;
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
            publish_diagnostics(connection, state, &path)?;
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
            publish_diagnostics(connection, state, &path)?;
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
            remove_doc(state, &path);
        }
        return Ok(());
    }
    // initialized, setTrace, cancelRequest, exit and anything else need no
    // action from this server.
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

fn publish_diagnostics(
    connection: &Connection,
    state: &mut ServerState,
    entry_path: &str,
) -> Result<(), String> {
    let analysis = analyze(entry_path, &state.docs);
    let mut fresh: Vec<String> = Vec::new();
    let mut file = 0i64;
    while (file as usize) < analysis.files.len() {
        let path = match analysis.files.get(file as usize) {
            Some(pair) => pair.0.clone(),
            None => break,
        };
        let uri = path_to_uri(&path);
        let diags = file_diagnostics(&analysis, file);
        send_diagnostics(connection, &uri, diags)?;
        fresh.push(uri);
        file += 1;
    }
    // Clear diagnostics for files that dropped out of the module graph.
    let mut idx = 0usize;
    while idx < state.published.len() {
        let stale = match state.published.get(idx) {
            Some(uri) => !fresh.contains(uri),
            None => false,
        };
        if stale {
            if let Some(uri) = state.published.get(idx) {
                let cleared: Vec<Value> = Vec::new();
                send_diagnostics(connection, &uri.clone(), cleared)?;
            }
        }
        idx += 1;
    }
    state.published = fresh;
    Ok(())
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
                        if note.0 == idx as i64 && note.2 != NO_FILE {
                            if let Some(location) = location_json(analysis, note.2, note.3, note.4) {
                                related.push(json!({ "location": location, "message": note.1 }));
                            }
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
            if !related.is_empty() {
                if let Some(object) = diag_json.as_object_mut() {
                    object.insert("relatedInformation".to_string(), Value::Array(related));
                }
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
    if (b'a'..=b'f').contains(&byte) {
        return Some(byte - b'a' + 10);
    }
    if (b'A'..=b'F').contains(&byte) {
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
        if byte == b'%' && idx + 2 < bytes.len() + 1 {
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
    // a local file.
    let path_part = if let Some(after) = rest.strip_prefix('/') {
        // "file:///..." — after is the absolute path (minus its leading
        // slash on Windows drive paths).
        after
    } else {
        return None;
    };
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
