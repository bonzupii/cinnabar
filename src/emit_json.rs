//! JSON document construction for the `--emit-json` output surfaces.
//!
//! Owns the format version constants, the `source` span object, and the
//! diagnostic envelope built from `Diag`/`Note` rows. Reads `errors` and
//! `notes` slices produced by the pipeline stages and the `files` table
//! from the module loader; emits `serde_json::Value` trees. Arena rows are
//! serialized by `inspect`, layout measurements by `codegen::layout`, and
//! documentation items by `docs`; each calls back into `source_json` and
//! `files_json` here.
//!
//! A span object carries the byte offsets stored on the arena row and the
//! zero-based line and UTF-16 column pair produced by
//! `analysis::offset_to_position`, the same function the language server
//! calls to build protocol positions.
//!
//! **Invariants:**
//! - No value in an emitted document is computed here; each is copied from
//!   a `Diag`, a `Note`, or an arena row an earlier stage wrote.
//! - A `NO_FILE` span serializes to JSON null; an absent file id keeps its
//!   offsets with a null `path`.

use crate::analysis::offset_to_position;
use crate::ast::{diag_kind_name, note_kind_name, Diag, Note, NO_FILE};
use serde_json::{json, Value};

/// The parse-only arena, as `--dump-ast --emit-json` emits it.
pub const AST_FORMAT: &str = "cinnabar.ast.v1";

/// The arena with every front-end attachment, as `--dump-typed-ast
/// --emit-json` emits it.
pub const TYPED_AST_FORMAT: &str = "cinnabar.typed-ast.v1";

/// The ABI layout report, as `--print-layout --emit-json` emits it.
pub const LAYOUT_FORMAT: &str = "cinnabar.layout.v1";

/// The published API documentation, as `cinnabar doc --emit-json` emits it.
pub const DOCS_FORMAT: &str = "cinnabar.docs.v1";

/// The diagnostic envelope. Every failing `--emit-json` invocation emits
/// this document, whichever stage produced the diagnostics; a clean one
/// emits it with an empty list.
pub const DIAGNOSTICS_FORMAT: &str = "cinnabar.diagnostics.v1";

/// Render one span as the source object every document shares.
///
/// A span whose file id is `NO_FILE` returns JSON null. A file id outside
/// the `files` table returns the offsets with a null `path`.
pub fn source_json(files: &[(String, String)], file: i64, start: i64, end: i64) -> Value {
    if file == NO_FILE {
        return Value::Null;
    }
    match files.get(file as usize) {
        Some(entry) => {
            let (start_line, start_column) = offset_to_position(&entry.1, start);
            let (end_line, end_column) = offset_to_position(&entry.1, end);
            json!({
                "file_id": file,
                "path": entry.0,
                "start": start,
                "end": end,
                "start_line": start_line,
                "start_column": start_column,
                "end_line": end_line,
                "end_column": end_column
            })
        }
        None => json!({
            "file_id": file,
            "path": Value::Null,
            "start": start,
            "end": end
        }),
    }
}

/// The structured diagnostic envelope: every diagnostic in `errors`, each
/// carrying its attached notes (consume paths, binding sites, prior moves).
pub fn diagnostics_report(errors: &[Diag], notes: &[Note], files: &[(String, String)]) -> Value {
    let mut diagnostics: Vec<Value> = Vec::new();
    let mut error_idx = 0usize;
    while error_idx < errors.len() {
        let error = match errors.get(error_idx) {
            Some(value) => value,
            None => break,
        };
        diagnostics.push(json!({
            "severity": "error",
            "category": diag_kind_name(&error.kind),
            "message": error.message,
            "source": source_json(files, error.file, error.start, error.end),
            "explanations": explanations_of(error_idx as i64, notes, files)
        }));
        error_idx += 1;
    }
    json!({
        "format": DIAGNOSTICS_FORMAT,
        "diagnostics": diagnostics
    })
}

fn explanations_of(diag_idx: i64, notes: &[Note], files: &[(String, String)]) -> Vec<Value> {
    let mut explanations: Vec<Value> = Vec::new();
    let mut note_idx = 0usize;
    while note_idx < notes.len() {
        match notes.get(note_idx) {
            Some(note) => {
                if note.0 == diag_idx {
                    // `kind` is the machine-readable classification; `message`
                    // is reader-facing prose.
                    explanations.push(json!({
                        "kind": note_kind_name(note.5),
                        "message": note.1,
                        "source": source_json(files, note.2, note.3, note.4)
                    }));
                }
            }
            None => break,
        }
        note_idx += 1;
    }
    explanations
}

/// The `file_id` to path mapping every arena document carries, so a node's
/// `file` slot can be read without a second load of the module set.
pub fn files_json(files: &[(String, String)]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut idx = 0usize;
    while idx < files.len() {
        match files.get(idx) {
            Some(entry) => out.push(json!({ "file_id": idx as i64, "path": entry.0 })),
            None => break,
        }
        idx += 1;
    }
    out
}

/// Serialize one document for standard output, returning a message on failure.
pub fn render_report(report: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(report)
        .map_err(|render_error| format!("cannot serialize JSON report: {}", render_error))
}
