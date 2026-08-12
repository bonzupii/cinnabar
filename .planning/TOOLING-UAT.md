---
status: complete
source: .planning/TOOLING.md
verified: 2026-08-11
implemented_claims: 10
passed: 10
failed: 0
blocked: 0
---

# Tooling End-to-End Verification

## Scope and verdict

Every capability that `TOOLING.md` labels implemented, done, delivered, or complete is present and
verified through its public boundary. Verification uses actual compiler binaries and protocols,
not source-presence checks or duplicated tooling logic.

The document is also truthful about work that is not delivered. The deferred items listed below
are roadmap scope and are not counted as implemented claims.

## Verification matrix

| Area | Public boundary exercised | Result | Evidence |
|------|---------------------------|--------|----------|
| Attached-facts CLI | `--dump-typed-ast`, `--print-layout`, `--emit-llvm`, `--emit-obj`, borrow diagnostics | PASS | `tests/tooling_tiers.rs`; compiler and rejection suites |
| LSP analysis | initialize, diagnostics, hover, definition, references, completion, signature help, code lenses, multi-root overlay, stale-result suppression | PASS | `tests/lsp_protocol.rs` over the real `cinnabar-lsp` stdio protocol |
| Editor package | `.cnb` registration, language configuration, canonical grammar, extension packaging | PASS | `tests/tooling_tiers.rs`; `npm pack --dry-run` |
| Formatter | check failure, in-place formatting, idempotent re-check, formatted-source front end | PASS | `tests/tooling_tiers.rs` and formatter integration suite |
| Documentation | attached doc facts, public API generation, Cinnabook HTTP response and compiler version | PASS | documentation suite; `tests/tooling_tiers.rs` over a real TCP server |
| Project/test workflow | manifest discovery, build, run, check, test, init, rejection snapshots and exit expectations | PASS | project integration suite and CLI fixture gate |
| Native surface | typed IDL parsing, generic native type/function emission, generated source compilation | PASS | `tests/tooling_tiers.rs` |
| Binary/target inspection | canonical layouts, sections, symbols, disassembly, host target, unavailable-target rejection | PASS | `tests/tooling_tiers.rs` |
| Learning/verification | seven Mushlings, verify workflow, deterministic fuzz replay/minimization, soundness JSON | PASS | `tests/tooling_tiers.rs` |
| Local playground | page load, compile/run, five-second termination, one-MiB rejection, server recovery | PASS | `tests/tooling_tiers.rs` over the real loopback HTTP server |

## Repository gate

The mandated command completed successfully on 2026-08-11:

```text
nix develop --command ./pre_commit_check.sh
```

The Windows workspace invoked that exact command inside the configured development container. The
gate passed Cargo check, zero-warning Clippy, opaque Semgrep policy checks, all Cargo tests, the
reference specification compile-and-run, AST checks, positive fixtures, and required rejection
fixtures. Full output is retained in `pre_commit.log`.

## Correctness rationale

The tests drive the same attached arena, canonical type layouts, borrow facts, executable compiler,
and server entry points used by users. Native stubs are compiled after generation; inspection is
derived from the code generator's layout report; LSP features are requested through JSON-RPC; and
HTTP tools are reached over TCP. No test introduces a second resolver, type inference pass, layout
table, name-based semantic special case, or fixture-sized registry.

The playground now handles execution timeout and request-size errors per connection, removes its
temporary source/binary, sends a truthful response, and accepts a subsequent request. This closes
the failure path that the original happy-path HTTP check did not cover.

## Explicitly deferred scope

- Structurally attached diagnostic suggestions and LSP code actions (Milestone 6)
- Incremental reuse inside the compiler/LSP pipeline; accepted edits currently run the full pipeline
- Non-host target backends and their runtimes; the driver rejects unavailable targets before compile
- Expanded ASan/UBSan/Valgrind fixture gate (Milestone 7 roadmap item)
- Mechanized progress/preservation proof; `cinnabar soundness` truthfully reports `formal_proof: false`

These are not verification failures: `TOOLING.md` does not claim they are implemented.
