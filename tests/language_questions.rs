//! Staging of lexical discard and reachability diagnostics.
//!
//! Runs the compiler on probes that test ordering: discard rejection at lex
//! time regardless of later failures, and reachability after type and borrow
//! checks so type errors are not shadowed.
//!
//! **Invariants:**
//! - Discards are lexical: rejected wherever casing is, in every position,
//!   even in a file that fails to resolve.
//! - Reachability is reported after type and borrow checking.

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

/// The discard rule is lexical: it fires in a file whose undeclared return
/// type stops resolution.
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

/// Type errors are reported before unused-item diagnostics, which fire only
/// after the typechecker runs.
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
