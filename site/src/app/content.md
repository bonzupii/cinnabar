<!-- @tagline -->

A statically-typed systems language with Austral-style linear typing. No garbage
collector. No lifetime annotations. No reachable panics.

<!-- @invariants-title -->

Consumed exactly once.

<!-- @invariants -->

Cinnabar grants no mechanism to bypass, suppress, weaken or defer its safety,
ownership, failure-handling and explicitness invariants. Programs must express
valid designs within those invariants; designs that require an exception are not
representable.

There is no `#[allow]`, no warning severity, no suppression pragma, and no escape
hatch to add one. If you are looking for the flag that turns a check off, its
absence is the feature.

<!-- @highlights-note -->

Enforced by the compiler, not by convention

<!-- @samples-note -->

Every sample copied verbatim from tests/fixtures/

<!-- @diagnostics-note -->

Styling study · illustrative output

<!-- @diagnostics-rules -->

Vermilion is reserved for the error and its primary span. Secondary spans, notes
and help stay grey. There is no warning colour, because there are no warnings.

<!-- @pipeline-note -->

Each stage computes its facts exactly once

<!-- @manifest-title -->

build.cnb is source, not a config format.

<!-- @manifest -->

It is read back through the compiler's own front end, so it obeys the same
casing, typing and literal rules as any other program — and a mistake in it is
reported as an ordinary diagnostic pointing at the offending line.

<!-- @closing-title -->

Cinnabar is under active early development.

<!-- @closing -->

Self-hosting — Cinnabar compiling itself — is a long-term goal and a completeness
test, not a gate for any individual feature.
