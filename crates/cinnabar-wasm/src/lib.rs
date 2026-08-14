//! WebAssembly entry point for the site's in-browser Cinnabar checker.
//!
//! `check` runs exactly the front end the language server runs —
//! `cinnabar::analysis::analyze`, through `lex -> parse -> resolve ->
//! typecheck -> borrow_check` — over a single in-memory source and never
//! anything past it. There is no second pipeline here: the Single-Fact Rule
//! this crate borrows from `cinnabar`'s own `analysis.rs` means a playground
//! answer can never disagree with what a real build or the LSP would say.
//!
//! The `cinnabar` dependency below is pinned to `default-features = false`,
//! which drops its `codegen` feature and, with it, `inkwell`/LLVM — the one
//! stage of the real pipeline that cannot target `wasm32-unknown-unknown`.
//! `check` therefore cannot link, execute, or otherwise run whatever source
//! it's given; it can only report what the front end established about it.

use cinnabar::analysis;
use cinnabar::ast::NO_FILE;
use serde_json::{json, Value};
use wasm_bindgen::prelude::wasm_bindgen;

/// The synthetic path `analyze` is called with. Never touches the real
/// filesystem: `module_loader::read_source` checks the overlay before
/// falling back to `std::fs`, and this path is always present in the
/// overlay, so that fallback is unreachable here.
const ENTRY_PATH: &str = "playground.cnb";

#[wasm_bindgen]
pub fn check(source: &str) -> String {
    let overlay = [(ENTRY_PATH.to_string(), source.to_string())];
    let result = analysis::analyze(ENTRY_PATH, &overlay);
    let report = json!({
        "format": "cinnabar.playground-diagnostics.v1",
        "diagnostics": diagnostics_json(&result),
    });
    match serde_json::to_string(&report) {
        Ok(rendered) => rendered,
        Err(serialize_error) => {
            json!({
                "format": "cinnabar.playground-diagnostics.v1",
                "diagnostics": [],
                "serialization_error": serialize_error.to_string(),
            })
            .to_string()
        }
    }
}

fn source_json(files: &[(String, String)], file: i64, start: i64, end: i64) -> Value {
    if file == NO_FILE {
        return Value::Null;
    }
    match files.get(file as usize) {
        Some(entry) => json!({
            "file_id": file,
            "path": entry.0,
            "start": start,
            "end": end,
        }),
        None => json!({
            "file_id": file,
            "path": Value::Null,
            "start": start,
            "end": end,
        }),
    }
}

fn diagnostics_json(result: &analysis::Analysis) -> Vec<Value> {
    let mut diagnostics: Vec<Value> = Vec::new();
    let mut error_idx = 0usize;
    while error_idx < result.errors.len() {
        let error = match result.errors.get(error_idx) {
            Some(value) => value,
            None => break,
        };
        let mut explanations: Vec<Value> = Vec::new();
        let mut note_idx = 0usize;
        while note_idx < result.notes.len() {
            match result.notes.get(note_idx) {
                Some(note) if note.0 == error_idx as i64 => {
                    explanations.push(json!({
                        "message": note.1,
                        "source": source_json(&result.files, note.2, note.3, note.4),
                    }));
                }
                _ => {}
            }
            note_idx += 1;
        }
        diagnostics.push(json!({
            "severity": "error",
            "message": error.0,
            "source": source_json(&result.files, error.1, error.2, error.3),
            "explanations": explanations,
        }));
        error_idx += 1;
    }
    diagnostics
}
