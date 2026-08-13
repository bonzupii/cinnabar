//! Suggestion-engine diagnostics, driven through the real compiler on real
//! fixtures.
//!
//! These tests pin the wording contract the milestone demands, not merely
//! that a suggestion appeared somewhere: every suggestion is hedged, an
//! ambiguous match names no candidate, and every definition-site label
//! carries a real span (the declaration it names).

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

const BANDAID_TERMS: [&str; 5] = ["suppress", "silence", "stub", "comment out", "ignore this"];

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

/// An unused private declaration is not a rejection.
///
/// MANIFESTO.md's list of compile-time invariants names unused *imports*,
/// not unused declarations. An unused-declaration check was written for this
/// milestone and removed: its only remedy was to mark demonstration code
/// `pub`, which is the local bandaid the milestone forbids a diagnostic from
/// steering anyone towards, and it forced the random-program generator —
/// which exists to emit valid programs — to emit `pub` everywhere instead.
///
/// Asserted on the compiler's exit status rather than on the absence of a
/// word, so it fails whatever such a diagnostic ends up being called.
#[test]
fn an_unused_private_declaration_still_compiles() {
    let path = fixture("dead_code.cnb");
    let status = match Command::new(env!("CARGO_BIN_EXE_cinnabar")).arg(&path).arg("--check-only").status() {
        Ok(status) => status,
        Err(err) => {
            assert!(false, "cannot run the compiler on {}: {}", path.display(), err);
            return;
        }
    };
    assert!(
        status.success(),
        "an unused private declaration was rejected: {}",
        compiler_output(&path)
    );
}

#[test]
fn duplicate_symbol_labels_the_first_declaration() {
    let output = compiler_output(&fixture("duplicate_symbol_note.cnb"));
    assert!(output.contains("duplicate symbol 'VALUE'"), "output: {}", output);
    assert!(output.contains("first declared here"), "output: {}", output);
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
