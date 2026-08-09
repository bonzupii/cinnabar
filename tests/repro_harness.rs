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
//! Every compiled repro is killed after `RUN_TIMEOUT_SECS` if it has not
//! exited, so a legitimately non-terminating program (an infinite VM
//! loop) costs the suite one bounded wait instead of hanging it.
//!
//! As phases land, entries move from `RECORD_ONLY` into `EXPECT_OK` (or,
//! for error-quality work, `EXPECT_REJECTED` gains message assertions).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Programs that must compile and exit with the given code.
const EXPECT_OK: &[(&str, i32)] = &[
    ("hello", 0),
    ("mini", 0),
    ("array_test", 0), // const in-range index arr[0]: proven at compile time, no Result wrapper
    ("borrow_index", 0), // &arr[i] and &mut arr[i] borrow elements; dynamic &rest[i] returns Result(&Pair, IndexError) with the borrowed element in the Ok payload
    ("rec_test", 120),
    ("tail_rec", 64), // 1M self-tail-recursive iterations: LLVM tail-call elimination keeps it O(1)-stack
    ("mem_probe", 70), // non-tail recursion depth 500000 (opaque heap read per level keeps the frames real) trips the stack guard, which exits 70 instead of SIGSEGV
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
    ("vm", 120), // factorial VM with backward jumps; PROG bytes reconciled to the documented 5! program
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
    ("linear_field_reinit", 0), // linear field sub-node cycle on a local: consume -> reinit -> consume -> exit
    ("linear_ref_swap", 0), // design-doc cycle through &mut: deallocate(ref.block) then ref.block = replacement
    ("linear_field_consume", 0), // bug-report 2a fixed shape: linear-in-struct is consumable end-to-end
    ("linear_ref_nonlinear_read", 14), // non-linear field reads through &mut are plain reads (no false positive)
    ("ret_borrow_shared_twice", 0), // bug-report d5 control: two shared borrows of one input coexist; free after the last use
    ("ret_borrow_single_origin", 0), // bug-report d7d: callee-origin summary — view_of_a's returned borrow traces to its first input only, so freeing the never-borrowed second input while the view is live compiles
];

/// Programs the compiler must reject.
const EXPECT_REJECTED: &[&str] = &[
    "index_oob_const",     // constant index out of bounds (5 >= 3): compile-time error
    "rt2",                 // linear value not consumed (correct rejection)
    "slice_test",          // U8 + Usize arithmetic (strict same-type rule)
    "div_zero_const",      // division by constant zero is a compile-time error
    "mod_zero_const",      // modulo by constant zero is a compile-time error
    "assign_shared_ref",   // field write through a shared &T reference: assignment requires &mut
    "linear_field_reassign", // reassigning a Live linear field without consuming it is a compile error
    "linear_field_dup",    // copying a linear field out of a shared &T reference duplicates the handle
    "linear_struct_dead_end", // bug-report 2a dead end: a moved-out linear handle left unconsumed is rejected
    "linear_field_dup_extract", // bug-report 2b: extracting the same linear field twice is a use of moved value
    "linear_ref_no_restore", // UAF shape: consuming through a &mut param without restoring is rejected
    "linear_ref_no_restore_falloff", // same, on the reachable fall-off-end exit (no return statement)
    "linear_ref_untracked", // consuming through a &mut to an untracked temporary is rejected
    "ret_borrow_ambiguous", // bug-report d6: returned borrow derives from two different input params on different paths -> ambiguity error
    "ret_borrow_sole_input", // bug-report d4 control: freeing the sole input while its returned borrow is live is rejected
    "ret_borrow_uaf", // bug-report d7b: ambiguous ref-returning callee (and freeing a possibly-aliased input) is rejected
    "duplicate_builtin_unit", // bug-report d3: declaring Unit is a builtin redeclaration, not a bare duplicate symbol
    "duplicate_builtin_int", // declaring Int must not silently replace the builtin scalar
    "duplicate_user_symbol", // control: user-user duplicates still report duplicate symbol
];

/// Known-broken programs; observed status printed only.  Each entry names
/// its fix target.
const RECORD_ONLY: &[&str] = &[
    "full_rt",   // Vec native by-value handle double-deref
    "mem_test",  // write_u8 Err + deallocate UB
    "rt1",       // vec_free NULL-deref
    "vec_test",  // Vec native
    "vm5",       // infinite interpreter loop (backward jumps); runs forever without crashing, killed by the 10s harness timeout
    "vm10",      // infinite interpreter loop (backward jumps); runs forever without crashing, killed by the 10s harness timeout
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

/// Seconds a compiled repro may run before the harness kills it.  A
/// correct program may legitimately loop forever (the VM interpreters
/// in RECORD_ONLY), so every execution is bounded: an infinite loop
/// must cost the harness its timeout, never hang the test suite.
const RUN_TIMEOUT_SECS: u64 = 10;

/// Code reported when a repro is killed for exceeding the time limit;
/// mirrors GNU coreutils `timeout` (124), so a hang reads distinctly
/// from a real exit code or a signal crash.
const TIMEOUT_CODE: i32 = 124;

/// Runs a compiled binary with stdio nulled, killing it after
/// `RUN_TIMEOUT_SECS` if it has not exited; returns its exit code, or
/// `TIMEOUT_CODE` when the time limit was hit.
fn run_binary(bin: &Path) -> i32 {
    let mut child = match Command::new(bin)
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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(RUN_TIMEOUT_SECS);
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
                    // Deadline hit: terminate the repro and reap it.  A
                    // signal death (the kill landed) reports the
                    // timeout; a child that raced past the deadline and
                    // exited on its own just before the kill reports its
                    // real code.
                    match child.kill() {
                        Ok(()) => {}
                        Err(err) => {
                            // InvalidInput means the child had already
                            // exited on its own just as the deadline
                            // hit — the expected race, not a failure;
                            // the real code is reported below.
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
