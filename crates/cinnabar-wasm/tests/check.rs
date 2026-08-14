//! Confirms `check()`'s JSON diagnostic shape against real fixtures from the
//! compiler's own corpus, so a regression here is caught before it reaches
//! the site's playground rather than by a visitor typing into it.

use std::fs;
use std::path::PathBuf;

fn fixture(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(relative);
    match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(read_error) => {
            assert!(false, "cannot read fixture {}: {}", path.display(), read_error);
            String::new()
        }
    }
}

fn parse_report(source: &str) -> serde_json::Value {
    match serde_json::from_str(&cinnabar_wasm::check(source)) {
        Ok(value) => value,
        Err(parse_error) => {
            assert!(false, "check() did not return valid JSON: {}", parse_error);
            serde_json::Value::Null
        }
    }
}

#[test]
fn accepts_a_known_good_fixture() {
    let report = parse_report(&fixture("repro/hanoi.cnb"));
    assert_eq!(report["format"], "cinnabar.playground-diagnostics.v1");
    assert_eq!(report["diagnostics"].as_array().map(|list| list.len()), Some(0));
}

#[test]
fn hovers_a_function_call_with_its_signature() {
    let source = fixture("repro/hanoi.cnb");
    // The call inside hanoi_moves, not the definition -- hover should follow
    // the resolved symbol back to its signature either way, and this is a
    // slightly more honest test of that than hovering the definition site.
    let call_site = source.rfind("hanoi_acc(disks, 0)");
    let offset = match call_site {
        Some(index) => index as i32,
        None => {
            assert!(false, "fixture no longer contains a call to 'hanoi_acc'");
            return;
        }
    };
    let value: serde_json::Value = match serde_json::from_str(&cinnabar_wasm::hover(&source, offset)) {
        Ok(value) => value,
        Err(parse_error) => {
            assert!(false, "hover() did not return valid JSON: {}", parse_error);
            return;
        }
    };
    let text = match value["text"].as_str() {
        Some(text) => text,
        None => {
            assert!(false, "expected hover text at a call site, got {}", value);
            return;
        }
    };
    assert!(text.contains("hanoi_acc"), "hover text missing the function name: {}", text);
    let source_span = &value["source"];
    assert!(source_span["start"].is_i64(), "hover missing a start offset: {}", value);
    assert!(source_span["end"].is_i64(), "hover missing an end offset: {}", value);
}

#[test]
fn hovers_nothing_off_the_end_of_the_document() {
    let source = fixture("repro/hanoi.cnb");
    let value: serde_json::Value =
        match serde_json::from_str(&cinnabar_wasm::hover(&source, source.len() as i32 + 100)) {
            Ok(value) => value,
            Err(parse_error) => {
                assert!(false, "hover() did not return valid JSON: {}", parse_error);
                return;
            }
        };
    assert!(value.is_null(), "expected null past the end of the document, got {}", value);
}

#[test]
fn rejects_a_known_bad_fixture_with_located_diagnostics() {
    let report = parse_report(&fixture("repro/borrow_after_move.cnb"));
    let diagnostics = match report["diagnostics"].as_array() {
        Some(list) => list,
        None => {
            assert!(false, "expected a diagnostics array, got {}", report);
            return;
        }
    };
    assert!(!diagnostics.is_empty(), "expected borrow_after_move.cnb to be rejected");
    for diagnostic in diagnostics {
        assert_eq!(diagnostic["severity"], "error");
        assert!(diagnostic["message"].is_string(), "diagnostic missing a message: {}", diagnostic);
        let source_span = &diagnostic["source"];
        assert!(source_span["path"].is_string(), "diagnostic missing a source path: {}", diagnostic);
        assert!(source_span["start"].is_i64(), "diagnostic missing a start offset: {}", diagnostic);
        assert!(source_span["end"].is_i64(), "diagnostic missing an end offset: {}", diagnostic);
        assert!(diagnostic["explanations"].is_array(), "diagnostic missing an explanations array: {}", diagnostic);
    }
}
