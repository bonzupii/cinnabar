//! Developer tools that sit beside the compiler rather than inside it.
//!
//! Five surfaces share this file because they share a shape — each drives
//! the real compiler binary as a subprocess and reports what it did.
//! `binary_report` shells out to the LLVM binutils for sections, symbols,
//! and disassembly. The Mushlings exercises are initialized from real
//! fixtures and verified by re-running the compiler over the learner's
//! edits. `replay_fuzz` and `minimize_fuzz` reproduce and then shrink a
//! failing artifact. `soundness_evidence` counts what the front end
//! actually established and emits it as JSON. `serve_playground` compiles
//! and runs submitted source over loopback HTTP.
//!
//! **Invariants:**
//! - The playground binds only loopback and runs submitted programs under a
//!   wall-clock limit with capped request bodies.
//! - Minimization preserves the failure signature, not merely the failure.
//! - Soundness evidence states `formal_proof: false` and scopes itself
//!   explicitly.
//! - An exercise requires a real diagnostic to teach.

use crate::ast::{node_tag, NODE_EXPR, NODE_INST, NODE_STRIDE, NODE_TRAIT, NODE_TY};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

// Host arch-os string from target descriptor.
pub fn host_target() -> String {
    let host = crate::target::Target::host();
    let arch = match host.arch {
        crate::target::TargetArch::X86_64 => "x86_64",
        crate::target::TargetArch::AArch64 => "aarch64",
    };
    let os = match host.os {
        crate::target::TargetOs::Linux => "linux",
        crate::target::TargetOs::Darwin => "darwin",
        crate::target::TargetOs::Bsd => "bsd",
        crate::target::TargetOs::Windows => "windows",
    };
    format!("{}-{}", arch, os)
}

fn run_capture(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|run_error| format!("cannot run {}: {}", program, run_error))?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: {}",
            program,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|utf8_error| format!("{} emitted non-UTF-8 output: {}", program, utf8_error))
}

pub fn binary_report(binary: &Path, layouts: &str) -> Result<String, String> {
    let binary_text = binary.to_string_lossy();
    let sections = run_capture("llvm-size", &["-A", binary_text.as_ref()])?;
    let symbols = run_capture("llvm-nm", &["--print-size", "--size-sort", binary_text.as_ref()])?;
    let disassembly = run_capture("llvm-objdump", &["-d", "--demangle", binary_text.as_ref()])?;
    Ok(format!(
        "Cinnabar binary inspection\nBinary: {}\nTarget: {}\nSource correlation: unavailable (the current backend does not emit debug line tables)\n\nTYPE LAYOUTS\n{}\nSECTIONS\n{}\nSYMBOLS BY SIZE\n{}\nDISASSEMBLY\n{}",
        binary.display(), host_target(), layouts, sections, symbols, disassembly
    ))
}

struct Exercise {
    file: &'static str,
    lesson: &'static str,
    source: &'static str,
    expected: &'static str,
    topic: &'static str,
}

fn exercises() -> Vec<Exercise> {
    vec![
        Exercise { file: "01_mixed_type.cnb", lesson: "A declaration is either a struct or an enum, never both.", source: include_str!("../tests/fixtures/invalid_mixed_type.cnb"), expected: "syntax", topic: "mix struct fields" },
        Exercise { file: "02_linear_paths.cnb", lesson: "A linear handle must be consumed exactly once on every path.", source: include_str!("../tests/fixtures/explain_leak.cnb"), expected: "linear", topic: "consumed on some paths" },
        Exercise { file: "03_unhandled_result.cnb", lesson: "A Result must be handled with try or match.", source: include_str!("../tests/fixtures/mushling_unhandled_result.cnb"), expected: "type_error", topic: "unhandled Result" },
        Exercise { file: "04_ambiguous_borrow.cnb", lesson: "A returned borrow must have one unambiguous input origin.", source: include_str!("../tests/fixtures/repro/ret_borrow_ambiguous.cnb"), expected: "linear", topic: "ambiguous returned borrow" },
        Exercise { file: "05_const_division.cnb", lesson: "Compile-time division by zero is rejected.", source: include_str!("../tests/fixtures/repro/div_zero_const.cnb"), expected: "type_error", topic: "division by zero" },
        Exercise { file: "06_integer_range.cnb", lesson: "Integer literals must fit their declared type.", source: include_str!("../tests/fixtures/repro/int_literal_range.cnb"), expected: "type_error", topic: "out of range" },
        Exercise { file: "07_recursion.cnb", lesson: "Recursion must be tail-recursive.", source: include_str!("../tests/fixtures/repro/non_tail_recursion.cnb"), expected: "type_error", topic: "non-tail recursive" },
        Exercise { file: "08_dropped_pub.cnb", lesson: "An item that is not public cannot be used from outside the module that declares it.", source: include_str!("../tests/fixtures/08_dropped_pub.cnb"), expected: "private_access", topic: "cannot call" },
        Exercise { file: "09_discard_patterns.cnb", lesson: "A value is bound with a real name and used; there is no discard.", source: include_str!("../tests/fixtures/09_discard_patterns.cnb"), expected: "syntax", topic: "discard pattern" },
    ]
}

pub fn initialize_mushlings(directory: &Path) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .map_err(|create_error| format!("cannot create '{}': {}", directory.display(), create_error))?;
    let mut guide = String::from("# Cinnabar Mushlings\n\nFix each program, then run `cinnabar mushlings verify`.\n\n");
    for exercise in exercises() {
        let destination = directory.join(exercise.file);
        if !destination.exists() {
            std::fs::write(&destination, exercise.source)
                .map_err(|write_error| format!("cannot write '{}': {}", destination.display(), write_error))?;
        }
        guide.push_str(&format!("- `{}`: {}\n", exercise.file, exercise.lesson));
    }
    std::fs::write(directory.join("README.md"), guide)
        .map_err(|write_error| format!("cannot write Mushlings guide: {}", write_error))
}

fn compiler_output(executable: &Path, source: &Path) -> Result<Output, String> {
    Command::new(executable)
        .arg(source)
        .arg("--check-only")
        .output()
        .map_err(|run_error| format!("cannot launch compiler: {}", run_error))
}

fn combined_output(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

// Single --check-only --emit-json run.
fn compiler_json(executable: &Path, source: &Path) -> Result<(bool, Value), String> {
    let output = Command::new(executable)
        .arg(source)
        .arg("--check-only")
        .arg("--emit-json")
        .output()
        .map_err(|run_error| format!("cannot launch compiler: {}", run_error))?;
    let report: Value = serde_json::from_slice(&output.stdout)
        .map_err(|parse_error| format!("compiler emitted invalid JSON: {}", parse_error))?;
    Ok((output.status.success(), report))
}

// Category names in diagnostics envelope.
fn categories_of(report: &Value) -> Vec<String> {
    let mut categories: Vec<String> = Vec::new();
    let diagnostics = match report.get("diagnostics").and_then(|entry| entry.as_array()) {
        Some(value) => value,
        None => return categories,
    };
    for diagnostic in diagnostics {
        if let Some(category) = diagnostic.get("category").and_then(|entry| entry.as_str()) {
            categories.push(category.to_string());
        }
    }
    categories
}

// Checks if envelope contains expected category and topic.
fn diagnostic_matches(report: &Value, category: &str, topic: &str) -> bool {
    let diagnostics = match report.get("diagnostics").and_then(|entry| entry.as_array()) {
        Some(value) => value,
        None => return false,
    };
    for diagnostic in diagnostics {
        let cat = match diagnostic.get("category").and_then(|entry| entry.as_str()) {
            Some(value) => value,
            None => continue,
        };
        if cat != category {
            continue;
        }
        let message = diagnostic.get("message").and_then(|entry| entry.as_str()).unwrap_or_default();
        if message.contains(topic) {
            return true;
        }
    }
    false
}

pub fn verify_mushlings(directory: &Path, executable: &Path) -> Result<(usize, usize, Vec<String>), String> {
    let mut solved = 0usize;
    let mut pending = 0usize;
    let mut progress = Vec::new();
    for exercise in exercises() {
        let path = directory.join(exercise.file);
        if !path.exists() {
            return Err(format!("missing exercise '{}'; run mushlings init", path.display()));
        }
        let (passed, report) = compiler_json(executable, &path)?;
        if passed {
            progress.push(format!("solved  {}", exercise.file));
            solved += 1;
        } else if diagnostic_matches(&report, exercise.expected, exercise.topic) {
            progress.push(format!("pending {} — {}", exercise.file, exercise.lesson));
            pending += 1;
        } else {
            let categories = categories_of(&report);
            return Err(format!("{} now fails for an unexpected reason: {:?}", exercise.file, categories));
        }
    }
    Ok((solved, pending, progress))
}

pub fn replay_fuzz(executable: &Path, source: &Path) -> Result<(bool, String), String> {
    let output = compiler_output(executable, source)?;
    Ok((output.status.success(), combined_output(&output)))
}

fn failure_signature(report: &Value) -> Option<String> {
    let diagnostics = report.get("diagnostics")?.as_array()?;
    if diagnostics.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    for diagnostic in diagnostics {
        let category = diagnostic.get("category").and_then(|entry| entry.as_str()).unwrap_or("diagnostic");
        let message = diagnostic.get("message").and_then(|entry| entry.as_str()).unwrap_or_default();
        parts.push(format!("{}: {}", category, message));
    }
    Some(parts.join("\n"))
}

pub fn minimize_fuzz(executable: &Path, source: &Path, destination: &Path) -> Result<usize, String> {
    let original = std::fs::read_to_string(source)
        .map_err(|read_error| format!("cannot read '{}': {}", source.display(), read_error))?;
    let (baseline_passed, baseline_report) = compiler_json(executable, source)?;
    if baseline_passed {
        return Err("the supplied fuzz artifact does not reproduce a compiler failure".to_string());
    }
    let signature = failure_signature(&baseline_report)
        .ok_or_else(|| "the failing artifact produced no stable error signature".to_string())?;
    let scratch = std::env::temp_dir().join(format!("cinnabar-fuzz-minimize-{}.cnb", std::process::id()));
    let mut lines: Vec<String> = original.lines().map(str::to_string).collect();
    let mut cursor = 0usize;
    while cursor < lines.len() {
        let mut candidate = lines.clone();
        candidate.remove(cursor);
        let candidate_text = format!("{}\n", candidate.join("\n"));
        std::fs::write(&scratch, candidate_text)
            .map_err(|write_error| format!("cannot write minimization scratch file: {}", write_error))?;
        let (trial_passed, trial_report) = compiler_json(executable, &scratch)?;
        let trial_signature = failure_signature(&trial_report);
        if !trial_passed && trial_signature.as_deref() == Some(signature.as_str()) {
            lines = candidate;
        } else {
            cursor += 1;
        }
    }
    std::fs::write(destination, format!("{}\n", lines.join("\n")))
        .map_err(|write_error| format!("cannot write '{}': {}", destination.display(), write_error))?;
    if scratch.exists() {
        std::fs::remove_file(&scratch)
            .map_err(|remove_error| format!("cannot remove minimization scratch file: {}", remove_error))?;
    }
    Ok(lines.len())
}

pub fn soundness_evidence(entry: &Path, nodes: &[i64], errors: usize) -> String {
    let mut expressions = 0usize;
    let mut types = 0usize;
    let mut instantiations = 0usize;
    let mut traits = 0usize;
    let mut offset = 0usize;
    while offset + NODE_STRIDE as usize <= nodes.len() {
        let id = (offset / NODE_STRIDE as usize) as i64;
        let tag = node_tag(nodes, id);
        if tag == NODE_EXPR {
            expressions += 1;
        } else if tag == NODE_TY {
            types += 1;
        } else if tag == NODE_INST {
            instantiations += 1;
        } else if tag == NODE_TRAIT {
            traits += 1;
        }
        offset += NODE_STRIDE as usize;
    }
    let evidence = json!({
        "schema": "cinnabar.soundness-evidence.v1",
        "compiler_version": env!("CARGO_PKG_VERSION"),
        "entry": entry,
        "formal_proof": false,
        "front_end": {
            "resolved": errors == 0,
            "typechecked": errors == 0,
            "borrow_checked": errors == 0,
            "diagnostics": errors
        },
        "typed_arena": {
            "expressions": expressions,
            "type_nodes": types,
            "generic_instantiations": instantiations,
            "trait_dispatches": traits
        },
        "scope": "Machine-checkable compiler evidence; not a mechanized preservation/progress proof."
    });
    match serde_json::to_string_pretty(&evidence) {
        Ok(text) => text,
        Err(serialize_error) => format!("{{\"serialization_error\":\"{}\"}}", serialize_error),
    }
}

fn playground_page() -> &'static str {
    "<!doctype html><meta charset=utf-8><title>Cinnabar Playground</title><style>body{font:16px system-ui;max-width:900px;margin:2rem auto}textarea{width:100%;height:24rem}pre{white-space:pre-wrap;background:#111;color:#eee;padding:1rem}</style><h1>Cinnabar Playground</h1><p>Runs locally on loopback using this compiler and its static runtime.</p><textarea id=s>fun main() I32\n  return 0\nend\n</textarea><button onclick=run()>Compile and run</button><pre id=o></pre><script>async function run(){o.textContent='Compiling';let r=await fetch('/run',{method:'POST',body:s.value});o.textContent=await r.text()}</script>"
}

fn read_request(stream: &mut TcpStream) -> Result<(String, Vec<u8>), String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let count = stream.read(&mut buffer).map_err(|read_error| format!("cannot read request: {}", read_error))?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if bytes.len() > 1_048_576 {
            return Err("playground request exceeds one MiB".to_string());
        }
    }
    let header_end = bytes.windows(4).position(|window| window == b"\r\n\r\n").ok_or_else(|| "malformed HTTP request".to_string())? + 4;
    let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
    let found_length = headers.lines().find_map(|line| {
        let lowered = line.to_ascii_lowercase();
        lowered.strip_prefix("content-length:").and_then(|value| value.trim().parse::<usize>().ok())
    });
    let mut length = 0usize;
    if let Some(value) = found_length {
        length = value;
    }
    if length > 1_048_576 {
        return Err("playground request body exceeds one MiB".to_string());
    }
    while bytes.len() < header_end + length {
        let count = stream.read(&mut buffer).map_err(|read_error| format!("cannot read request body: {}", read_error))?;
        if count == 0 {
            return Err(format!(
                "playground request body ended prematurely: declared {} bytes, received {}",
                length,
                bytes.len() - header_end
            ));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok((headers, bytes[header_end..].to_vec()))
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) -> Result<(), String> {
    let headers = format!("HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", status, content_type, body.len());
    stream.write_all(headers.as_bytes()).map_err(|write_error| format!("cannot write response headers: {}", write_error))?;
    stream.write_all(body).map_err(|write_error| format!("cannot write response body: {}", write_error))
}

fn handle_playground(stream: &mut TcpStream, executable: &Path) -> Result<(), String> {
    let (headers, body) = read_request(stream)?;
    if headers.starts_with("GET / ") {
        return respond(stream, "200 OK", "text/html; charset=utf-8", playground_page().as_bytes());
    }
    if headers.starts_with("POST /run ") {
        let scratch = std::env::temp_dir().join(format!("cinnabar-playground-{}.cnb", std::process::id()));
        let binary = std::env::temp_dir().join(format!("cinnabar-playground-{}-bin", std::process::id()));
        std::fs::write(&scratch, body).map_err(|write_error| format!("cannot write playground source: {}", write_error))?;
        let compiled = Command::new(executable)
            .arg(&scratch)
            .arg("-o")
            .arg(&binary)
            .output()
            .map_err(|run_error| format!("cannot launch playground compiler: {}", run_error))?;
        let mut text = combined_output(&compiled);
        if compiled.status.success() {
            match execute_with_timeout(&binary, Duration::from_secs(5)) {
                Ok(executed) => {
                    text.push_str(&combined_output(&executed));
                    text.push_str(&format!("\nProgram exit status: {}\n", executed.status));
                }
                Err(execution_error) => {
                    text.push_str(&format!("\nExecution error: {}\n", execution_error));
                }
            }
        }
        std::fs::remove_file(&scratch).map_err(|remove_error| format!("cannot remove playground source: {}", remove_error))?;
        if binary.exists() {
            std::fs::remove_file(&binary).map_err(|remove_error| format!("cannot remove playground binary: {}", remove_error))?;
        }
        return respond(stream, "200 OK", "text/plain; charset=utf-8", text.as_bytes());
    }
    respond(stream, "404 Not Found", "text/plain; charset=utf-8", b"not found")
}

fn execute_with_timeout(binary: &Path, limit: Duration) -> Result<Output, String> {
    let mut child = Command::new(binary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|run_error| format!("cannot execute playground binary: {}", run_error))?;
    let started = Instant::now();
    loop {
        let status = child.try_wait().map_err(|wait_error| format!("cannot monitor playground binary: {}", wait_error))?;
        if status.is_some() {
            return child.wait_with_output().map_err(|wait_error| format!("cannot collect playground output: {}", wait_error));
        }
        if started.elapsed() >= limit {
            child.kill().map_err(|kill_error| format!("cannot stop timed-out playground binary: {}", kill_error))?;
            let completed = child.wait_with_output().map_err(|wait_error| format!("cannot collect timed-out playground output: {}", wait_error))?;
            return Err(format!("playground program exceeded the {} second execution limit; stdout: {}; stderr: {}", limit.as_secs(), String::from_utf8_lossy(&completed.stdout), String::from_utf8_lossy(&completed.stderr)));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub fn serve_playground(
    address: &str,
    executable: &Path,
    mut report_error: impl FnMut(&str),
) -> Result<(), String> {
    let parsed: SocketAddr = address.parse().map_err(|parse_error| format!("invalid playground address: {}", parse_error))?;
    if !parsed.ip().is_loopback() {
        return Err("the local playground may bind only to a loopback address".to_string());
    }
    let listener = TcpListener::bind(parsed).map_err(|bind_error| format!("cannot bind playground: {}", bind_error))?;
    // Only bind is fatal; per-connection failures are reported without stopping server.
    for incoming in listener.incoming() {
        let mut stream = match incoming {
            Ok(stream) => stream,
            Err(accept_error) => {
                let message = format!("cannot accept playground connection: {}", accept_error);
                report_error(&message);
                continue;
            }
        };
        match handle_playground(&mut stream, executable) {
            Ok(()) => {}
            Err(request_error) => {
                let written = respond(
                    &mut stream,
                    "400 Bad Request",
                    "text/plain; charset=utf-8",
                    request_error.as_bytes(),
                );
                if let Err(write_error) = written {
                    let message = format!("cannot write playground error response: {}", write_error);
                    report_error(&message);
                }
            }
        }
    }
    Ok(())
}

pub fn default_minimized_path(source: &Path) -> PathBuf {
    source.with_extension("min.cnb")
}
