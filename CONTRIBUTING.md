# Contributing to Cinnabar

Cinnabar is a from-scratch compiler with a strict, opinionated set of invariants — read before you write code, not after.

## Before you touch code

1. **[`MANIFESTO.md`](MANIFESTO.md)** — the normative language specification. If any code, fixture, or doc disagrees with it, the manifesto wins.
2. **[`tests/fixtures/spec.cnb`](tests/fixtures/spec.cnb)** — the immutable reference implementation fixture. Never edit it.
3. **[`ARCHITECTURE.md`](ARCHITECTURE.md)** — how the compiler pipeline (`lexer → parser → module_loader → resolver → typechecker → borrow_checker → codegen`) is actually built.
4. **[`AGENTS.md`](AGENTS.md)** — the repository's binding working conventions: no `unwrap`/`expect`/`panic!`, no `_` discard bindings, no re-derived facts across pipeline stages, category-level fixes only (never patch a single fixture), file header comment format, and the exact verification gate below. These conventions apply to every contributor, human or AI.

## Getting a dev environment

Cinnabar needs LLVM 21, a staged musl libc, and a handful of other pinned tools, so [Nix](https://nixos.org/) is the supported way to get a working toolchain:

```bash
nix develop
```

This drops you into a shell with the right `rustc`, `llvm`, `clang`, `clippy`, `semgrep`, and friends already on `PATH`. `cargo build`/`cargo clippy` will not work outside this shell (no `llvm-config` on a bare host). A ready-made dev container (`compose.dev.yaml`, see [`CONTAINER_DEVELOPMENT.md`](CONTAINER_DEVELOPMENT.md)) is also available if you'd rather not install Nix locally.

## Making a change

- Read the relevant part of the spec and existing code before editing — see [`ARCHITECTURE.md`](ARCHITECTURE.md) for where each concern lives.
- Fix the general case. A change that makes a single failing fixture pass without stating (and implementing) the general rule it represents will be rejected — see "Category-Level Fixes Only" in [`AGENTS.md`](AGENTS.md).
- Match existing conventions: casing rules, typed errors with real spans end-to-end, no swallowed values, no re-implemented facts a prior pipeline stage already computed.
- New `.cnb` fixtures under `tests/fixtures/` are welcome as regression tests or examples; they're reference data, not spec.

## Verifying a change

The build gate is [`pre_commit_check.sh`](pre_commit_check.sh), run inside the Nix dev shell:

```bash
nix develop --command ./pre_commit_check.sh
```

It runs `cargo check`, `cargo clippy -D warnings`, the project's Semgrep ruleset, `cargo test`, CLI smoke checks, fixture compilation/AST checks, and the full negative-fixture rejection suite. Results land in `pre_commit.log` (gitignored). This is exactly what [CI](.github/workflows/ci.yml) runs on every push and pull request — a green run locally means a green run in CI.

The script itself is never modified by contributors; if you think a check is wrong, open an issue rather than editing it.

## Submitting a change

- Keep commits scoped to what you actually changed — don't stage or reformat unrelated files.
- Describe *why* a change is correct for the general case the spec describes, not just that the gate passed (a green gate is necessary but not sufficient evidence of correctness).
- Open a pull request against `main`. CI (the pre-commit gate) must pass before review.
- For anything security-relevant (a soundness hole, a way to make the borrow checker accept a use-after-move, a memory-safety bug in generated code), see [`SECURITY.md`](SECURITY.md) instead of a public issue.

## Reporting bugs and proposing features

Use [GitHub Issues](https://github.com/bonzupii/cinnabar/issues). For a rejected-when-it-shouldn't-be or accepted-when-it-shouldn't-be program, include the minimal `.cnb` source that reproduces it and what you expected instead. See [`ROADMAP.md`](ROADMAP.md) for what's already planned before proposing a new feature.
