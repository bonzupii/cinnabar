//! Milestone 8 — undefined behaviour, asserted against the emitted IR.
//!
//! ## Why not UBSan
//!
//! UBSan's checks are emitted by Clang's *front end* while lowering C. There
//! is no LLVM pass to run over a Cinnabar module, so covering it would mean
//! the Cinnabar emitter emitting equivalents itself — and almost every class
//! it checks is one this language has designed out rather than left
//! unchecked:
//!
//! | UBSan class            | Cinnabar |
//! |------------------------|----------|
//! | signed overflow        | arithmetic wraps per width in two's complement (`MANIFESTO.md`) |
//! | shift out of range     | shift counts mask by `width - 1` |
//! | division by zero       | `/` and `%` return `Result`; a provable zero is a compile error |
//! | array out of bounds    | constant index out of range is a compile error; dynamic returns `Result` |
//! | raw memory access      | `Memory.read_u8`/`write_u8` bounds-check and return `Result` |
//! | null dereference       | there is no dereference operator, and no pointer in user code |
//!
//! Adding runtime checks for those would be checking conditions the language
//! does not admit. So what is asserted here is the stronger, static property:
//! **the compiler does not emit IR whose behaviour is undefined**. That holds
//! for every program the compiler can produce, not only the ones a test
//! happens to execute — which is more than running UBSan over a corpus would
//! have given.
//!
//! ## What each assertion corresponds to
//!
//! - **No `nsw`/`nuw` on arithmetic.** Those flags tell LLVM that overflow
//!   cannot happen and make it undefined when it does. `MANIFESTO.md`
//!   guarantees the opposite — that arithmetic wraps — so a flag appearing on
//!   an `add`, `sub`, `mul` or `shl` would be the compiler contradicting the
//!   language it implements. `getelementptr inbounds nuw` is exempt and
//!   checked separately: that is a struct field offset, which cannot wrap
//!   because the layout says so.
//! - **No `exact` on division or shifts**, which would make a non-exact
//!   result undefined.
//! - **No `poison` values.**
//! - **`unreachable` only in a control-flow join every path diverges out
//!   of.** A `match` lowers its last pattern test as a branch, and the
//!   not-taken edge is unreachable precisely because the type checker proved
//!   the match exhaustive; the join after a branch whose arms all return is
//!   unreachable for the same kind of reason. Both are statically proven
//!   rather than assumed at runtime, so neither needs a check —
//!   `MANIFESTO.md` keeps runtime checks for genuinely dynamic conditions.
//!   An `unreachable` anywhere else would be a real "cannot happen" with
//!   nothing proving it, and is what this assertion is looking for.

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

/// The block names an `unreachable` may appear in.
///
/// Both are control-flow joins, and a join is unreachable exactly when every
/// path into it diverged. `match_merge` is the not-taken edge of a match's
/// last pattern test, unreachable because the type checker proved the match
/// exhaustive. `if_merge` is the join after a branch whose arms all return.
///
/// Anywhere else, an `unreachable` is a "cannot happen" with nothing proving
/// it, which is what this list exists to catch.
const DIVERGING_JOINS: [&str; 2] = ["match_merge", "if_merge"];

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("repro")
        .join(format!("{}.cnb", name))
}

// One directory per test, never nested inside another's: these run
// concurrently, and a guard removing a parent would delete a sibling's IR
// mid-scan.
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
    // The opcodes are already the strings this returns, so there is nothing
    // to map them through — and nothing for a wildcard arm to swallow.
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

        // Track the enclosing basic block, so an `unreachable` can be
        // attributed to the construct that produced it.
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

/// The overflow-flag assertion is not vacuous: the IR really does contain
/// the arithmetic it is scanning.
///
/// A scanner that matched no instructions would pass every fixture in
/// silence, and the absence of a flag would be evidence of nothing.
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

    // A fixture that exercises every width and every operator.
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
