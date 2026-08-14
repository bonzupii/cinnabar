/*
 * The compiler pipeline, from ARCHITECTURE.md.
 *
 * README.md states the canonical one-liner as seven stages:
 *   lexer → parser → module_loader → resolver → typechecker → borrow_checker → codegen
 * ARCHITECTURE.md then describes the loader as the stage that drives lexing and
 * parsing per file. Both are kept below: `order` is the README sequence, and
 * each entry names the source file that owns it.
 *
 * Only the structure is here — what the stages are, what they are called in
 * the source tree, and what order they run in. What each one does is prose,
 * and lives in src/app/architecture/content.md as a `@stage-<slug>` block. The
 * home page shows the same list without the summaries, so this module is read
 * by both routes while the wording has one home.
 */

export type Stage = {
  index: string;
  name: string;
  /** Keys this stage's summary block in the architecture route's content.md. */
  slug: string;
  file: string;
};

export const STAGES: readonly Stage[] = [
  { index: "01", name: "Lexer", slug: "lexer", file: "src/lexer.rs" },
  { index: "02", name: "Parser", slug: "parser", file: "src/parser.rs" },
  {
    index: "03",
    name: "Module loader",
    slug: "module-loader",
    file: "src/module_loader.rs",
  },
  { index: "04", name: "Resolver", slug: "resolver", file: "src/resolver.rs" },
  { index: "05", name: "Typechecker", slug: "typechecker", file: "src/typecheck.rs" },
  {
    index: "06",
    name: "Borrow checker",
    slug: "borrow-checker",
    file: "src/borrow.rs",
  },
  { index: "07", name: "Codegen", slug: "codegen", file: "src/codegen/" },
] as const;

/**
 * The three flat buffers that hold the entire compiler state.
 *
 * ARCHITECTURE.md: "Unlike a typical Rust compiler, Cinnabar does not
 * represent its AST, symbol table, or type information as recursive Rust enums
 * or heap-boxed trees."
 *
 * The name and the Rust type are the arena's identity and stay here; what each
 * one holds is a `@arena-<name>` block in the architecture route's content.md.
 */
export const ARENAS = [
  { name: "nodes", type: "Vec<i64>" },
  { name: "names", type: "Vec<String>" },
  { name: "lists", type: "Vec<Vec<i64>>" },
] as const;
