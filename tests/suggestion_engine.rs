//! Suggestion-engine diagnostics, pinned through the real compiler.
//!
//! Runs real fixtures through the compiler binary and reads what it printed.
//! Pins the wording contract: every suggestion is hedged, an ambiguous match
//! names no candidate, and every definition-site label carries a real span.
//!
//! **Invariants:**
//! - Suggestions are asserted through the compiler's rendered output, never
//!   by calling the engine directly.
//! - An ambiguous case is asserted to name no candidate.

use std::path::{Path, PathBuf};
use std::process::Command;

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

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// The compiler's combined output for `path`, with colour stripped.
fn compiler_output(path: &Path) -> String {
    let output = match Command::new(env!("CARGO_BIN_EXE_cinnabar")).arg(path).output() {
        Ok(output) => output,
        Err(err) => {
            assert!(false, "cannot run the compiler on {}: {}", path.display(), err);
            return String::new();
        }
    };
    assert!(
        !output.status.success(),
        "{} was accepted; it must be rejected",
        path.display()
    );
    strip_ansi(&format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

// Shared with the engine so new terms are picked up automatically.
use cinnabar::suggest::BANDAID_TERMS;

#[test]
fn unresolved_function_offers_a_hedged_match() {
    let output = compiler_output(&fixture("suggest_unresolved_fn.cnb"));
    assert!(output.contains("unknown function 'cheksum'"), "output: {}", output);
    assert!(
        output.contains("did you mean 'checksum'?"),
        "no hedged suggestion offered, output: {}",
        output
    );
    for term in BANDAID_TERMS {
        assert!(!output.contains(term), "bandaid term '{}' in output: {}", term, output);
    }
}

#[test]
fn ambiguous_name_offers_no_candidate() {
    let output = compiler_output(&fixture("suggest_ambiguous.cnb"));
    assert!(output.contains("unknown function 'port'"), "output: {}", output);
    assert!(
        !output.contains("did you mean"),
        "an ambiguous match named a candidate, output: {}",
        output
    );
}

#[test]
fn unresolved_type_offers_a_hedged_match() {
    let output = compiler_output(&fixture("suggest_unresolved_type.cnb"));
    assert!(output.contains("unknown type 'Poit'"), "output: {}", output);
    assert!(
        output.contains("did you mean 'Point'?"),
        "no hedged type suggestion offered, output: {}",
        output
    );
    for term in BANDAID_TERMS {
        assert!(!output.contains(term), "bandaid term '{}' in output: {}", term, output);
    }
}

#[test]
fn duplicate_symbol_labels_the_first_declaration() {
    let output = compiler_output(&fixture("duplicate_symbol_note.cnb"));
    assert!(output.contains("duplicate symbol 'VALUE'"), "output: {}", output);
    assert!(output.contains("first declared here"), "output: {}", output);
    assert!(
        output.contains("pub const VALUE: I64 = 2"),
        "the diagnostic does not show the offending declaration: {}",
        output
    );
    assert!(
        output.contains("pub const VALUE: I64 = 1"),
        "the note claims to point at the first declaration but that line is \
         not rendered, so the label is not on it: {}",
        output
    );
}

#[test]
fn immutable_assignment_labels_the_val_binding() {
    let output = compiler_output(&fixture("immutable_assign.cnb"));
    assert!(
        output.contains("cannot assign to 'x': assignment requires var"),
        "output: {}",
        output
    );
    assert!(output.contains("declared here"), "output: {}", output);
}
