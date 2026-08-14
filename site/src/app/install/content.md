<!-- @lede -->

Cinnabar targets LLVM 21 via `inkwell` and needs `clang`, `llc` and `opt` on
`PATH`, plus a static musl libc to link Cinnabar binaries against. The project
ships a Nix flake that provisions all of it.

<!-- @nix -->

The flake provides LLVM, clang, the Rust toolchain, and the static musl libc
that `build.rs` stages into the compiler binary.

<!-- @nix-outside -->

Outside `nix develop`, `cargo build` and `cargo clippy` will fail unless you have
a matching LLVM 21 toolchain and `MUSL_LIBC_A` — pointing at a static `libc.a` —
configured yourself. See `build.rs` and `flake.nix` for the exact discovery logic
and paths.

<!-- @first-program -->

`init` scaffolds `build.cnb`, `main.cnb` and `tests/smoke.cnb`. It refuses to
overwrite: if any of the three exists, it writes none of them.

<!-- @first-program-file -->

Or compile a single file straight through the pipeline:

<!-- @docker -->

Windows contributors can run the same Nix environment in one reusable Docker
Compose service. Its named Nix and Cargo caches survive branch changes, while
every worktree receives an isolated Rust `target` volume. The setup also includes
the linked-worktree Git mounts Nix needs and a rust-analyzer wrapper that runs
inside `nix develop`.

Native Linux development remains Nix-first and does not require Docker.

<!-- @lsp -->

The repository also builds `cinnabar-lsp`, a Language Server Protocol server over
the same compiler pipeline. It speaks stdio and provides diagnostics — with the
borrow checker's explanatory notes as related information and code lenses —
hover, go-to-definition and find-references across the module graph, completion,
and signature help.

Every answer is read from the facts the pipeline attaches. The server contains no
second implementation of name resolution or type inference.

<!-- @lsp-vscode -->

In VS Code, point a generic LSP extension at the binary for `.cnb` files.

<!-- @gate -->

The repository's build gate is `pre_commit_check.sh`, run inside the Nix dev
shell. It runs `cargo check`, `cargo clippy -D warnings`, a custom Semgrep
ruleset, `cargo test`, CLI smoke checks, compiles and dumps several fixtures,
runs the compiled `spec.cnb` reference binary, and runs a battery of negative
fixtures.

<!-- @profiles -->

For quicker local feedback, the `balanced` and `smoke` profiles reduce the
randomized corpus sizes and the number of successful fixtures that are linked and
executed. Rejected fixtures remain checked in every profile.

Run the full gate with no profile override before submitting a change.
