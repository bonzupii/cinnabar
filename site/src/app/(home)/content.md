<!-- @tagline -->

A statically-typed systems language with Austral-style linear typing. No garbage
collector. No lifetime annotations. No reachable panics.

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

<!-- @samples-note -->

Verbatim from tests/fixtures/

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
