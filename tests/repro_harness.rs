//! Repro-corpus harness.
//!
//! Locks the Phase-0 triage baseline.  Every program in
//! `tests/fixtures/repro/` is classified as one of:
//!
//! - `EXPECT_OK(n)`: must compile, and the binary must exit with code n;
//! - `EXPECT_REJECTED`: must be rejected by the compiler;
//! - `RECORD_ONLY`: known-broken until its fix phase lands; the observed
//!   status is printed but not asserted.
//!
//! Compilation goes through the real CLI (Cargo builds the binary for
//! integration tests, exposed as `CARGO_BIN_EXE_cinnabar`), so the full
//! pipeline including llc/clang is exercised.  Binaries are written to a
//! unique temp directory; the repo is never littered.
//!
//! As phases land, entries move from `RECORD_ONLY` into `EXPECT_OK` (or,
//! for error-quality work, `EXPECT_REJECTED` gains message assertions).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Programs that must compile and exit with the given code.
const EXPECT_OK: &[(&str, i32)] = &[
    ("hello", 0),
    ("mini", 0),
    ("rec_test", 120),
    ("hanoi", 255),
    ("head", 10),
    ("enum_test", 0),
    ("mem2", 0),
    ("vm2", 1),
    ("vm3", 1),
    ("vm4", 1),
    ("vm6", 1),
    ("vm7", 5),
    ("vm8", 1),
    ("vm9", 5),
    ("vm11", 4),
    ("continue_test", 9),
    ("jump_test", 3),
    ("jump2", 3),
    ("jump3", 3),
    ("jump4", 1),
    ("nested_continue_test", 109),
    ("elif_test", 1), // elif keyword (was: rejected, missing feature)
    ("elif_chain", 3), // elif chain with else
    ("modulo", 42), // % returns Result(Int, DivError), non-zero divisor
    ("div_runtime", 7), // runtime zero divisor -> Err(DivByZero), no trap
    ("multiline_const", 30), // const initializer spanning lines in parens
    ("fib", 155), // while loops without a user-declared Unit (builtin Unit)
];

/// Programs the compiler must reject.
const EXPECT_REJECTED: &[&str] = &[
    "array_test",     // no array indexing (deliberate omission)
    "rt2",            // linear value not consumed (correct rejection)
    "slice_test",     // U8 + Usize arithmetic (strict same-type rule)
    "div_zero_const", // division by constant zero is a compile-time error
    "mod_zero_const", // modulo by constant zero is a compile-time error
];

/// Known-broken programs; observed status printed only.  Each entry names
/// its fix target.
const RECORD_ONLY: &[&str] = &[
    "full_rt",   // Vec native by-value handle double-deref
    "mem_probe", // unbounded recursion (TCO / guard)
    "mem_test",  // write_u8 Err + deallocate UB
    "rt1",       // vec_free NULL-deref
    "vec_test",  // Vec native
    "vm",        // backward-jump loop crash
    "vm5",       // backward-jump loop crash
    "vm10",      // backward-jump loop crash
];

fn fixture_path(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    root.join("tests")
        .join("fixtures")
        .join("repro")
        .join(format!("{}.cnb", name))
}

/// Runs `cmd`, returning the process exit code (139 for a signal crash or
/// a spawn failure).
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

/// Compiles the fixture to `bin`; returns the compiler's exit code.
fn compile(cinnabar: &str, fixture: &Path, bin: &Path) -> i32 {
    exit_code(Command::new(cinnabar).arg(fixture).arg("-o").arg(bin))
}

/// Runs a compiled binary with stdio nulled; returns its exit code.
fn run_binary(bin: &Path) -> i32 {
    let mut cmd = Command::new(bin);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    exit_code(&mut cmd)
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("cinnabar_repro_{}", std::process::id()))
}

#[test]
fn repro_corpus_baseline() {
    let cinnabar = env!("CARGO_BIN_EXE_cinnabar");
    let dir = temp_dir();
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("cannot create temp dir: {}", err);
            return;
        }
    }

    let mut idx = 0usize;
    while idx < EXPECT_OK.len() {
        let (name, want) = match EXPECT_OK.get(idx) {
            Some(pair) => *pair,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", name));
        let compile_code = compile(cinnabar, &fixture_path(name), &bin);
        assert_eq!(compile_code, 0, "{} failed to compile (code {})", name, compile_code);
        let run_code = run_binary(&bin);
        assert_eq!(run_code, want, "{} ran with exit {} (want {})", name, run_code, want);
        idx += 1;
    }

    let mut ridx = 0usize;
    while ridx < EXPECT_REJECTED.len() {
        let name = match EXPECT_REJECTED.get(ridx) {
            Some(name) => *name,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", name));
        let compile_code = compile(cinnabar, &fixture_path(name), &bin);
        assert!(compile_code != 0, "{} was unexpectedly accepted", name);
        ridx += 1;
    }

    let mut oidx = 0usize;
    while oidx < RECORD_ONLY.len() {
        let name = match RECORD_ONLY.get(oidx) {
            Some(name) => *name,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", name));
        let compile_code = compile(cinnabar, &fixture_path(name), &bin);
        if compile_code == 0 {
            let run_code = run_binary(&bin);
            println!("RECORD {}: compile=OK run={}", name, run_code);
        } else {
            println!("RECORD {}: compile=FAIL({})", name, compile_code);
        }
        oidx += 1;
    }

    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => eprintln!("temp cleanup failed: {}", err),
    }
}
