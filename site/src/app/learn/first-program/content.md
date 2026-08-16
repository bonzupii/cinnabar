The supported development environment supplies LLVM 21, clang, Rust, and the
static musl libc used by generated binaries.

<!-- @lede -->

Start in the repository’s Nix shell, build the compiler, then create a project.
The generated manifest is Cinnabar source rather than a separate config format.

## Build and scaffold

```bash
nix develop
cargo build --release
./target/release/cinnabar init hello
```

`init` creates `build.cnb`, `main.cnb`, and `tests/smoke.cnb`. It refuses to
overwrite any of them, so a partial project is never silently replaced.

## Check before linking

```bash
./target/release/cinnabar check hello
./target/release/cinnabar run hello
```

`check` runs the language front end without linking. `run` compiles the project
to a static native binary and executes it.

The browser playground is useful for lexer-through-borrow-check feedback, but
native code generation and execution require the local toolchain.
