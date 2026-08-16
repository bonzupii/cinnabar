<!-- @tagline -->

Systems programming without safety escape hatches.

<!-- @hero-why -->

Cinnabar combines **linear resource ownership** with a **flow-sensitive borrow
checker**. The compiler proves where resources go without lifetime annotations,
a garbage collector, or a switch that weakens a safety rule.

<!-- @hero-proof -->

Edit the rejected fixture above. Each change runs through the browser build of
the real lexer, parser, resolver, typechecker, and borrow checker. Linking and
execution remain native-toolchain jobs.

<!-- @promises-note -->

The language contract, in practical terms

<!-- @promises -->

### Ownership without ceremony

Linear handles must be consumed exactly once on every path. Borrow scopes are
inferred from control flow, so APIs do not carry lifetime syntax.

### Rules without escape hatches

There are errors, not warnings, and no suppression attributes. Invalid resource
flow and unhandled failure are rejected with source-located diagnostics.

### Predictable runtime behavior

No garbage collector and no user-reachable panic path. Division, indexing, and
allocation expose failure as values; successful builds link to static binaries.

<!-- @samples-note -->

Verbatim from tests/fixtures/

<!-- @sample-hanoi -->

Strict tail recursion lowers to a jump, keeping call-stack use constant.

<!-- @sample-vec -->

The native vector carries a linear consumption obligation across success and
error paths.

<!-- @sample-slice -->

Exhaustive array rest-patterns bind the remaining elements for a recursive fold.

<!-- @depth-title -->

Start with the idea, then inspect the machine.

<!-- @depth -->

The learning path explains why the language exists before introducing ownership,
borrowing, explicit failure, and a first project. The CLI and architecture pages
then document the tools and compiler pipeline without duplicating the normative
specification.

<!-- @status-title -->

Early development, with the contracts written down.

<!-- @status -->

The compiler already exercises the fixed front-end pipeline and native backend;
the roadmap records what is resolved, in progress, and still open without
reducing that work to a misleading completion percentage.
