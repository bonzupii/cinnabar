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
