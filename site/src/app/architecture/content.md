<!-- @lede -->

`src/main.rs` wires seven stages into one fixed, sequential pipeline. Each
stage computes its facts once and attaches them to the program representation;
nothing downstream re-derives them.

<!-- @stages-note -->

Expand a stage for what it does

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

<!-- @source -->

`ARCHITECTURE.md` was written by reading the source directly, and is rendered
below at build time.

<!-- @document -->

ARCHITECTURE.md, in full
