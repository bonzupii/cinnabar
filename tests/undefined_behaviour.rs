//! Undefined-behaviour classes, asserted against the emitted IR.
//!
//! UBSan cannot run here (its checks are emitted by Clang's front end while
//! lowering C), and most classes it covers are designed out of the language:
//! arithmetic wraps per width, shift counts mask by `width - 1`, `/` and `%`
//! return `Result`, constant out-of-range indexing is a compile error,
//! `Memory.read_u8`/`write_u8` bounds-check, and user code has no pointers
//! or dereference operator. What is asserted instead is the static property
//! that the compiler emits no IR whose behaviour is undefined:
//!
//! - **No `nsw`/`nuw` on `add`, `sub`, `mul`, or `shl`** — the language
//!   guarantees wrapping. (`getelementptr inbounds nuw` is a struct field
//!   offset and is checked separately.)
//! - **No `exact` on division or shifts.**
//! - **No `poison` values.**
//! - **`unreachable` only in a control-flow join every path diverges out of**
//!   (e.g. the exhaustive-match join), never as an assumed "cannot happen".

#[path = "support/test_controls.rs"]
mod test_controls;
#[path = "support/repro_corpus.rs"]
mod repro_corpus;

use repro_corpus::EXPECT_OK;
use std::path::{Path, PathBuf};
use std::process::Command;
use test_controls::{evenly_selected, profile_name, profile_usize, reduced_usize_control, test_profile};

const BALANCED_CASES: usize = 20;
const SMOKE_CASES: usize = 6;

/// The only block names an `unreachable` may appear in: control-flow joins
/// every path into diverged (`match_merge`, `if_merge`).
const DIVERGING_JOINS: [&str; 2] = ["match_merge", "if_merge"];

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("repro")
        .join(format!("{}.cnb", name))
}

// Per-test directory: concurrent tests must not share IR output.
fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cinnabar_ub_{}_{}", std::process::id(), label))
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(err) => eprintln!("undefined_behaviour temp cleanup failed: {}", err),
        }
    }
}

fn emit_ir(dir: &Path, name: &str) -> String {
    let out = dir.join(format!("{}.ll", name));
    let status = Command::new(env!("CARGO_BIN_EXE_cinnabar"))
        .arg(fixture_path(name))
        .arg("--emit-llvm")
        .arg("-o")
        .arg(&out)
        .status();
    match status {
        Ok(code) => assert!(code.success(), "{} failed to emit IR ({})", name, code),
        Err(err) => assert!(false, "{} could not be compiled: {}", name, err),
    }
    match std::fs::read_to_string(&out) {
        Ok(text) => text,
        Err(err) => {
            assert!(false, "cannot read emitted IR {}: {}", out.display(), err);
            String::new()
        }
    }
}

/// The instructions whose overflow behaviour the language pins.
const ARITHMETIC_OPCODES: [&str; 4] = ["add", "sub", "mul", "shl"];

/// Whether `line` defines an instruction of one of those opcodes.
fn arithmetic_opcode(line: &str) -> Option<&'static str> {
    let value = match line.split_once(" = ") {
        Some(pair) => pair.1.trim(),
        None => return None,
    };
    // Returns the matched opcode itself; no remapping needed.
    for opcode in ARITHMETIC_OPCODES {
        if value.starts_with(opcode) {
            return Some(opcode);
        }
    }
    None
}

fn assert_no_undefined_behaviour(name: &str, ir: &str) {
    let mut block = String::new();
    for line in ir.lines() {
        let trimmed = line.trim();

        // Track the enclosing basic block.
        let head = match trimmed.split_whitespace().next() {
            Some(token) => token,
            None => continue,
        };
        if head.ends_with(':') && !head.starts_with('%') {
            block = head.trim_end_matches(':').to_string();
            continue;
        }

        if let Some(opcode) = arithmetic_opcode(trimmed) {
            assert!(
                !trimmed.contains(" nsw ") && !trimmed.contains(" nuw "),
                "{}: `{}` carries an overflow flag, which makes overflow undefined — \
                 the language guarantees arithmetic wraps per width:\n  {}",
                name,
                opcode,
                trimmed
            );
        }
        assert!(
            !trimmed.contains(" exact "),
            "{}: an `exact` flag makes a non-exact result undefined:\n  {}",
            name,
            trimmed
        );
        assert!(
            !trimmed.contains("poison"),
            "{}: a poison value reaches the emitted IR:\n  {}",
            name,
            trimmed
        );
        if trimmed == "unreachable" {
            assert!(
                DIVERGING_JOINS.iter().any(|join| block.starts_with(join)),
                "{}: `unreachable` in block `{}`. The only ones the language \
                 justifies are control-flow joins that every path diverges \
                 out of — {:?}. Anywhere else it is a `cannot happen` with \
                 nothing proving it.",
                name,
                block,
                DIVERGING_JOINS
            );
        }
    }
}

#[test]
fn the_compiler_emits_no_undefined_behaviour() {
    let profile = test_profile();
    let budget = reduced_usize_control(
        profile,
        "CINNABAR_UB_CASES",
        profile_usize(profile, EXPECT_OK.len(), BALANCED_CASES, SMOKE_CASES),
    );
    eprintln!(
        "undefined-behaviour profile: {} ({} of {} fixtures)",
        profile_name(profile),
        budget,
        EXPECT_OK.len()
    );

    let dir = temp_dir("corpus");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());

    let mut checked = 0usize;
    let mut idx = 0usize;
    while idx < EXPECT_OK.len() {
        let name = match EXPECT_OK.get(idx) {
            Some(pair) => pair.0,
            None => break,
        };
        if evenly_selected(idx, EXPECT_OK.len(), budget) {
            assert_no_undefined_behaviour(name, &emit_ir(&dir, name));
            checked += 1;
        }
        idx += 1;
    }
    assert_eq!(checked, budget, "selected {} fixtures but checked {}", budget, checked);

    drop(guard);
}

/// The scan must match real arithmetic in the IR, or the flag assertion is vacuous.
#[test]
fn the_scan_finds_the_arithmetic_it_checks() {
    let dir = temp_dir("coverage");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());

    let ir = emit_ir(&dir, "int_width_grid");
    let mut found = 0usize;
    for line in ir.lines() {
        if arithmetic_opcode(line.trim()).is_some() {
            found += 1;
        }
    }
    assert!(
        found > 0,
        "the scan matched no arithmetic at all in int_width_grid, so its \
         overflow-flag assertion proves nothing"
    );
    eprintln!("overflow-flag scan covers {} arithmetic instructions", found);

    drop(guard);
}
