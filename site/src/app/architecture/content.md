<!-- @lede -->

Cinnabar does not represent its AST, symbol table or type information as
recursive enums or heap-boxed trees. The entire compiler state is three flat,
allocation-only buffers, and one fixed pipeline runs over them.

<!-- @arena-title -->

A flat node arena, not a tree.

<!-- @arena -->

Each row in `nodes` has a fixed stride, and its meaning is determined by its
`NODE_TAG` plus, for many tags, a secondary opcode. Generic accessors read and
write those rows.

Where a fact has no room in an entity's own row it is piggybacked into an unused
payload slot of that same row, rather than introduced as a separate side table —
keeping the Single-Fact Rule intact without growing the number of arenas.

<!-- @stages-note -->

Each stage attaches its facts for later stages to read

<!-- @source -->

The full technical walkthrough below is `ARCHITECTURE.md`, rendered at build
time.
