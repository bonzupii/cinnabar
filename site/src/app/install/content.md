The repository’s Nix flake is the supported path. It provides the exact LLVM,
clang, Rust, and static musl environment the compiler expects.

<!-- @lede -->

Cinnabar targets LLVM 21 and links static native binaries. Enter the supplied
development shell before building so the compiler and linker see the same
toolchain used by the project.

## Requirements

Install [Nix](https://nixos.org/download/) with flakes enabled, clone the
repository, and enter the project directory. Building outside `nix develop`
requires an equivalent LLVM 21 toolchain and a configured static `libc.a`.

## Build

```bash
nix develop
cargo build --release
```

Add `--bin cinnabar-lsp` when you also want the language server.

## Create and run a project

```bash
./target/release/cinnabar init hello
./target/release/cinnabar check hello
./target/release/cinnabar run hello
```

The scaffold contains `build.cnb`, `main.cnb`, and `tests/smoke.cnb`. Initialization
is all-or-nothing and refuses to overwrite an existing file.

## Editor support

`cinnabar-lsp` speaks LSP over stdio and uses the compiler’s attributed pipeline
for diagnostics, hover, definitions, references, completion, and signature help.
Point your editor’s `.cnb` language configuration at the built binary.

Container setup, worktree caches, attached VS Code configuration, and the full
repository verification gate belong in the [contributor development guide](/contributing/development/).
