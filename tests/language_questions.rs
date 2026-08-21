//! Two rules, and the stage each is enforced at.
//!
//! Both began as questions `MANIFESTO.md` did not answer, and both were at
//! some point answered accidentally by an implementation before anyone
//! decided them. Both implementations were reverted; the rules that stand
//! now were decided first and implemented second.
//!
//! What each rule *says* is asserted in `tests/rejection_diagnostics.rs`,
//! message by message. What this file asserts is *when* each is reported —
//! which neither a diagnostic list nor an exit code can see, and which is
//! the part that was got wrong once already.
//!
//! **Invariants:**
//! - **Discards are lexical.** A leading underscore is rejected where casing
//!   is, so the rule holds in every position at once — match arm, binding,
//!   parameter, field — rather than in each place someone remembered to
//!   check. A file that would fail later for other reasons still reports it.
//! - **Reachability is reported after type and borrow checking.** Reported
//!   from the resolver instead, it would stop the pipeline first, and a
//!   program that does not type-check would be told which of its functions
//!   nothing calls rather than what is wrong with it. That is exactly the
//!   shadowing that left `invalid_resolver_and_typechecker.cnb` asserting
//!   four of its twenty-four cases.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::new();
    let mut escaped = false;
    for ch in text.chars() {
        if escaped {
            if ch.is_ascii_alphabetic() {
                escaped = false;
            }
            continue;
        }
        if ch == '\u{1b}' {
            escaped = true;
            continue;
        }
        out.push(ch);
    }
    out
}

fn compiler_output(path: &Path) -> String {
    match Command::new(env!("CARGO_BIN_EXE_cinnabar")).arg(path).arg("--check-only").output() {
        Ok(output) => strip_ansi(&format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )),
        Err(err) => {
            assert!(false, "cannot run the compiler on {}: {}", path.display(), err);
            String::new()
        }
    }
}

/// A discard is rejected even in a file that never reaches the resolver.
///
/// The rule is lexical, so it does not depend on the program being
/// otherwise well-formed. Moved into a later stage, this file — whose
/// undeclared return type stops resolution — would report the missing type
/// and say nothing about the discard, and the rule would silently hold only
/// in programs that were already correct.
#[test]
fn a_discard_is_reported_before_the_program_resolves() {
    let dir = std::env::temp_dir().join(format!("cinnabar_lexical_{}", std::process::id()));
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let path = dir.join("unresolvable.cnb");
    // `NoSuchType` does not exist, so this file cannot get past the
    // resolver — and the discard must still be reported.
    match std::fs::write(&path, "fun main() NoSuchType\n  val _ = 1\n  return 0\nend\n") {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot write the probe: {}", err);
            return;
        }
    }
    let output = compiler_output(&path);
    assert!(
        output.contains("discard pattern"),
        "a discard went unreported in a file that fails to resolve, so the \
         rule is no longer lexical: {}",
        output
    );
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => eprintln!("probe cleanup failed: {}", err),
    }
}

/// Reachability does not shadow the stages that can explain a broken program.
///
/// `invalid_typechecker.cnb` has type errors, and its items are reached only
/// because it declares them for that purpose. It must report the type
/// errors: they are what is wrong with it. Reported from the resolver, the
/// unused-item diagnostics would fire first and the type checker would never
/// run at all.
#[test]
fn reachability_does_not_shadow_type_errors() {
    let output = compiler_output(&fixture("invalid_typechecker.cnb"));
    assert!(
        output.contains("constant initializer type mismatch"),
        "the type checker did not run, so reachability is being reported too \
         early: {}",
        output
    );
    assert!(
        !output.contains("unused "),
        "unused-item diagnostics were reported alongside type errors; a \
         broken program should be told what is wrong with it first: {}",
        output
    );
}
