use serde_json::{json, Value};
use std::error::Error;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

fn fixture(rel: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), rel)
}

fn file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{}", normalized)
    } else {
        format!("file:///{}", normalized)
    }
}

fn send_message(writer: &mut impl Write, message: &Value) -> Result<(), Box<dyn Error>> {
    let body = message.to_string();
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    writer.flush()?;
    Ok(())
}

fn read_message(reader: &mut BufReader<ChildStdout>) -> Result<Value, Box<dyn Error>> {
    let mut content_length = None;
    loop {
        let mut header = String::new();
        let bytes_read = reader.read_line(&mut header)?;
        if bytes_read == 0 {
            return Err("cinnabar-lsp closed stdout before sending a complete message".into());
        }
        if header == "\r\n" {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length: ") {
            content_length = Some(value.trim().parse::<usize>()?);
        }
    }
    let length = content_length.ok_or("LSP message had no Content-Length header")?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

struct MessageReader {
    receiver: Receiver<Result<Value, String>>,
}

impl MessageReader {
    fn new(stdout: ChildStdout) -> Self {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader) {
                    Ok(message) => {
                        if sender.send(Ok(message)).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        eprintln!("failed to read cinnabar-lsp message: {}", error);
                        return;
                    }
                }
            }
        });
        Self { receiver }
    }

    fn next(&self) -> Result<Value, Box<dyn Error>> {
        self.next_within(Duration::from_secs(15))
    }

    fn next_within(&self, timeout: Duration) -> Result<Value, Box<dyn Error>> {
        match self.receiver.recv_timeout(timeout) {
            Ok(Ok(message)) => Ok(message),
            Ok(Err(message)) => Err(message.into()),
            Err(error) => Err(format!("timed out waiting for an LSP message: {}", error).into()),
        }
    }
}

fn read_notification(
    reader: &MessageReader,
    expected_method: &str,
) -> Result<Value, Box<dyn Error>> {
    loop {
        let message = reader.next()?;
        if message.get("method").and_then(Value::as_str) == Some(expected_method) {
            return Ok(message);
        }
    }
}

fn read_diagnostics_for_uri(
    reader: &MessageReader,
    expected_uri: &str,
    stage: &str,
) -> Result<Value, Box<dyn Error>> {
    let mut observed: Vec<String> = Vec::new();
    loop {
        let message = match reader.next() {
            Ok(value) => value,
            Err(error) => {
                return Err(format!("{}: {}; observed {:?}", stage, error, observed).into());
            }
        };
        let is_diagnostics = message.get("method").and_then(Value::as_str)
            == Some("textDocument/publishDiagnostics");
        let uri_matches = message
            .get("params")
            .and_then(|params| params.get("uri"))
            .and_then(Value::as_str)
            == Some(expected_uri);
        if is_diagnostics && uri_matches {
            return Ok(message);
        }
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("response");
        let uri = message
            .get("params")
            .and_then(|params| params.get("uri"))
            .and_then(Value::as_str)
            .unwrap_or("no-uri");
        let detail = message
            .get("params")
            .and_then(|params| params.get("message"))
            .and_then(Value::as_str)
            .unwrap_or(uri);
        observed.push(format!("{} {}", method, detail));
    }
}

fn diagnostic_count(message: &Value) -> Option<usize> {
    message
        .get("params")
        .and_then(|params| params.get("diagnostics"))
        .and_then(Value::as_array)
        .map(Vec::len)
}

fn read_response(
    reader: &MessageReader,
    expected_id: i64,
) -> Result<Value, Box<dyn Error>> {
    loop {
        let message = reader.next()?;
        if message.get("id").and_then(Value::as_i64) == Some(expected_id) {
            return Ok(message);
        }
    }
}

fn read_response_within(
    reader: &MessageReader,
    expected_id: i64,
    timeout: Duration,
) -> Result<Value, Box<dyn Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = match deadline.checked_duration_since(Instant::now()) {
            Some(duration) => duration,
            None => return Err(format!("timed out waiting for LSP response {}", expected_id).into()),
        };
        let message = reader.next_within(remaining)?;
        if message.get("id").and_then(Value::as_i64) == Some(expected_id) {
            return Ok(message);
        }
    }
}

#[test]
fn stdio_hover_for_large_http_server_completes_within_bound() -> Result<(), Box<dyn Error>> {
    // This is deliberately generous enough for the complete compiler
    // pipeline on a debug build, but finite: a hover may not monopolize the
    // server indefinitely on a valid large source file.
    let hover_timeout = Duration::from_secs(10);
    let mut child = Command::new(env!("CARGO_BIN_EXE_cinnabar-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let outcome = (|| -> Result<(), Box<dyn Error>> {
        let mut writer = child.stdin.take().ok_or("cinnabar-lsp stdin was not piped")?;
        let stdout = child.stdout.take().ok_or("cinnabar-lsp stdout was not piped")?;
        let reader = MessageReader::new(stdout);

        send_message(
            &mut writer,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "capabilities": {}, "clientInfo": { "name": "hover-bound-test" } }
            }),
        )?;
        let initialized = read_response_within(&reader, 1, hover_timeout)
            .map_err(|error| format!("large-file hover initialization: {}", error))?;
        let hover_enabled = initialized
            .pointer("/result/capabilities/hoverProvider")
            .and_then(Value::as_bool);
        if hover_enabled != Some(true) {
            return Err(format!("hover capability was not enabled: {}", initialized).into());
        }
        send_message(
            &mut writer,
            &json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
        )?;

        let entry = fixture("http_server.cnb");
        let uri = file_uri(&entry);
        let source = std::fs::read_to_string(&entry)?;
        send_message(
            &mut writer,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "languageId": "cinnabar",
                        "version": 1,
                        "text": source
                    }
                }
            }),
        )?;
        let diagnostics = read_diagnostics_for_uri(&reader, &uri, "large-file diagnostics")?;
        assert_eq!(diagnostic_count(&diagnostics), Some(0), "diagnostics: {}", diagnostics);
        send_message(
            &mut writer,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/hover",
                "params": {
                    "textDocument": { "uri": uri },
                    "position": { "line": 7, "character": 12 }
                }
            }),
        )?;
        let hover = read_response_within(&reader, 2, hover_timeout)
            .map_err(|error| format!("large-file hover request: {}", error))?;
        let markdown = hover
            .pointer("/result/contents/value")
            .and_then(Value::as_str)
            .ok_or("large-file hover response had no markdown value")?;
        if !markdown.contains("**const**")
            || !markdown.contains("SERVER_PORT")
            || !markdown.contains("Usize")
        {
            return Err(format!("large-file hover did not describe SERVER_PORT: {}", hover).into());
        }
        Ok(())
    })();

    let kill_result = child.kill();
    let status = child.wait()?;
    if outcome.is_ok() && kill_result.is_err() && !status.success() {
        return Err(format!("cinnabar-lsp exited before test cleanup: {}", status).into());
    }
    outcome
}

#[test]
fn stdio_hover_during_http_server_diagnostics_responds_without_duplicate_analysis() -> Result<(), Box<dyn Error>> {
    // VS Code sends `initialized`, opens the buffer (which schedules
    // diagnostics), and can then replace a hover request before diagnostics
    // finish.  Both hover requests need a terminal response promptly; once
    // the authoritative diagnostic analysis finishes, a later hover must
    // answer from those attached facts.
    let prompt_timeout = Duration::from_secs(1);
    let mut child = Command::new(env!("CARGO_BIN_EXE_cinnabar-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let outcome = (|| -> Result<(), Box<dyn Error>> {
        let mut writer = child.stdin.take().ok_or("cinnabar-lsp stdin was not piped")?;
        let stdout = child.stdout.take().ok_or("cinnabar-lsp stdout was not piped")?;
        let reader = MessageReader::new(stdout);

        send_message(
            &mut writer,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "capabilities": {}, "clientInfo": { "name": "diagnostics-hover-test" } }
            }),
        )?;
        let initialized = read_response_within(&reader, 1, Duration::from_secs(10))?;
        if initialized.pointer("/result/capabilities/hoverProvider").and_then(Value::as_bool) != Some(true) {
            return Err(format!("hover capability was not enabled: {}", initialized).into());
        }
        send_message(
            &mut writer,
            &json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
        )?;

        let entry = fixture("http_server.cnb");
        let uri = file_uri(&entry);
        let source = std::fs::read_to_string(&entry)?;
        send_message(
            &mut writer,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "languageId": "cinnabar",
                        "version": 1,
                        "text": source
                    }
                }
            }),
        )?;

        // The server debounces diagnostics for 75 ms.  This delay makes the
        // hover overlap the full-document diagnostic analysis rather than
        // accidentally testing the no-work-before-debounce interval.
        std::thread::sleep(Duration::from_millis(150));
        send_message(
            &mut writer,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/hover",
                "params": {
                    "textDocument": { "uri": uri },
                    "position": { "line": 7, "character": 12 }
                }
            }),
        )?;
        send_message(
            &mut writer,
            &json!({
                "jsonrpc": "2.0",
                "method": "$/cancelRequest",
                "params": { "id": 2 }
            }),
        )?;
        send_message(
            &mut writer,
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "textDocument/hover",
                "params": {
                    "textDocument": { "uri": uri },
                    "position": { "line": 7, "character": 12 }
                }
            }),
        )?;

        let deadline = Instant::now() + prompt_timeout;
        let mut first_hover: Option<Value> = None;
        let mut replacement_hover: Option<Value> = None;
        let mut diagnostics_arrived = false;
        while first_hover.is_none() || replacement_hover.is_none() {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or("hover requests did not receive terminal responses while diagnostics were active")?;
            let message = reader.next_within(remaining)?;
            let response_id = message.get("id").and_then(Value::as_i64);
            if response_id == Some(2) {
                first_hover = Some(message);
            } else if response_id == Some(3) {
                replacement_hover = Some(message);
            } else {
                let is_diagnostics = message.get("method").and_then(Value::as_str)
                    == Some("textDocument/publishDiagnostics");
                let is_entry_diagnostics = message
                    .get("params")
                    .and_then(|params| params.get("uri"))
                    .and_then(Value::as_str)
                    == Some(uri.as_str());
                if is_diagnostics && is_entry_diagnostics {
                    diagnostics_arrived = true;
                }
            }
        }

        let first_hover = first_hover.ok_or("first hover response was absent")?;
        let replacement_hover = replacement_hover.ok_or("replacement hover response was absent")?;
        for response in [&first_hover, &replacement_hover] {
            let result = response.get("result").ok_or("hover response had no result")?;
            let describes_server_port = result
                .pointer("/contents/value")
                .and_then(Value::as_str)
                .is_some_and(|markdown| markdown.contains("SERVER_PORT") && markdown.contains("Usize"));
            if !result.is_null() && !describes_server_port {
                return Err(format!("overlapping hover had an unexpected result: {}", response).into());
            }
        }

        if !diagnostics_arrived {
            let diagnostics = read_diagnostics_for_uri(&reader, &uri, "http_server diagnostics")?;
            assert_eq!(diagnostic_count(&diagnostics), Some(0), "diagnostics: {}", diagnostics);
        }

        send_message(
            &mut writer,
            &json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "textDocument/hover",
                "params": {
                    "textDocument": { "uri": uri },
                    "position": { "line": 7, "character": 12 }
                }
            }),
        )?;
        let cached_hover = read_response_within(&reader, 4, prompt_timeout)?;
        let markdown = cached_hover
            .pointer("/result/contents/value")
            .and_then(Value::as_str)
            .ok_or("hover after diagnostics had no markdown value")?;
        if !markdown.contains("SERVER_PORT") || !markdown.contains("Usize") {
            return Err(format!("hover after diagnostics did not reuse SERVER_PORT facts: {}", cached_hover).into());
        }

        // A document update invalidates the attached snapshot before the
        // next diagnostic worker starts.  The server must not expose the
        // old Usize fact during that gap; after the matching generation
        // completes, the same hover must expose the edited I64 fact.
        let edited_source = source.replacen(
            "pub const SERVER_PORT: Usize = 4067",
            "pub const SERVER_PORT: I64 = 4067",
            1,
        ).replacen(
            "val bind_result = bind(&listener, SERVER_PORT)",
            "val bind_result = bind(&listener, Usize.from(SERVER_PORT))",
            1,
        );
        send_message(
            &mut writer,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": uri, "version": 2 },
                    "contentChanges": [{ "text": edited_source }]
                }
            }),
        )?;
        send_message(
            &mut writer,
            &json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "textDocument/hover",
                "params": {
                    "textDocument": { "uri": uri },
                    "position": { "line": 7, "character": 12 }
                }
            }),
        )?;
        let invalidated_hover = read_response_within(&reader, 5, prompt_timeout)?;
        if invalidated_hover.get("result") != Some(&Value::Null) {
            return Err(format!("stale analysis answered a new document generation: {}", invalidated_hover).into());
        }
        let edited_diagnostics = read_diagnostics_for_uri(&reader, &uri, "edited http_server diagnostics")?;
        assert_eq!(diagnostic_count(&edited_diagnostics), Some(0), "diagnostics: {}", edited_diagnostics);
        send_message(
            &mut writer,
            &json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "textDocument/hover",
                "params": {
                    "textDocument": { "uri": uri },
                    "position": { "line": 7, "character": 12 }
                }
            }),
        )?;
        let edited_hover = read_response_within(&reader, 6, prompt_timeout)?;
        let edited_markdown = edited_hover
            .pointer("/result/contents/value")
            .and_then(Value::as_str)
            .ok_or("hover after edited diagnostics had no markdown value")?;
        if !edited_markdown.contains("SERVER_PORT") || !edited_markdown.contains("I64") {
            return Err(format!("hover after edited diagnostics did not use I64 facts: {}", edited_hover).into());
        }
        Ok(())
    })();

    let kill_result = child.kill();
    let status = child.wait()?;
    if outcome.is_ok() && kill_result.is_err() && !status.success() {
        return Err(format!("cinnabar-lsp exited before test cleanup: {}", status).into());
    }
    outcome
}

#[test]
fn stdio_server_handles_overlay_diagnostics_hover_and_shutdown() -> Result<(), Box<dyn Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cinnabar-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut writer = child.stdin.take().ok_or("cinnabar-lsp stdin was not piped")?;
    let stdout = child.stdout.take().ok_or("cinnabar-lsp stdout was not piped")?;
    let reader = MessageReader::new(stdout);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "capabilities": {}, "clientInfo": { "name": "protocol-test" } }
        }),
    )?;
    let initialized = read_response(&reader, 1)?;
    assert_eq!(
        initialized
            .get("result")
            .and_then(|result| result.get("capabilities"))
            .and_then(|capabilities| capabilities.get("hoverProvider"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        initialized
            .get("result")
            .and_then(|result| result.get("capabilities"))
            .and_then(|capabilities| capabilities.get("codeLensProvider"))
            .and_then(|provider| provider.get("resolveProvider"))
            .and_then(Value::as_bool),
        Some(false)
    );
    send_message(
        &mut writer,
        &json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    )?;

    let entry = fixture("multi_file/main.cnb");
    let uri = file_uri(&entry);
    let overlay = "use Math.add\n\npub fun main() I64\n  return add(30, 40)\nend\n";
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "cinnabar",
                    "version": 1,
                    "text": overlay
                }
            }
        }),
    )?;
    let diagnostics = read_notification(&reader, "textDocument/publishDiagnostics")?;
    assert_eq!(
        diagnostics
            .get("params")
            .and_then(|params| params.get("uri"))
            .and_then(Value::as_str),
        Some(uri.as_str())
    );
    assert_eq!(
        diagnostics
            .get("params")
            .and_then(|params| params.get("diagnostics"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 10 }
            }
        }),
    )?;
    let hover = read_response(&reader, 2)?;
    let hover_text = hover
        .get("result")
        .and_then(|result| result.get("contents"))
        .and_then(|contents| contents.get("value"))
        .and_then(Value::as_str)
        .ok_or("hover response had no markdown value")?;
    assert!(hover_text.contains("fun add(a: I64, b: I64) I64"), "hover: {}", hover_text);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 10 }
            }
        }),
    )?;
    let definition = read_response(&reader, 20)?;
    let definition_uri = definition
        .get("result")
        .and_then(|result| result.get("uri"))
        .and_then(Value::as_str)
        .ok_or("definition response had no URI")?;
    assert!(definition_uri.ends_with("Math.cnb"), "definition: {}", definition);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "textDocument/references",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 10 },
                "context": { "includeDeclaration": true }
            }
        }),
    )?;
    let references = read_response(&reader, 21)?;
    let reference_count = references.get("result").and_then(Value::as_array).map(Vec::len);
    assert!(reference_count.is_some_and(|count| count >= 2), "references: {}", references);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 22,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 2 }
            }
        }),
    )?;
    let completion = read_response(&reader, 22)?;
    let completion_items = completion.get("result").and_then(Value::as_array).ok_or("completion response was not an array")?;
    assert!(completion_items.iter().any(|item| item.get("label").and_then(Value::as_str) == Some("add")), "completion: {}", completion);

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 23,
            "method": "textDocument/signatureHelp",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": 3, "character": 18 }
            }
        }),
    )?;
    let signature = read_response(&reader, 23)?;
    assert_eq!(signature.get("result").and_then(|result| result.get("activeParameter")).and_then(Value::as_i64), Some(1));
    let signature_label = signature
        .pointer("/result/signatures/0/label")
        .and_then(Value::as_str)
        .ok_or("signature response had no label")?;
    assert!(signature_label.contains("fun add(a: I64, b: I64) I64"), "signature: {}", signature);

    // Rapid edits coalesce: the superseded broken generation must never be
    // published after the immediately-following valid generation.
    let broken_entry = "use Math.add\n\npub fun main() I64\n  return add(30)\nend\n";
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri, "version": 2 },
                "contentChanges": [{ "text": broken_entry }]
            }
        }),
    )?;
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri, "version": 3 },
                "contentChanges": [{ "text": overlay }]
            }
        }),
    )?;
    let latest = read_diagnostics_for_uri(&reader, &uri, "rapid-edit result")?;
    assert_eq!(
        latest
            .get("params")
            .and_then(|params| params.get("diagnostics"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0),
        "a superseded edit published diagnostics: {}",
        latest
    );

    // Keep a real entry error live while a secondary module opens, changes,
    // and closes.  Secondary buffers must stay in the entry graph and must
    // never erase the entry file's diagnostics.
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri, "version": 4 },
                "contentChanges": [{ "text": broken_entry }]
            }
        }),
    )?;
    let broken_entry_diagnostics = read_diagnostics_for_uri(&reader, &uri, "broken entry")?;
    assert!(
        diagnostic_count(&broken_entry_diagnostics).is_some_and(|count| count > 0),
        "entry-file arity error was not published: {}",
        broken_entry_diagnostics
    );

    let secondary = fixture("multi_file/Math.cnb");
    let secondary_uri = file_uri(&secondary);
    let secondary_text = std::fs::read_to_string(&secondary)?;
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": secondary_uri,
                    "languageId": "cinnabar",
                    "version": 1,
                    "text": secondary_text
                }
            }
        }),
    )?;
    let entry_after_secondary_open = read_diagnostics_for_uri(&reader, &uri, "entry after secondary open")?;
    assert!(
        diagnostic_count(&entry_after_secondary_open).is_some_and(|count| count > 0),
        "opening an imported module cleared entry diagnostics: {}",
        entry_after_secondary_open
    );
    let clean_secondary = read_diagnostics_for_uri(&reader, &secondary_uri, "clean secondary")?;
    assert_eq!(diagnostic_count(&clean_secondary), Some(0));

    let broken_secondary = "pub fun add(a: I64, b: I64) I64\n  return a + missing\nend\n";
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": secondary_uri, "version": 2 },
                "contentChanges": [{ "text": broken_secondary }]
            }
        }),
    )?;
    let entry_after_secondary_change = read_diagnostics_for_uri(&reader, &uri, "entry after secondary change")?;
    assert!(diagnostic_count(&entry_after_secondary_change).is_some_and(|count| count > 0));
    let secondary_diagnostics = read_diagnostics_for_uri(&reader, &secondary_uri, "broken secondary")?;
    assert!(
        diagnostic_count(&secondary_diagnostics).is_some_and(|count| count > 0),
        "module overlay error was not published: {}",
        secondary_diagnostics
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": { "textDocument": { "uri": secondary_uri } }
        }),
    )?;
    let secondary_after_close = read_diagnostics_for_uri(&reader, &secondary_uri, "secondary close clear")?;
    assert_eq!(
        diagnostic_count(&secondary_after_close),
        Some(0),
        "closing the module left stale overlay diagnostics: {}",
        secondary_after_close
    );
    let entry_after_secondary_close = read_diagnostics_for_uri(&reader, &uri, "entry after secondary close")?;
    assert!(diagnostic_count(&entry_after_secondary_close).is_some_and(|count| count > 0));

    let explain = fixture("explain_leak.cnb");
    let explain_uri = file_uri(&explain);
    let explain_text = std::fs::read_to_string(&explain)?;
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": explain_uri,
                    "languageId": "cinnabar",
                    "version": 1,
                    "text": explain_text
                }
            }
        }),
    )?;
    let explain_diagnostics = read_diagnostics_for_uri(&reader, &explain_uri, "explain diagnostics")?;
    let explain_count = explain_diagnostics
        .get("params")
        .and_then(|params| params.get("diagnostics"))
        .and_then(Value::as_array)
        .map(Vec::len)
        .ok_or("explain diagnostics were not an array")?;
    assert!(explain_count > 0, "explain fixture unexpectedly clean");
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "textDocument/codeLens",
            "params": { "textDocument": { "uri": explain_uri } }
        }),
    )?;
    let lenses = read_response(&reader, 4)?;
    let lens_count = lenses
        .get("result")
        .and_then(Value::as_array)
        .map(Vec::len)
        .ok_or("code-lens result was not an array")?;
    assert!(lens_count > 0, "borrow explanations were not exposed as code lenses: {}", lenses);

    // Reverse open order: Math.cnb first becomes a temporary standalone
    // root.  Opening main.cnb afterward must adopt it into main's larger
    // compiler-produced graph, so subsequent Math edits retain main's
    // importing context and diagnostics.
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": { "textDocument": { "uri": uri } }
        }),
    )?;
    let closed_entry = read_diagnostics_for_uri(&reader, &uri, "reverse-order entry clear")?;
    assert_eq!(diagnostic_count(&closed_entry), Some(0));
    let closed_secondary = read_diagnostics_for_uri(&reader, &secondary_uri, "reverse-order secondary clear")?;
    assert_eq!(diagnostic_count(&closed_secondary), Some(0));

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": secondary_uri,
                    "languageId": "cinnabar",
                    "version": 3,
                    "text": secondary_text
                }
            }
        }),
    )?;
    let standalone_secondary = read_diagnostics_for_uri(&reader, &secondary_uri, "standalone secondary")?;
    assert_eq!(diagnostic_count(&standalone_secondary), Some(0));

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "cinnabar",
                    "version": 5,
                    "text": broken_entry
                }
            }
        }),
    )?;
    let adopted_entry = read_diagnostics_for_uri(&reader, &uri, "adopted entry")?;
    assert!(
        diagnostic_count(&adopted_entry).is_some_and(|count| count > 0),
        "reverse-order entry diagnostics were not published: {}",
        adopted_entry
    );
    let adopted_secondary = read_diagnostics_for_uri(&reader, &secondary_uri, "adopted secondary")?;
    assert_eq!(diagnostic_count(&adopted_secondary), Some(0));

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": secondary_uri, "version": 4 },
                "contentChanges": [{ "text": broken_secondary }]
            }
        }),
    )?;
    let adopted_entry_after_change = read_diagnostics_for_uri(&reader, &uri, "adopted entry after change")?;
    assert!(diagnostic_count(&adopted_entry_after_change).is_some_and(|count| count > 0));
    let adopted_secondary_after_change = read_diagnostics_for_uri(&reader, &secondary_uri, "adopted secondary after change")?;
    assert!(
        diagnostic_count(&adopted_secondary_after_change).is_some_and(|count| count > 0),
        "reverse-order module edit escaped the entry graph: {}",
        adopted_secondary_after_change
    );

    send_message(
        &mut writer,
        &json!({ "jsonrpc": "2.0", "id": 3, "method": "shutdown", "params": null }),
    )?;
    let shutdown = read_response(&reader, 3)?;
    assert!(shutdown.get("error").is_none(), "shutdown response: {}", shutdown);
    send_message(
        &mut writer,
        &json!({ "jsonrpc": "2.0", "method": "exit", "params": null }),
    )?;
    drop(writer);

    let mut attempts = 0usize;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None => {
                if attempts >= 100 {
                    child.kill()?;
                    return Err("cinnabar-lsp did not exit within five seconds of shutdown".into());
                }
                std::thread::sleep(Duration::from_millis(50));
                attempts += 1;
            }
        }
    };
    let mut stderr_text = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        stderr.read_to_string(&mut stderr_text)?;
    }
    assert!(
        status.success(),
        "cinnabar-lsp failed: {}",
        stderr_text
    );
    Ok(())
}
