//! The CLI tool surfaces, exercised end to end through the built binary.
//!
//! Drives the compiler executable as a subprocess across the tool commands:
//! the project and inspection surfaces, the attached-fact and formatter
//! flags, and the documentation and playground servers over real HTTP on an
//! OS-assigned loopback port. It also checks that the editor package
//! references language assets that actually exist.
//!
//! **Invariants:**
//! - Each server test binds a port the OS chooses and stops its child
//!   afterward, so the suite stays runnable in parallel and leaves nothing
//!   listening behind it.
//! - Scratch directories are unique per process and per invocation.
//! - These go through the CLI rather than the library. A flag that parses
//!   but is wired to nothing is exactly the failure they exist to catch,
//!   and only the real binary can show it.

use serde_json::Value;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn unique_directory(label: &str) -> PathBuf {
    let nanos = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos(),
        Err(time_error) => time_error.duration().as_nanos(),
    };
    std::env::temp_dir().join(format!("cinnabar_{}_{}_{}", label, std::process::id(), nanos))
}

fn run(compiler: &str, arguments: &[&str]) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(compiler).args(arguments).output()?)
}

fn text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn free_address() -> Result<String, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    drop(listener);
    Ok(address.to_string())
}

fn request(address: &str, message: &[u8]) -> Result<String, Box<dyn Error>> {
    let mut attempts = 0usize;
    loop {
        match TcpStream::connect(address) {
            Ok(mut stream) => {
                stream.set_read_timeout(Some(Duration::from_secs(20)))?;
                stream.write_all(message)?;
                stream.shutdown(Shutdown::Write)?;
                let mut response = String::new();
                stream.read_to_string(&mut response)?;
                return Ok(response);
            }
            Err(connect_error) => {
                attempts += 1;
                if attempts >= 600 {
                    return Err(format!("server at {} did not start: {}", address, connect_error).into());
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

fn stop_server(child: &mut Child) -> Result<(), Box<dyn Error>> {
    child.kill()?;
    let status = child.wait()?;
    if status.success() {
        return Err("server unexpectedly exited successfully after termination".into());
    }
    Ok(())
}

#[test]
fn tier_five_and_six_commands_work_end_to_end() -> Result<(), Box<dyn Error>> {
    let compiler = env!("CARGO_BIN_EXE_cinnabar");
    let directory = unique_directory("advanced_tooling");
    std::fs::create_dir_all(&directory)?;
    let source = directory.join("main.cnb");
    std::fs::write(&source, "fun main() I32\n  return 0\nend\n")?;
    // Generated names must be registered natives, so this exercises the
    // real `Memory` surface; the IDL's `type` generates `nat type` only.
    let idl = directory.join("memory.idl");
    let native = directory.join("Memory.cnb");
    std::fs::write(&idl, "module Memory\ntype Block\nfun allocate(size: Usize) Block impure\nfun deallocate(block: Block) Unit impure\nfun write_u8(block: &Block, offset: Usize, value: U8) Unit impure\nfun read_u8(block: &Block, offset: Usize) U8 impure\n")?;
    let generated = run(compiler, &["native-stub", &path_text(&idl), "-o", &path_text(&native)])?;
    assert!(generated.status.success(), "native-stub failed: {}", text(&generated));
    let checked_native = run(compiler, &[&path_text(&native), "--check-only"])?;
    assert!(checked_native.status.success(), "generated surface failed: {}", text(&checked_native));

    let targets = run(compiler, &["targets"])?;
    assert!(targets.status.success());
    assert!(text(&targets).contains("host\tavailable"));
    let unknown_target = run(compiler, &["build", &path_text(&directory), "--target", "aarch64"])?;
    assert!(!unknown_target.status.success());
    assert!(text(&unknown_target).contains("unknown target"));

    let report = directory.join("inspection.txt");
    let inspected = run(compiler, &["inspect", &path_text(&source), "-o", &path_text(&report)])?;
    assert!(inspected.status.success(), "inspection failed: {}", text(&inspected));
    let report_text = std::fs::read_to_string(&report)?;
    assert!(report_text.contains("TYPE LAYOUTS"));
    assert!(report_text.contains("SYMBOLS BY SIZE"));
    assert!(report_text.contains("DISASSEMBLY"));
    assert!(report_text.contains("Source correlation: unavailable"));

    let evidence_path = directory.join("soundness.json");
    let soundness = run(compiler, &["soundness", &path_text(&source), "-o", &path_text(&evidence_path)])?;
    assert!(soundness.status.success(), "soundness failed: {}", text(&soundness));
    let evidence: Value = serde_json::from_str(&std::fs::read_to_string(&evidence_path)?)?;
    assert_eq!(evidence.get("schema").and_then(Value::as_str), Some("cinnabar.soundness-evidence.v1"));
    assert_eq!(evidence.get("formal_proof").and_then(Value::as_bool), Some(false));
    assert_eq!(evidence.pointer("/front_end/borrow_checked").and_then(Value::as_bool), Some(true));
    assert!(evidence.pointer("/typed_arena/trait_dispatches").is_some());

    let lessons = directory.join("mushlings");
    let initialized = run(compiler, &["mushlings", "init", &path_text(&lessons)])?;
    assert!(initialized.status.success(), "Mushlings init failed: {}", text(&initialized));
    let verified = run(compiler, &["mushlings", "verify", &path_text(&lessons)])?;
    assert!(verified.status.success(), "Mushlings verify failed: {}", text(&verified));
    assert!(text(&verified).contains("0 solved, 9 pending"));

    let failure = directory.join("fuzz_fail_7.cnb");
    let minimized = directory.join("fuzz_fail_7.min.cnb");
    std::fs::write(&failure, "fun main() I64\n  return 1 / 0\nend\n")?;
    let reduced = run(compiler, &["fuzz", "minimize", &path_text(&failure), "-o", &path_text(&minimized)])?;
    assert!(reduced.status.success(), "fuzz minimization failed: {}", text(&reduced));
    let replayed = run(compiler, &["fuzz", "replay", &path_text(&minimized)])?;
    assert!(!replayed.status.success());
    assert!(text(&replayed).contains("deterministically reproduced"));

    std::fs::remove_dir_all(&directory)?;
    Ok(())
}

#[test]
fn attached_fact_and_formatter_cli_surfaces_work_end_to_end() -> Result<(), Box<dyn Error>> {
    let compiler = env!("CARGO_BIN_EXE_cinnabar");
    let directory = unique_directory("attached_fact_tools");
    std::fs::create_dir_all(&directory)?;
    let source = directory.join("main.cnb");
    std::fs::write(&source, "type Pair\n  left: I64\n  right: I64\nend\n\nfun main() I32\n  val pair = Pair(left: 1, right: 2)\n  if pair.left == 1\n  return 0\n  else\n  return 1\n  end\nend\n")?;

    let format_check_before = run(compiler, &["fmt", "--check", &path_text(&source)])?;
    assert!(!format_check_before.status.success());
    let formatted = run(compiler, &["fmt", &path_text(&source)])?;
    assert!(formatted.status.success(), "format failed: {}", text(&formatted));
    let format_check_after = run(compiler, &["fmt", "--check", &path_text(&source)])?;
    assert!(format_check_after.status.success(), "formatted source was not canonical: {}", text(&format_check_after));

    let typed = run(compiler, &[&path_text(&source), "--dump-typed-ast"])?;
    assert!(typed.status.success(), "typed AST failed: {}", text(&typed));
    assert!(text(&typed).contains("expr "));
    assert!(text(&typed).contains(" ty="));
    let layouts = run(compiler, &[&path_text(&source), "--print-layout"])?;
    assert!(layouts.status.success(), "layout report failed: {}", text(&layouts));
    assert!(text(&layouts).contains("Pair"));

    let llvm = directory.join("main.ll");
    let object = directory.join("main.o");
    let emitted_llvm = run(compiler, &[&path_text(&source), "--emit-llvm", "-o", &path_text(&llvm)])?;
    assert!(emitted_llvm.status.success(), "LLVM emission failed: {}", text(&emitted_llvm));
    assert!(std::fs::read_to_string(&llvm)?.contains("define"));
    let emitted_object = run(compiler, &[&path_text(&source), "--emit-obj", "-o", &path_text(&object)])?;
    assert!(emitted_object.status.success(), "object emission failed: {}", text(&emitted_object));
    assert!(std::fs::metadata(&object)?.len() > 0);

    std::fs::remove_dir_all(&directory)?;
    Ok(())
}

#[test]
fn documentation_and_playground_servers_work_over_http() -> Result<(), Box<dyn Error>> {
    let compiler = env!("CARGO_BIN_EXE_cinnabar");
    let directory = unique_directory("http_tools");
    std::fs::create_dir_all(&directory)?;
    let fixture_path = directory.join("main.cnb");
    std::fs::write(&fixture_path, "#! Playground documentation\nfun main() I32\n  return 0\nend\n")?;
    let fixture = path_text(&fixture_path);

    let burn_address = free_address()?;
    let mut burn = Command::new(compiler)
        .args(["burn", &fixture, "--address", &burn_address])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let burn_result = request(&burn_address, b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stop_server(&mut burn)?;
    let burn_response = burn_result?;
    assert!(burn_response.contains("200 OK"));
    assert!(burn_response.contains("Cinnabook"));
    assert!(burn_response.contains(env!("CARGO_PKG_VERSION")));

    let playground_address = free_address()?;
    let mut playground = Command::new(compiler)
        .args(["playground", "--address", &playground_address])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let page_result = request(&playground_address, b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let program = b"fun main() I32\n  return 7\nend\n";
    let headers = format!("POST /run HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", program.len());
    let mut message = headers.into_bytes();
    message.extend_from_slice(program);
    let run_result = request(&playground_address, &message);
    let looping_program = b"fun main() I32\n  while true\n    continue\n  end\n  return 0\nend\n";
    let looping_headers = format!("POST /run HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", looping_program.len());
    let mut looping_message = looping_headers.into_bytes();
    looping_message.extend_from_slice(looping_program);
    let timeout_result = request(&playground_address, &looping_message);
    let oversized_result = request(
        &playground_address,
        b"POST /run HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1048577\r\nConnection: close\r\n\r\n",
    );
    let health_result = request(&playground_address, b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stop_server(&mut playground)?;
    let page = page_result?;
    let run_response = run_result?;
    let timeout_response = timeout_result?;
    let oversized_response = oversized_result?;
    let health_response = health_result?;
    assert!(page.contains("Compile and run"));
    assert!(run_response.contains("Program exit status"));
    assert!(run_response.contains("7"));
    assert!(timeout_response.contains("exceeded the 5 second execution limit"));
    assert!(oversized_response.contains("400 Bad Request"));
    assert!(oversized_response.contains("exceeds one MiB"));
    assert!(health_response.contains("200 OK"));
    assert!(health_response.contains("Compile and run"));
    std::fs::remove_dir_all(&directory)?;
    Ok(())
}

#[test]
fn vscode_package_references_valid_language_assets() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("editors").join("vscode");
    let package: Value = serde_json::from_str(&std::fs::read_to_string(root.join("package.json"))?)?;
    let configuration: Value = serde_json::from_str(&std::fs::read_to_string(root.join("language-configuration.json"))?)?;
    let grammar: Value = serde_json::from_str(&std::fs::read_to_string(root.join("syntaxes").join("cinnabar.tmLanguage.json"))?)?;
    assert_eq!(package.pointer("/contributes/languages/0/extensions/0").and_then(Value::as_str), Some(".cnb"));
    assert_eq!(configuration.pointer("/comments/lineComment").and_then(Value::as_str), Some("#"));
    assert_eq!(configuration.pointer("/comments/blockComment/0").and_then(Value::as_str), Some("#|"));
    assert_eq!(grammar.get("scopeName").and_then(Value::as_str), Some("source.cinnabar"));
    let grammar_text = std::fs::read_to_string(root.join("syntaxes").join("cinnabar.tmLanguage.json"))?;
    assert!(grammar_text.contains("comment.block.documentation.cinnabar"));
    assert!(grammar_text.contains("comment.line.documentation.cinnabar"));
    Ok(())
}
