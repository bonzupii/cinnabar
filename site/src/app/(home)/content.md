<!-- @tagline -->

A statically-typed systems language with Austral-style linear types, checked by
a flow-sensitive borrow checker. There is no `#[allow]`, and no flag that turns
a check off.

<!-- @hero-why -->

It is meant for compilers, runtimes, kernels, firmware and network stacks —
where a garbage collector, hidden control flow and a runtime panic are all
unacceptable. The compiler is written in Rust and emits native code through
LLVM 21.

<!-- @hero-steps -->

- `nix develop` provisions LLVM 21, clang and the static musl libc the compiler
  links against.
- `cargo build --release` builds `cinnabar` and, with `--bin cinnabar-lsp`, the
  language server.
- `cinnabar init` scaffolds `build.cnb`, `main.cnb` and `tests/smoke.cnb`, and
  refuses to overwrite any of them.

<!-- @invariants-title -->

What zero-trust means.

<!-- @invariants -->

Cinnabar assumes an author may optimise for finishing the task rather than for
long-term correctness — under deadline pressure, or because the author is a
code-generating model. It therefore grants no mechanism to bypass, suppress,
weaken or defer its safety, ownership, failure-handling and explicitness
invariants. A design that needs an exception is not representable in the
language.

There is no `#[allow]`, no warning severity, no suppression pragma, and no
escape hatch to add one. If you are looking for the flag that turns a check off,
its absence is the feature.

<!-- @highlights-note -->

README.md · language highlights

<!-- @highlights -->

<!--
  README.md's own list. Each heading is keyed by its slug to an icon in
  src/content/highlights.ts; rewording a heading means updating that key.
-->

### Linear resource management

Native handles — Memory.Block, Vec(T), String, HashMap(K, V) — must be consumed
exactly once on every path. No double-free, no use-after-move, no leaks, checked
statically.

### No lifetime annotations

Borrow scopes are flow-sensitive and inferred by the compiler. An ambiguous
returned borrow is a compile error, resolved by restructuring the API, not by
annotating.

### No dereference operator

There is no * and no ->. References are reached through field access, method
calls and pattern matching; the compiler manages the indirection internally.

### Errors only, never warnings

There is no lint severity and no #[allow]. A program either compiles cleanly or
is rejected with a real diagnostic.

### No panics reachable from user code

Division, modulo and dynamic indexing return Result instead of trapping.
Provable zero-division and out-of-range constant indices are compile-time errors
instead.

### O(1) call-stack recursion

Every self-recursive call must be in strict tail position. LLVM turns it into a
jump, so there is no runtime stack guard and no stack-overflow crash.

### Explicit everything

val/var, pub, impure, try — and casing itself — are compiler-enforced grammar,
not convention. A mis-cased identifier is a lexical error.

### Static, freestanding binaries

Every program links statically against a staged musl libc. No dynamic-linker
dependency in the output binary, and no dependency on the host's libc.

<!-- @samples-note -->

Verbatim from tests/fixtures/

<!-- @sample-hanoi -->

Structs and strict tail position. hanoi_acc calls itself as the direct value of
a return, which is the only self-recursive call the typechecker accepts; LLVM
turns it into a jump at -O2.

<!-- @sample-vec -->

vec is a native handle, so it carries a consumption obligation. Both the error
path and the success path have to discharge it — hence the fail_vec helper,
which frees before returning.

<!-- @sample-slice -->

Array rest-patterns and a tail-recursive fold. Match is exhaustive: every
variant, array length and rest pattern has to be covered, and there is no
catch-all arm to cover them with.

<!-- @diagnostics-note -->

Illustrative · not captured compiler output

<!-- @diagnostics-rules -->

Vermilion marks the error and its primary span. Secondary spans, notes and help
stay grey. There is no warning colour, because there are no warnings.

<!-- @pipeline-note -->

src/main.rs wires the stages in this order

<!-- @manifest-title -->

build.cnb is source, not a config format.

<!-- @manifest -->

The manifest is read back through the compiler's own front end, so it obeys the
same casing, typing and literal rules as any other program — and a mistake in it
is reported as an ordinary diagnostic pointing at the offending line.

`NAME` names the built artifact and must be a single path component. `ENTRY` and
`TESTS` are relative paths confined to the project root; `TESTS` defaults to
`tests` when omitted.

<!-- @closing-title -->

Cinnabar is under active early development.

<!-- @closing -->

Six of the eight milestones in `ROADMAP.md` are complete and two are partial.
Self-hosting — Cinnabar compiling itself — is a long-term goal and a
completeness test, not a gate for any individual feature.
