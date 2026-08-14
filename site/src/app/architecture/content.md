<!-- @lede -->

`src/main.rs` wires seven stages into one fixed, sequential pipeline. Each
stage computes its facts once and attaches them to the program representation;
nothing downstream re-derives them.

<!-- @stages-note -->

src/main.rs wires the stages in this order

<!-- @stage-lexer -->

A hand-written byte scanner writing token rows straight into the shared arena.
There is no separate token type.

<!-- @stage-parser -->

Recursive descent, no generator. Blocks close with `end`, so indentation carries
no meaning, and one bad statement does not abort the file.

<!-- @stage-module-loader -->

No package manager: `use X.y` resolves to the sibling file `X.cnb` and is parsed
recursively.

<!-- @stage-resolver -->

Scopes, imports, and the casing rules. A mis-cased identifier is an error here
and never reaches the typechecker.

<!-- @stage-typechecker -->

Structural and unification-free, over canonical interned type keys. Linearity is
inferred once, here.

<!-- @stage-borrow-checker -->

Flow-sensitive dataflow over a per-function CFG. Rejects double moves, use after
move, leaks, and overlapping `&mut` borrows.

<!-- @stage-codegen -->

Lowers type keys to LLVM, marks tail calls, then optimizes, assembles and links
statically against a staged musl.

<!-- @stages-halt -->

A failure at any stage halts the pipeline and prints source-located diagnostics.
There is no partial output.

<!-- @arena-title -->

A flat node arena, not a tree.

<!-- @arena -->

The compiler does not represent its AST, symbol table or type information as
recursive Rust enums or heap-boxed trees. Every entity is a fixed-width row in
one of three flat buffers, and every reference between entities is an integer
index.

A row's meaning is its `NODE_TAG` plus, for many tags, a secondary opcode.
Where a fact has no room in an entity's own row — a type descriptor's linearity
flag, for instance — it is piggybacked into an unused payload slot of that same
row rather than kept in a side table.

<!-- @arena-properties -->

- No Box<Node>, and no recursive Rust enum walking
- Every reference between entities is an integer index
- A row's meaning is its NODE_TAG plus a secondary opcode
- The shape a self-hosted Cinnabar compiler would use to represent itself

<!-- @arena-nodes -->

One arena where every entity — tokens, items, functions, types, expressions,
statements, patterns, resolved symbols, canonical type descriptors,
monomorphization instances, trait-dispatch facts — is a fixed-width row.

<!-- @arena-names -->

An interning table for identifiers and string data, addressed by integer id.
Equal string literals get one name id, which is what lets codegen emit a single
.rodata global per distinct literal.

<!-- @arena-lists -->

An arena of variable-length integer lists — argument lists, item lists, field
lists — addressed by list id.

<!-- @single-fact-rule -->

A fact is computed exactly once, by the stage responsible for it, and attached
to the program representation for every later stage to read. Name resolution
belongs to the resolver; types belong to the typechecker; linearity is computed
once during typechecking and read — never recomputed — by the borrow checker and
codegen. Two independent implementations of the same fact are treated as a
standing correctness bug, even if they currently happen to agree.

<!-- @source -->

`ARCHITECTURE.md` was written by reading the source directly, and is rendered
below at build time.

<!-- @document -->

ARCHITECTURE.md, in full
