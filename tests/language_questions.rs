//! The open language question, pinned at its current answer.
//!
//! `MANIFESTO.md` does not settle it, and this test exists so that settling
//! it is a deliberate act rather than a side effect. It was, at some point,
//! answered accidentally by an implementation before anyone decided it, and
//! that implementation was reverted. What is left is the behaviour as it
//! stands — not an endorsement of it.
//!
//! The other question that lived here — whether an unused declaration is an
//! error — has been answered: it is, for public and private items alike.
//! Its fixture moved to `tests/rejection_diagnostics.rs`, where the two
//! diagnostics it must produce are asserted individually.
//!
//! Each is asserted on the compiler's **exit status** rather than on the
//! absence of a particular word, so it fails whatever a future rejection
//! ends up being called.
//!
//! The fixtures also exist as the programs to check once each question is
//! settled, which is why they are asserted at all: `09_discard_patterns.cnb`
//! sat broken for two commits — it did not compile, for reasons unrelated to
//! discards — while being cited as the record of what discards do. A record
//! nothing runs is not a record.
//!
//! **Invariants:**
//! - Each question is asserted on the compiler's exit status, never on the
//!   presence of a particular word, so the test survives whatever a future
//!   rejection ends up being called.
//! - Answering a question here is a language change and belongs in
//!   `MANIFESTO.md` first. Changing one of these assertions to match a new
//!   implementation, rather than to match a decision, is the exact failure
//!   this file exists to prevent.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

/// The compiler's combined output for `path`, for use in failure messages.
fn compiler_output(path: &Path) -> String {
    match Command::new(env!("CARGO_BIN_EXE_cinnabar")).arg(path).arg("--check-only").output() {
        Ok(output) => format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(err) => format!("(could not run the compiler: {})", err),
    }
}

fn accepts(path: &Path) -> bool {
    match Command::new(env!("CARGO_BIN_EXE_cinnabar")).arg(path).arg("--check-only").status() {
        Ok(status) => status.success(),
        Err(err) => {
            assert!(false, "cannot run the compiler on {}: {}", path.display(), err);
            false
        }
    }
}

/// **Open question: does Cinnabar reject discard patterns?** Today it does not.
///
/// A bare underscore as a match arm, as a binding, and as the prefix of one
/// all compile. The match-arm case is the consequential one: a catch-all
/// makes any match trivially exhaustive, so adding a variant to an enum
/// stops forcing anyone to handle it.
///
/// `MANIFESTO.md` does not ban discards. The prohibition in `AGENTS.md`
/// governs this compiler's own Rust source, not the language it compiles.
#[test]
fn discard_patterns_still_compile() {
    let path = fixture("09_discard_patterns.cnb");
    assert!(
        accepts(&path),
        "the discard-pattern record does not compile, so it records nothing: {}",
        compiler_output(&path)
    );
}

/// The discard record runs, rather than merely compiling.
///
/// Exit zero, and only because every check inside it passed — including the
/// one that reaches the catch-all arm. A file that compiled but computed the
/// wrong thing would still be a broken record.
#[test]
fn the_discard_record_runs_and_agrees_with_itself() {
    let source = fixture("09_discard_patterns.cnb");
    let binary = std::env::temp_dir().join(format!("cinnabar_discards_{}", std::process::id()));
    let built = Command::new(env!("CARGO_BIN_EXE_cinnabar")).arg(&source).arg("-o").arg(&binary).output();
    match built {
        Ok(output) => assert!(
            output.status.success(),
            "the discard record failed to build: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(err) => {
            assert!(false, "cannot run the compiler: {}", err);
            return;
        }
    }
    let ran = Command::new(&binary).status();
    match ran {
        Ok(status) => assert_eq!(
            status.code(),
            Some(0),
            "the discard record exited {:?}, so one of its own checks failed",
            status.code()
        ),
        Err(err) => assert!(false, "cannot run {}: {}", binary.display(), err),
    }
    match std::fs::remove_file(&binary) {
        Ok(()) => {}
        Err(err) => eprintln!("discard record cleanup failed: {}", err),
    }
}
