//! The Cinnabar compiler pipeline, exposed as a library.
//!
//! The CLI driver (`src/main.rs`) and the language server
//! (`src/bin/cinnabar_lsp.rs`) consume one shared implementation of every
//! stage through this crate; each stage computes its facts once and attaches
//! them to the flat node arena for later consumers to read.
//!
//! **Invariants:**
//! - There is exactly one implementation of each stage.

pub mod analysis;
pub mod advanced_tools;
pub mod ast;
pub mod borrow;
#[cfg(feature = "codegen")]
pub mod codegen;
pub mod docs;
pub mod emit_json;
pub mod format;
pub mod inspect;
pub mod lexer;
pub mod module_loader;
pub mod native_stub;
pub mod parser;
pub mod project;
pub mod resolver;
pub mod snapshot_review;
pub mod suggest;
pub mod target;
pub mod typecheck;
