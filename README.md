# Cinnabar

Cinnabar is a from-scratch, statically-typed **systems programming language** and compiler, written in Rust and targeting native machine code via LLVM. It is designed for building compilers, runtimes, kernels, firmware, and network stacks — domains where garbage collection, hidden control flow, and runtime panics are unacceptable.

The language's defining feature is **Austral-style linear typing**: resource-owning handles (heap memory, vectors, strings, hash maps, sockets) must be consumed exactly once on every execution path, enforced entirely at compile time by a dedicated flow-sensitive borrow checker — with no lifetime annotations, no garbage collector, and no reference counting.

> The authoritative language specification is [`MANIFESTO.md`](MANIFESTO.md). If anything below (or any `.cnb` file in the repo) contradicts it, the manifesto wins.

## Language highlights

- **Linear resource management.** Native handles (`Memory.Block`, `Collections.Vec(T)`, `Collections.String`, `Collections.HashMap(K, V)`) must be consumed exactly once on every path — no double-free, no use-after-move, no leaks, checked statically.
- **No lifetime annotations.** Borrow scopes are flow-sensitive and inferred by the compiler; an ambiguous returned borrow is a compile error, resolved by restructuring the API, not by annotating.
- **No dereference operator.** There is no `*` or `->`. References are accessed through field access, method calls, and pattern matching; the compiler manages indirection internally.
- **Errors only, never warnings.** There is no lint severity, no `#[allow]`. A program either compiles cleanly or is rejected with a real diagnostic.
- **No panics reachable from user code.** Division, modulo, and dynamic indexing return `Result` instead of trapping. Constant-provable zero-division and out-of-range constant indices are compile-time errors instead.
- **O(1) call-stack recursion.** Every self-recursive call must be in strict tail position (a compile-time-enforced rule); LLVM tail-call elimination turns it into a jump, so there is no runtime stack guard and no stack-overflow crash.
- **Explicit everything.** `val`/`var` (immutable/mutable), `pub` (visibility), `impure` (side effects/effect purity), `try` (Result/Option propagation), and casing itself (`snake_case`/`PascalCase`/`SCREAMING_SNAKE_CASE`) are all compiler-enforced grammar, not convention.
- **Static, freestanding binaries.** The compiler links every program statically against a staged musl libc — no dynamic linker dependency in the output binary.

See [`MANIFESTO.md`](MANIFESTO.md) for the full, normative specification and the full list of anti-principles (no macros, no operator overloading, no async, no trait objects, no GC, no exceptions).

## A taste of the language

```cinnabar
pub const DISKS: I64 = 8

pub type MoveCount
  pub moves: I64
end

fun hanoi_acc(n: I64, acc: I64) I64
  if n <= 0
    return acc
  end
  return hanoi_acc(n - 1, acc + acc + 1)
end

fun hanoi(n: I64) MoveCount
  return MoveCount(moves: hanoi_acc(n, 0))
end
```

Linear native handles must be consumed exactly once:

```cinnabar
fun print_int_inline(value: I64) impure Result(Unit, Collections.Error)
  val vec = try Collections.vec_new[U8]()
  try Collections.vec_push(&mut vec, digit_byte(value))
  Collections.vec_free(vec)          # linear handle consumed exactly once
  return Ok(Unit)
end
```

Pattern matching with array rest-patterns and traits:

```cinnabar
pub trait Checksum
  pub fun checksum(value: &Self) U32
end

fun split_first(view: &[U8]) Option(SplitFirst)
  match view
    [] => return None
    [first, rest @ ..] => return Some(SplitFirst(first: first, rest_len: slice_len(rest)))
  end
end
```

Multi-file modules are resolved automatically from `use` statements — `use Math.add` in `main.cnb` loads the sibling file `Math.cnb`:

```cinnabar
# main.cnb
use Math.add

pub fun main() I64
  return add(10, 20)
end
```

More real examples live in [`tests/fixtures/`](tests/fixtures/), especially [`tests/fixtures/spec.cnb`](tests/fixtures/spec.cnb) — the immutable reference implementation fixture, which doubles as an executable language tour.

## Building the compiler

Cinnabar targets **LLVM 21** (via the `inkwell` crate) and requires `clang`/`llc`/`opt` on `PATH`, plus a static musl libc to link Cinnabar binaries against. The project ships a Nix flake that provisions all of this:

```bash
nix develop
cargo build --release
```

Outside of `nix develop`, `cargo build`/`cargo clippy` will fail unless you have a matching LLVM 21 toolchain and `MUSL_LIBC_A` (pointing at a static `libc.a`) configured yourself — see [`build.rs`](build.rs) and [`flake.nix`](flake.nix) for the exact discovery logic and paths.

## Using the compiler

```
cinnabar <FILE> [-o|--output PATH] [--dump-ast] [--run] [-O|--opt-level {0,1,2,3,s,z}]
```

| Flag | Description |
|---|---|
| `<FILE>` | Input Cinnabar source file (positional, required), conventionally `.cnb` |
| `-o, --output <PATH>` | Output binary path (defaults to the input path with `.cnb` stripped) |
| `--dump-ast` | Parse only, pretty-print the AST, and exit (no resolve/typecheck/borrow-check/codegen) |
| `--run` | Execute the produced binary after a successful build and propagate its exit code |
| `-O, --opt-level <LEVEL>` | LLVM optimization level: `0`, `1`, `2`, `3`, `s`, `z` (default `2`) |

Examples:

```bash
cargo run -- tests/fixtures/spec.cnb                 # compiles spec.cnb -> tests/fixtures/spec
cargo run -- tests/fixtures/multi_file/main.cnb --run # compiles and runs, following `use Math.add`
cargo run -- my_program.cnb --dump-ast                # inspect the parsed AST
```

On success, the compiler prints `Successfully compiled <input> to '<output>'.` and exits `0`. Any lex, parse, resolve, typecheck, borrow-check, or codegen failure is rendered as one or more source-located diagnostics (via [`ariadne`](https://github.com/zesterer/ariadne)) and exits non-zero.

## Compiler architecture

Cinnabar is a single fixed pipeline:

```
lexer → parser → module_loader → resolver → typechecker → borrow_checker → codegen
```

Every stage computes its facts exactly once and attaches them to the program representation for later stages to read — nothing is silently re-derived downstream. See [`ARCHITECTURE.md`](ARCHITECTURE.md) for a full technical walkthrough of each stage, the compiler's unusual flat-array/arena internal representation, and the codegen/linking pipeline.

## Repository layout

```
src/
  main.rs           CLI driver, pipeline wiring, AST dumper
  lexer.rs          Hand-written byte-level lexer
  parser.rs         Recursive-descent parser
  ast.rs            Flat node-arena AST representation and opcode constants
  module_loader.rs  Multi-file module discovery/loading
  resolver.rs       Name resolution, scoping, casing enforcement
  typecheck.rs      Type checking, canonical type keys, linearity inference
  borrow.rs         Flow-sensitive borrow/linearity checker (CFG dataflow)
  codegen/          LLVM IR generation (via inkwell) and native linking
tests/
  fixtures/         .cnb example/regression programs (positive and EXPECT_REJECTED)
austral_refs/       Reference material from Austral (the language's direct influence)
MANIFESTO.md        Normative language specification
ROADMAP.md          Planned milestones and open work
AGENTS.md           Contribution/AI-agent working conventions for this repo
pre_commit_check.sh Build/lint/test/fixture verification gate
flake.nix           Nix dev shell (LLVM, clang, valgrind, etc.)
build.rs            Locates and stages a static musl libc for linking
```

## Verifying a change

The repository's build gate is [`pre_commit_check.sh`](pre_commit_check.sh), run inside the Nix dev shell:

```bash
nix develop --command ./pre_commit_check.sh
```

It runs `cargo check`, `cargo clippy -D warnings`, a custom Semgrep ruleset, `cargo test`, CLI smoke checks, compiles and `--dump-ast`s several fixtures, runs the compiled `spec.cnb` reference binary, and runs a battery of `EXPECT_REJECTED` negative fixtures (bad casing, immutable assignment, unknown variables, nested comments, etc.). See [`AGENTS.md`](AGENTS.md) for the full set of repository conventions this project holds itself to (no `unwrap`/`panic!`, no `_` discard bindings, no re-derived facts, category-level fixes only, etc.).

## Status

Cinnabar is under active early development. See [`ROADMAP.md`](ROADMAP.md) for what's resolved and what's planned next (the fixed-width integer suite, string literals, native OS surfaces, a project manifest format, diagnostic quality improvements, and formal verification work). Self-hosting — Cinnabar compiling itself — is a long-term goal and completeness test, not a gate for any individual feature.

## License

Apache-2.0 WITH LLVM-exception. See [`LICENSE`](LICENSE).
