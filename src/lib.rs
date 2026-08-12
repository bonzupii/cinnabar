// The Cinnabar compiler pipeline, exposed as a library so that the CLI
// driver (`src/main.rs`) and the language server (`src/bin/cinnabar_lsp.rs`)
// consume one shared implementation of every stage.  The pipeline itself is
// unchanged: each stage computes its facts once and attaches them to the
// flat node arena for every later consumer to read (Single-Fact Rule).

pub mod analysis;
pub mod advanced_tools;
pub mod ast;
pub mod borrow;
pub mod codegen;
pub mod docs;
pub mod format;
pub mod inspect;
pub mod lexer;
pub mod module_loader;
pub mod native_stub;
pub mod parser;
pub mod project;
pub mod resolver;
pub mod typecheck;
