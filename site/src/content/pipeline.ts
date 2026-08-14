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
  /** Approximate size, as ARCHITECTURE.md reports it. */
  size?: string;
  summary: string;
};

export const STAGES: readonly Stage[] = [
  {
    index: "01",
    name: "Lexer",
    file: "src/lexer.rs",
    size: "~622 lines",
    summary:
      "A hand-written byte-level scanner writing token rows straight into the shared arena — there is no separate token type. Handles the four comment forms, five string escapes, and decimal and hex literals, with checked arithmetic so overflow is caught rather than wrapped.",
  },
  {
    index: "02",
    name: "Parser",
    file: "src/parser.rs",
    size: "~1448 lines",
    summary:
      "Hand-rolled recursive descent, no generator. Indentation is not significant: blocks close with `end` and newlines separate statements. Line-based recovery means one malformed statement does not abort the rest of the file.",
  },
  {
    index: "03",
    name: "Module loader",
    file: "src/module_loader.rs",
    size: "~226 lines",
    summary:
      "There is no package manager. A `use X.y` whose first segment is not a local `mod` resolves to the sibling file `X.cnb` and is parsed recursively, producing the root item list plus one entry per externally loaded file.",
  },
  {
    index: "04",
    name: "Resolver",
    file: "src/resolver.rs",
    size: "~1498 lines",
    summary:
      "Builds a scope tree over two namespaces, seeds the builtins, hoists declarations, and enforces the casing rules here — a mis-cased identifier is a resolver error and never reaches the typechecker. Tags the entry point so later stages read the tag instead of comparing names.",
  },
  {
    index: "05",
    name: "Typechecker",
    file: "src/typecheck.rs",
    size: "~3900 lines",
    summary:
      "Structural and unification-free, keyed by canonical interned type keys. Evaluates constants, records monomorphization instances, and infers linearity once — a generic parameter is conservatively linear, since its instantiation is unknown at definition time.",
  },
  {
    index: "06",
    name: "Borrow checker",
    file: "src/borrow.rs",
    size: "~3274 lines",
    summary:
      "Flow-sensitive dataflow over a per-function control-flow graph. Enforces exactly-once consumption on every path out of scope, aliasing exclusivity, field-level partial moves, and rejection of ambiguous returned borrows — all from facts earlier stages attached, never by matching type names.",
  },
  {
    index: "07",
    name: "Codegen",
    file: "src/codegen/",
    summary:
      "Lowers canonical type keys to LLVM types, marks self-tail-recursive calls `tail`, then shells out to `opt`, `llc` and `clang -static -nostdlib -no-pie`. Links against a musl libc staged into the compiler binary at build time.",
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
