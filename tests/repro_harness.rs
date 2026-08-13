//! The fixture corpus, run for exit codes and for stream contents.
//!
//! Compiles every case in `repro_corpus` and checks the exit code it
//! produces, then separately runs the `STREAM_CASES` to check that terminal
//! output and command-line arguments carry their contents byte for byte.
//! Children run under a timeout with stdin closed, and both output streams
//! are drained on their own threads so a fixture that writes more than a
//! pipe buffer holds cannot deadlock the harness.
//!
//! Corpus size is chosen by the test profile rather than by editing the
//! table, so a reduced run covers the same corpus more thinly instead of
//! covering a different, smaller one.
//!
//! **Invariants:**
//! - The corpus lives in `tests/support/repro_corpus.rs` and is shared with
//!   `sanitizer_gate`. A second copy would drift, and the two suites would
//!   quietly stop covering the same programs.
//! - A fixture never reads the harness's own standard input.
//! - A reduced profile samples evenly across the corpus, never a prefix.

#[path = "support/test_controls.rs"]
mod test_controls;
#[path = "support/repro_corpus.rs"]
mod repro_corpus;
#[path = "support/stream_cases.rs"]
mod stream_cases;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use repro_corpus::EXPECT_OK;
use stream_cases::{StreamCase, STREAM_CASES};
use test_controls::{
    evenly_selected, profile_name, profile_usize, reduced_usize_control, test_profile,
};


const EXPECT_REJECTED: &[&str] = &[
    "index_oob_const",
    "rt2",
    "div_zero_const",
    "mod_zero_const",
    "assign_shared_ref",
    "linear_field_reassign",
    "linear_field_dup",
    "linear_struct_dead_end",
    "linear_field_dup_extract",
    "linear_ref_no_restore",
    "linear_ref_no_restore_falloff",
    "linear_ref_untracked",
    "ret_borrow_ambiguous",
    "ret_borrow_sole_input",
    "ret_borrow_uaf",
    "duplicate_builtin_unit",
    "duplicate_builtin_int",
    "duplicate_user_symbol",
    "idx10b_mut_alias_used",
    "idx10c_mut_shared_same",
    "vec_push_linear_move",
    "idx10j2_dyn_dyn_match",
    "idx10f_element_move_while_borrowed",
    "idx10g_element_double_move",
    "b3_two_mut",
    "b4_mut_shared",
    "int_literal_range",
    "int_literal_no_peer",
    "string_bad_escape",
    "string_not_an_int",
    "file_unclosed",
    "borrow_after_move",
    "int_unsigned_neg",
    "non_tail_recursion",
    "vec_no_extraction",
    "vec_undrained_free",
    "vec_pop_unconsumed",
    "hash_map_undrained_free",
    "hash_map_linear_key_undrained_free",
    "unresolved_call_cascade",
    "non_struct_field_cascade",
    "undeclared_const_cascade",
    "const_div_zero_cascade",
    "malformed_type_cascade",
];

const RECORD_ONLY: &[&str] = &[
    "full_rt",
    "mem_test",
    "rt1",
    "vec_test",
    "vm5",
    "vm10",
];

// Compile-only fixtures: the binary must build, but is never executed.
// http_server.cnb is a blocking network server loop, so running it would
// hang the harness; compiling it proves the Net native surface lowers and
// links (per the zero-execution rule for that fixture).
const EXPECT_COMPILE: &[(&str, &str)] = &[("http_server", "tests/fixtures/http_server.cnb")];

fn fixture_path(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    root.join("tests")
        .join("fixtures")
        .join("repro")
        .join(format!("{}.cnb", name))
}

fn fixture_rel_path(rel: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    root.join(rel)
}

fn exit_code(cmd: &mut Command) -> i32 {
    match cmd.status() {
        Ok(status) => match status.code() {
            Some(code) => code,
            None => 139,
        },
        Err(err) => {
            eprintln!("spawn failed: {}", err);
            139
        }
    }
}

fn compile_and_link(cinnabar: &str, fixture: &Path, bin: &Path) -> i32 {
    exit_code(Command::new(cinnabar).arg(fixture).arg("-o").arg(bin))
}

fn compile_to_llvm(cinnabar: &str, fixture: &Path, ir: &Path) -> i32 {
    exit_code(
        Command::new(cinnabar)
            .arg(fixture)
            .arg("--emit-llvm")
            .arg("-o")
            .arg(ir),
    )
}

const DEFAULT_RUN_TIMEOUT_SECS: usize = 10;
const BALANCED_RUN_CASES: usize = 10;
const BALANCED_RECORD_CASES: usize = 2;
const SMOKE_RUN_CASES: usize = 4;
const SMOKE_RECORD_CASES: usize = 0;

const TIMEOUT_CODE: i32 = 124;

struct ReproConfig {
    profile: test_controls::TestProfile,
    run_cases: usize,
    record_cases: usize,
    link_compile_only: bool,
    run_timeout_secs: u64,
}

fn bool_control(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => match value.as_str() {
            "1" | "true" | "yes" => true,
            "0" | "false" | "no" => false,
            invalid => {
                assert!(
                    false,
                    "{} must be one of 1, true, yes, 0, false, or no; got '{}'",
                    name,
                    invalid
                );
                default
            }
        },
        Err(error) => match error {
            std::env::VarError::NotPresent => default,
            std::env::VarError::NotUnicode(value) => {
                assert!(false, "{} is not Unicode: {:?}", name, value);
                default
            }
        },
    }
}

fn repro_config() -> ReproConfig {
    let profile = test_profile();
    let run_default = profile_usize(
        profile,
        EXPECT_OK.len(),
        BALANCED_RUN_CASES,
        SMOKE_RUN_CASES,
    );
    let record_default = profile_usize(
        profile,
        RECORD_ONLY.len(),
        BALANCED_RECORD_CASES,
        SMOKE_RECORD_CASES,
    );
    let run_cases =
        reduced_usize_control(profile, "CINNABAR_REPRO_RUN_CASES", run_default);
    let record_cases =
        reduced_usize_control(profile, "CINNABAR_REPRO_RECORD_CASES", record_default);
    assert!(
        run_cases <= EXPECT_OK.len(),
        "CINNABAR_REPRO_RUN_CASES ({}) cannot exceed the {} expected-success fixtures",
        run_cases,
        EXPECT_OK.len()
    );
    assert!(
        record_cases <= RECORD_ONLY.len(),
        "CINNABAR_REPRO_RECORD_CASES ({}) cannot exceed the {} record-only fixtures",
        record_cases,
        RECORD_ONLY.len()
    );
    let link_default = match profile {
        test_controls::TestProfile::Full => true,
        test_controls::TestProfile::Balanced => false,
        test_controls::TestProfile::Smoke => false,
    };
    let link_compile_only = match profile {
        test_controls::TestProfile::Full => link_default,
        test_controls::TestProfile::Balanced => {
            bool_control("CINNABAR_REPRO_LINK_COMPILE_ONLY", link_default)
        }
        test_controls::TestProfile::Smoke => {
            bool_control("CINNABAR_REPRO_LINK_COMPILE_ONLY", link_default)
        }
    };
    let run_timeout = reduced_usize_control(
        profile,
        "CINNABAR_TEST_RUN_TIMEOUT_SECS",
        DEFAULT_RUN_TIMEOUT_SECS,
    );
    assert!(run_timeout > 0, "CINNABAR_TEST_RUN_TIMEOUT_SECS must be greater than zero");
    ReproConfig {
        profile,
        run_cases,
        record_cases,
        link_compile_only,
        run_timeout_secs: run_timeout as u64,
    }
}

fn run_binary(bin: &Path, timeout_secs: u64) -> i32 {
    let child = match Command::new(bin)
        // A fixture must never read the harness's own standard input.
        // `Terminal.read_line` blocks until a line or end of input arrives,
        // so an inherited descriptor would make a fixture's exit code
        // depend on whether the suite was run from a terminal, a pipe, or
        // CI. A null stdin is at end of input immediately, which is a
        // definite state every run agrees on.
        //
        // A fixture that means to be read rather than merely counted names
        // its input in `STREAM_CASES` and runs under `run_with_streams`,
        // which supplies exactly those bytes — still a definite state, just
        // a richer one than "nothing".
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            eprintln!("spawn failed: {}", err);
            return 139;
        }
    };
    wait_with_timeout(child, timeout_secs)
}

fn wait_with_timeout(mut child: std::process::Child, timeout_secs: u64) -> i32 {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(code) => code,
                    None => 139,
                };
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    match child.kill() {
                        Ok(()) => {}
                        Err(err) => {
                            if err.kind() != std::io::ErrorKind::InvalidInput {
                                eprintln!("kill after deadline failed: {}", err);
                            }
                        }
                    }
                    match child.wait() {
                        Ok(status) => {
                            return match status.code() {
                                Some(code) => code,
                                None => TIMEOUT_CODE,
                            };
                        }
                        Err(err) => {
                            eprintln!("reap failed: {}", err);
                            return TIMEOUT_CODE;
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(err) => {
                eprintln!("wait failed: {}", err);
                return 139;
            }
        }
    }
}

// One directory per test: the tests in this file run concurrently, so a
// shared per-process directory would let the first one to finish delete the
// binaries the others are still running.
fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cinnabar_repro_{}_{}", std::process::id(), label))
}

// Removes the harness temp dir even when an assertion fails mid-run: a
// failed iteration must not leak its compiled binaries (each a ~4.5 MB
// embedded-libc.a link) into the temp filesystem.
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(err) => eprintln!("repro temp cleanup failed: {}", err),
        }
    }
}

/// Drains one of a child's output streams to end of file, on its own
/// thread.
///
/// A program that fills a pipe buffer blocks until someone reads it, so
/// draining the two streams in sequence would deadlock against a fixture
/// that writes enough to both. The thread is started before anything is
/// written to the child's standard input for the same reason.
fn drain<R: Read + Send + 'static>(
    pipe: Option<R>,
    stream: &'static str,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        match pipe {
            Some(mut source) => match source.read_to_end(&mut buffer) {
                Ok(count) => assert_eq!(count, buffer.len(), "short read from {}", stream),
                Err(err) => assert!(false, "cannot read {}: {}", stream, err),
            },
            None => assert!(false, "{} was not piped", stream),
        }
        buffer
    })
}

fn collect(handle: std::thread::JoinHandle<Vec<u8>>, stream: &str) -> Vec<u8> {
    match handle.join() {
        Ok(bytes) => bytes,
        Err(failure) => {
            let detail = match failure.downcast_ref::<String>() {
                Some(text) => text.clone(),
                None => "no message".to_string(),
            };
            assert!(false, "the {} reader failed: {}", stream, detail);
            Vec::new()
        }
    }
}

struct StreamOutcome {
    exit: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_with_streams(bin: &Path, case: &StreamCase, timeout_secs: u64) -> StreamOutcome {
    let mut child = match Command::new(bin)
        .args(case.args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            eprintln!("spawn failed: {}", err);
            return StreamOutcome { exit: 139, stdout: Vec::new(), stderr: Vec::new() };
        }
    };
    let out_reader = drain(child.stdout.take(), "standard output");
    let err_reader = drain(child.stderr.take(), "standard error");
    // The input is written and its descriptor closed before the wait.
    // `read_line` blocks until a line or end of input arrives, so a standard
    // input left open would hang the fixture that reads past its last line —
    // the exact fixture this table exists to run.
    match child.stdin.take() {
        Some(mut pipe) => match pipe.write_all(case.stdin) {
            Ok(()) => {}
            Err(err) => assert!(false, "{}: cannot write standard input: {}", case.name, err),
        },
        None => assert!(false, "{}: standard input was not piped", case.name),
    }
    let exit = wait_with_timeout(child, timeout_secs);
    StreamOutcome {
        exit,
        stdout: collect(out_reader, "standard output"),
        stderr: collect(err_reader, "standard error"),
    }
}

/// Renders a stream for a failure message, so a mismatch reads as text
/// rather than as a byte array. Escaped rather than lossy: a dropped or
/// added terminator is the defect being looked for, and it has to be
/// visible in the message.
fn shown(bytes: &[u8]) -> String {
    let mut text = String::new();
    for byte in bytes {
        match byte {
            b'\n' => text.push_str("\\n"),
            b'\t' => text.push_str("\\t"),
            printable if printable.is_ascii_graphic() || *printable == b' ' => {
                text.push(*printable as char)
            }
            other => text.push_str(&format!("\\x{:02x}", other)),
        }
    }
    text
}

fn assert_stream(case: &StreamCase, stream: &str, want: &[u8], got: &[u8]) {
    assert!(
        want == got,
        "{}: {} was \"{}\", want \"{}\"",
        case.name,
        stream,
        shown(got),
        shown(want)
    );
}

/// What each fixture writes, to which descriptor, and what it makes of what
/// it was given — none of which the exit-code corpus can see.
#[test]
fn terminal_streams_and_arguments_carry_their_contents() {
    let cinnabar = env!("CARGO_BIN_EXE_cinnabar");
    let config = repro_config();
    let dir = temp_dir("streams");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            // Not a silent return: this test carries the only assertions of
            // terminal byte behaviour, and passing without making them would
            // report coverage that did not run.
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());

    let mut idx = 0usize;
    while idx < STREAM_CASES.len() {
        let case = match STREAM_CASES.get(idx) {
            Some(case) => case,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", case.name));
        let compile_code = compile_and_link(cinnabar, &fixture_path(case.name), &bin);
        assert_eq!(compile_code, 0, "{} failed to compile (code {})", case.name, compile_code);
        let outcome = run_with_streams(&bin, case, config.run_timeout_secs);
        assert_stream(case, "standard output", case.stdout, &outcome.stdout);
        assert_stream(case, "standard error", case.stderr, &outcome.stderr);
        assert_eq!(
            outcome.exit, case.exit,
            "{} ran with exit {} (want {})",
            case.name, outcome.exit, case.exit
        );
        idx += 1;
    }

    drop(guard);
}

#[test]
fn repro_corpus_baseline() {
    let cinnabar = env!("CARGO_BIN_EXE_cinnabar");
    let config = repro_config();
    eprintln!(
        "repro profile: {} (link+run expected-success={}, LLVM-only expected-success={}, record-only={}, link compile-only={})",
        profile_name(config.profile),
        config.run_cases,
        EXPECT_OK.len() - config.run_cases,
        config.record_cases,
        config.link_compile_only
    );
    let dir = temp_dir("baseline");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());

    let mut idx = 0usize;
    while idx < EXPECT_OK.len() {
        let (name, want) = match EXPECT_OK.get(idx) {
            Some(pair) => *pair,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", name));
        let ir = dir.join(format!("{}.ll", name));
        let execute = evenly_selected(idx, EXPECT_OK.len(), config.run_cases);
        let compile_code = if execute {
            compile_and_link(cinnabar, &fixture_path(name), &bin)
        } else {
            compile_to_llvm(cinnabar, &fixture_path(name), &ir)
        };
        assert_eq!(compile_code, 0, "{} failed to compile (code {})", name, compile_code);
        if execute {
            let run_code = run_binary(&bin, config.run_timeout_secs);
            assert_eq!(run_code, want, "{} ran with exit {} (want {})", name, run_code, want);
        }
        idx += 1;
    }

    let mut ridx = 0usize;
    while ridx < EXPECT_REJECTED.len() {
        let name = match EXPECT_REJECTED.get(ridx) {
            Some(name) => *name,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", name));
        let compile_code = compile_and_link(cinnabar, &fixture_path(name), &bin);
        assert!(compile_code != 0, "{} was unexpectedly accepted", name);
        ridx += 1;
    }

    let mut oidx = 0usize;
    while oidx < RECORD_ONLY.len() {
        let name = match RECORD_ONLY.get(oidx) {
            Some(name) => *name,
            None => break,
        };
        if evenly_selected(oidx, RECORD_ONLY.len(), config.record_cases) {
            let bin = dir.join(format!("{}_bin", name));
            let compile_code = compile_and_link(cinnabar, &fixture_path(name), &bin);
            if compile_code == 0 {
                let run_code = run_binary(&bin, config.run_timeout_secs);
                println!("RECORD {}: compile=OK run={}", name, run_code);
            } else {
                println!("RECORD {}: compile=FAIL({})", name, compile_code);
            }
        }
        oidx += 1;
    }

    let mut cidx = 0usize;
    while cidx < EXPECT_COMPILE.len() {
        let (name, rel) = match EXPECT_COMPILE.get(cidx) {
            Some(pair) => *pair,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", name));
        let ir = dir.join(format!("{}.ll", name));
        let compile_code = if config.link_compile_only {
            compile_and_link(cinnabar, &fixture_rel_path(rel), &bin)
        } else {
            compile_to_llvm(cinnabar, &fixture_rel_path(rel), &ir)
        };
        assert_eq!(compile_code, 0, "{} failed to compile (code {})", name, compile_code);
        cidx += 1;
    }

    drop(guard);
}
