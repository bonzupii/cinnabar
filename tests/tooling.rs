// Integration tests for the tooling layer: the analysis queries the
// language server is built on, position mapping, module-loader overlays,
// and the borrow checker's explanatory notes.  All of them consume facts
// the pipeline attached; none re-derive resolution or types.

use cinnabar::analysis::{
    analyze, completions, definition, file_id_of, hover, offset_to_position, position_to_offset,
    references, signature_help,
};
use cinnabar::ast::NO_FILE;

fn fixture(rel: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), rel)
}

fn offset_of(haystack: &str, needle: &str) -> i64 {
    match haystack.find(needle) {
        Some(pos) => pos as i64,
        None => -1,
    }
}

#[test]
fn spec_analyzes_clean() {
    let analysis = analyze(&fixture("spec.cnb"), &[]);
    assert!(analysis.resolved);
    assert!(analysis.typechecked);
    let rendered: Vec<String> = analysis.errors.iter().map(|diag| diag.0.clone()).collect();
    assert!(analysis.errors.is_empty(), "unexpected errors: {:?}", rendered);
}

#[test]
fn position_mapping_roundtrips() {
    let text = "fun a() I64\n  return 1\nend\n";
    let cases = [0i64, 4, 11, 12, 14, 25];
    let mut idx = 0usize;
    while idx < cases.len() {
        let offset = match cases.get(idx) {
            Some(value) => *value,
            None => break,
        };
        let (line, col) = offset_to_position(text, offset);
        let back = position_to_offset(text, line, col);
        assert_eq!(back, offset, "roundtrip failed for offset {}", offset);
        idx += 1;
    }
    // Multi-byte characters advance UTF-16 columns correctly: 'é' is two
    // UTF-8 bytes but one UTF-16 unit.
    let unicode = "# caf\u{e9} note\nval x = 1\n";
    let after_accent = offset_of(unicode, " note");
    let (line, col) = offset_to_position(unicode, after_accent);
    assert_eq!(line, 0);
    assert_eq!(col, 6);
    assert_eq!(position_to_offset(unicode, line, col), after_accent);
}

#[test]
fn definition_crosses_module_files() {
    let entry = fixture("multi_file/main.cnb");
    let analysis = analyze(&entry, &[]);
    assert!(analysis.resolved, "errors: {:?}", analysis.errors);
    let main_text = analysis
        .files
        .first()
        .map(|pair| pair.1.clone())
        .unwrap_or_default();
    let call_at = offset_of(&main_text, "add(10");
    assert!(call_at >= 0);
    let entry_file = file_id_of(&analysis, &entry);
    assert!(entry_file >= 0);
    let (def_file, def_start, def_end) = match definition(&analysis, entry_file, call_at) {
        Some(span) => span,
        None => (NO_FILE, 0, 0),
    };
    assert_ne!(def_file, NO_FILE, "definition of 'add' not found");
    let def_path = analysis
        .files
        .get(def_file as usize)
        .map(|pair| pair.0.clone())
        .unwrap_or_default();
    assert!(def_path.ends_with("Math.cnb"), "definition in {}", def_path);
    assert!(def_end > def_start);
}

#[test]
fn hover_shows_attached_types_and_signatures() {
    let entry = fixture("multi_file/main.cnb");
    let analysis = analyze(&entry, &[]);
    let main_text = analysis
        .files
        .first()
        .map(|pair| pair.1.clone())
        .unwrap_or_default();
    let entry_file = file_id_of(&analysis, &entry);
    // Hovering the literal 10 reports its adopted type.
    let lit_at = offset_of(&main_text, "10");
    let (lit_hover, lit_span) = match hover(&analysis, entry_file, lit_at) {
        Some(found) => found,
        None => (String::new(), (NO_FILE, 0, 0)),
    };
    assert!(lit_hover.contains("I64"), "literal hover: {}", lit_hover);
    assert_ne!(lit_span.0, NO_FILE);
    // Hovering the call to add shows the resolved signature.
    let call_at = offset_of(&main_text, "add(10");
    let (call_hover, call_span) = match hover(&analysis, entry_file, call_at) {
        Some(found) => found,
        None => (String::new(), (NO_FILE, 0, 0)),
    };
    assert!(
        call_hover.contains("fun add(a: I64, b: I64) I64"),
        "call hover: {}",
        call_hover
    );
    assert_ne!(call_span.0, NO_FILE);
}

#[test]
fn references_span_the_module_graph() {
    let entry = fixture("multi_file/main.cnb");
    let analysis = analyze(&entry, &[]);
    let main_text = analysis
        .files
        .first()
        .map(|pair| pair.1.clone())
        .unwrap_or_default();
    let entry_file = file_id_of(&analysis, &entry);
    let call_at = offset_of(&main_text, "add(10");
    let refs = references(&analysis, entry_file, call_at);
    // At least the call site in main.cnb and the declaration in Math.cnb.
    assert!(refs.len() >= 2, "references: {:?}", refs);
    let mut files_seen: Vec<i64> = Vec::new();
    let mut idx = 0usize;
    while idx < refs.len() {
        match refs.get(idx) {
            Some(span) => {
                if !files_seen.contains(&span.0) {
                    files_seen.push(span.0);
                }
            }
            None => break,
        }
        idx += 1;
    }
    assert!(files_seen.len() >= 2, "references confined to one file: {:?}", refs);
}

#[test]
fn completions_offer_symbols_locals_and_keywords() {
    let entry = fixture("multi_file/main.cnb");
    let analysis = analyze(&entry, &[]);
    let main_text = analysis
        .files
        .first()
        .map(|pair| pair.1.clone())
        .unwrap_or_default();
    let entry_file = file_id_of(&analysis, &entry);
    let inside_main = offset_of(&main_text, "return add") + 4;
    let items = completions(&analysis, entry_file, inside_main);
    let labels: Vec<String> = items.iter().map(|item| item.0.clone()).collect();
    assert!(labels.iter().any(|label| label == "main"), "missing 'main' in {:?}", labels);
    assert!(labels.iter().any(|label| label == "return"), "missing keyword in {:?}", labels);
    assert!(
        labels.iter().any(|label| label.contains("add")),
        "missing 'add' in {:?}",
        labels
    );
}

#[test]
fn completions_exclude_symbols_the_resolver_hides() {
    let entry = fixture("spec.cnb");
    let analysis = analyze(&entry, &[]);
    let text = analysis
        .files
        .first()
        .map(|pair| pair.1.clone())
        .unwrap_or_default();
    let file = file_id_of(&analysis, &entry);
    let inside_main = offset_of(&text, "return reference_checks") + 7;
    let items = completions(&analysis, file, inside_main);
    let labels: Vec<String> = items.iter().map(|item| item.0.clone()).collect();
    assert!(labels.iter().any(|label| label == "gate"), "missing visible import: {:?}", labels);
    assert!(
        !labels.iter().any(|label| label == "Runtime.probe_twice"),
        "private module member leaked into completion: {:?}",
        labels
    );
    assert!(
        !labels.iter().any(|label| label == "Binary.combine_le_bytes"),
        "private helper leaked into completion: {:?}",
        labels
    );
}

#[test]
fn completions_exclude_locals_from_sibling_branches() {
    let entry = fixture("multi_file/main.cnb");
    let source = "use Math.add\n\npub fun main() I64\n  if true\n    val only_then: I64 = 1\n    return only_then\n  else\n    val only_else: I64 = 2\n    return only_else\n  end\nend\n";
    let overlay = [(entry.clone(), source.to_string())];
    let analysis = analyze(&entry, &overlay);
    assert!(analysis.errors.is_empty(), "unexpected errors: {:?}", analysis.errors);
    let file = file_id_of(&analysis, &entry);
    let in_else = offset_of(source, "return only_else") + 7;
    let items = completions(&analysis, file, in_else);
    let labels: Vec<String> = items.iter().map(|item| item.0.clone()).collect();
    assert!(labels.iter().any(|label| label == "only_else"), "missing branch local: {:?}", labels);
    assert!(
        !labels.iter().any(|label| label == "only_then"),
        "sibling-branch local leaked into completion: {:?}",
        labels
    );
}

#[test]
fn signature_help_tracks_the_active_argument() {
    let entry = fixture("multi_file/main.cnb");
    let analysis = analyze(&entry, &[]);
    let main_text = analysis
        .files
        .first()
        .map(|pair| pair.1.clone())
        .unwrap_or_default();
    let entry_file = file_id_of(&analysis, &entry);
    let second_arg = offset_of(&main_text, "20");
    let info_found = signature_help(&analysis, entry_file, second_arg);
    assert!(info_found.is_some(), "no signature help at the call site");
    if let Some(info) = info_found {
        assert!(info.label.contains("fun add"), "label: {}", info.label);
        assert_eq!(info.params.len(), 2);
        assert_eq!(info.active, 1, "active parameter should be the second");
    }
}

#[test]
fn borrow_notes_explain_inconsistent_paths() {
    let analysis = analyze(&fixture("explain_leak.cnb"), &[]);
    let rendered: Vec<String> = analysis.errors.iter().map(|diag| diag.0.clone()).collect();
    assert!(
        rendered.iter().any(|message| message.contains("consumed on some paths")),
        "expected a path-inconsistency error, got: {:?}",
        rendered
    );
    assert!(!analysis.notes.is_empty(), "no explanatory notes were attached");
    let mut idx = 0usize;
    while idx < analysis.notes.len() {
        match analysis.notes.get(idx) {
            Some(note) => {
                assert_ne!(note.2, NO_FILE, "note carries a fabricated span: {:?}", note.1);
                assert!(note.0 >= 0 && (note.0 as usize) < analysis.errors.len());
            }
            None => break,
        }
        idx += 1;
    }
}

#[test]
fn overlay_buffers_shadow_the_file_system() {
    let entry = fixture("multi_file/main.cnb");
    let broken = "use Math.add\n\npub fun main() I64\n  return add(10)\nend\n";
    let overlay = [(entry.clone(), broken.to_string())];
    let analysis = analyze(&entry, &overlay);
    assert!(
        !analysis.errors.is_empty(),
        "overlay with an arity error should fail to check"
    );
    // The same entry with the on-disk (valid) content stays clean.
    let clean = analyze(&entry, &[]);
    assert!(clean.errors.is_empty(), "on-disk fixture should be clean: {:?}", clean.errors);
}
