/*
 * The compiler pipeline, from ARCHITECTURE.md.
 *
 * README.md states the canonical one-liner as seven stages:
 *   lexer → parser → module_loader → resolver → typechecker → borrow_checker → codegen
 * ARCHITECTURE.md then describes the loader as the stage that drives lexing and
 * parsing per file. Both are kept below: `order` is the README sequence, and
 * each entry names the source file that owns it.
 */

export type Stage = {
  index: string;
  name: string;
  file: string;
  summary: string;
};

export const STAGES: readonly Stage[] = [
  {
    index: "01",
    name: "Lexer",
    file: "src/lexer.rs",
    summary:
      "A hand-written byte scanner writing token rows straight into the shared arena. There is no separate token type.",
  },
  {
    index: "02",
    name: "Parser",
    file: "src/parser.rs",
    summary:
      "Recursive descent, no generator. Blocks close with `end`, so indentation carries no meaning, and one bad statement does not abort the file.",
  },
  {
    index: "03",
    name: "Module loader",
    file: "src/module_loader.rs",
    summary:
      "No package manager: `use X.y` resolves to the sibling file `X.cnb` and is parsed recursively.",
  },
  {
    index: "04",
    name: "Resolver",
    file: "src/resolver.rs",
    summary:
      "Scopes, imports, and the casing rules. A mis-cased identifier is an error here and never reaches the typechecker.",
  },
  {
    index: "05",
    name: "Typechecker",
    file: "src/typecheck.rs",
    summary:
      "Structural and unification-free, over canonical interned type keys. Linearity is inferred once, here.",
  },
  {
    index: "06",
    name: "Borrow checker",
    file: "src/borrow.rs",
    summary:
      "Flow-sensitive dataflow over a per-function CFG. Rejects double moves, use after move, leaks, and overlapping `&mut` borrows.",
  },
  {
    index: "07",
    name: "Codegen",
    file: "src/codegen/",
    summary:
      "Lowers type keys to LLVM, marks tail calls, then optimizes, assembles and links statically against a staged musl.",
  },
] as const;

/**
 * The rule ARCHITECTURE.md calls governing: a fact is computed once, by the
 * stage responsible for it, and attached for every later stage to read.
 */
export const SINGLE_FACT_RULE =
  "A fact is computed exactly once, by the stage responsible for it, and attached to the program representation for every later stage to read. Name resolution belongs to the resolver; types belong to the typechecker; linearity is computed once during typechecking and read — never recomputed — by the borrow checker and codegen. Two independent implementations of the same fact are treated as a standing correctness bug, even if they currently happen to agree.";

/**
 * The three flat buffers that hold the entire compiler state.
 *
 * ARCHITECTURE.md: "Unlike a typical Rust compiler, Cinnabar does not
 * represent its AST, symbol table, or type information as recursive Rust enums
 * or heap-boxed trees."
 */
export const ARENAS = [
  {
    name: "nodes",
    type: "Vec<i64>",
    summary:
      "One arena where every entity — tokens, items, functions, types, expressions, statements, patterns, resolved symbols, canonical type descriptors, monomorphization instances, trait-dispatch facts — is a fixed-width row.",
  },
  {
    name: "names",
    type: "Vec<String>",
    summary:
      "An interning table for identifiers and string data, addressed by integer id. Equal string literals get one name id, which is what lets codegen emit a single .rodata global per distinct literal.",
  },
  {
    name: "lists",
    type: "Vec<Vec<i64>>",
    summary:
      "An arena of variable-length integer lists — argument lists, item lists, field lists — addressed by list id.",
  },
] as const;

/** What the arena design buys, stated as short claims. */
export const ARENA_PROPERTIES = [
  "No Box<Node>, and no recursive Rust enum walking",
  "Every reference between entities is an integer index",
  "A row's meaning is its NODE_TAG plus a secondary opcode",
  "The shape a self-hosted Cinnabar compiler would use to represent itself",
] as const;
