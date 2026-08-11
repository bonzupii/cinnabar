use serde_json::{json, Value};
use std::error::Error;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

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
        match self.receiver.recv_timeout(Duration::from_secs(15)) {
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
) -> Result<Value, Box<dyn Error>> {
    loop {
        let message = reader.next()?;
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
    let broken_entry_diagnostics = read_diagnostics_for_uri(&reader, &uri)?;
    assert!(
        diagnostic_count(&broken_entry_diagnostics).is_some_and(|count| count > 0),
        "entry-file arity error was not published: {}",
        broken_entry_diagnostics
    );

    let module = fixture("multi_file/Math.cnb");
    let module_uri = file_uri(&module);
    let module_text = std::fs::read_to_string(&module)?;
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": module_uri,
                    "languageId": "cinnabar",
                    "version": 1,
                    "text": module_text
                }
            }
        }),
    )?;
    let entry_after_module_open = read_diagnostics_for_uri(&reader, &uri)?;
    assert!(
        diagnostic_count(&entry_after_module_open).is_some_and(|count| count > 0),
        "opening an imported module cleared entry diagnostics: {}",
        entry_after_module_open
    );
    let clean_module = read_diagnostics_for_uri(&reader, &module_uri)?;
    assert_eq!(diagnostic_count(&clean_module), Some(0));

    let broken_module = "pub fun add(a: I64, b: I64) I64\n  return a + missing\nend\n";
    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": module_uri, "version": 2 },
                "contentChanges": [{ "text": broken_module }]
            }
        }),
    )?;
    let entry_after_module_change = read_diagnostics_for_uri(&reader, &uri)?;
    assert!(diagnostic_count(&entry_after_module_change).is_some_and(|count| count > 0));
    let broken_module_diagnostics = read_diagnostics_for_uri(&reader, &module_uri)?;
    assert!(
        diagnostic_count(&broken_module_diagnostics).is_some_and(|count| count > 0),
        "module overlay error was not published: {}",
        broken_module_diagnostics
    );

    send_message(
        &mut writer,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": { "textDocument": { "uri": module_uri } }
        }),
    )?;
    let entry_after_module_close = read_diagnostics_for_uri(&reader, &uri)?;
    assert!(diagnostic_count(&entry_after_module_close).is_some_and(|count| count > 0));
    let module_after_close = read_diagnostics_for_uri(&reader, &module_uri)?;
    assert_eq!(
        diagnostic_count(&module_after_close),
        Some(0),
        "closing the module left stale overlay diagnostics: {}",
        module_after_close
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
