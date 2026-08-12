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

Every snippet below is copied verbatim from the repository's own known-good fixture corpus in [`tests/fixtures/`](tests/fixtures/) — each is a complete, compiling program with a real `main`, not hand-assembled for this document. (This repo's toolchain requires LLVM 21 + a staged musl libc via `nix develop`, which isn't available in the environment these docs were written in, so "known-good, taken from the fixture corpus" stands in for "compiled and verified locally.")

Tail recursion and structs — [`tests/fixtures/repro/hanoi.cnb`](tests/fixtures/repro/hanoi.cnb):

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

fun hanoi_moves(disks: I64) I64
  return hanoi_acc(disks, 0)
end

fun hanoi(n: I64) MoveCount
  return MoveCount(moves: hanoi_moves(n))
end

pub fun main() I64
  val result = hanoi(DISKS)
  return result.moves
end
```

Linear native handles, generics, and `Result` — [`tests/fixtures/repro/vec_test.cnb`](tests/fixtures/repro/vec_test.cnb):

```cinnabar
pub mod Collections
  pub nat type Vec(T)
  pub nat type String
  pub nat type HashMap(K, V)

  pub type Error
    pub AllocationFailed(Usize)
    pub IndexOutOfBounds(Usize)
    pub KeyNotFound
    pub EmptySlice
    pub InvalidUtf8
  end

  pub nat fun vec_new<T>() impure Result(Vec(T), Error)
  pub nat fun vec_push<T>(vec: &mut Vec(T), value: T) impure Result(Unit, Error)
  pub nat fun vec_view<T>(vec: &Vec(T)) &[T]
  pub nat fun vec_free<T>(vec: Vec(T)) impure Unit
  pub nat fun string_from_slice(view: &[U8]) impure Result(String, Error)
  pub nat fun string_len(value: &String) Usize
  pub nat fun string_free(value: String) impure Unit
  pub nat fun hash_map_new<K, V>() impure Result(HashMap(K, V), Error)
  pub nat fun hash_map_insert<K, V>(map: &mut HashMap(K, V), key: K, value: V) impure Result(Unit, Error)
  pub nat fun hash_map_get<K, V>(map: &HashMap(K, V), key: K) impure Result(V, Error)
  pub nat fun hash_map_free<K, V>(map: HashMap(K, V)) impure Unit
end

use Collections.vec_new
use Collections.vec_push
use Collections.vec_view
use Collections.vec_free

pub mod Slice
  pub nat fun len<T>(view: &[T]) Usize
end

use Slice.len as slice_len

const BAD_NEW: I64 = 1
const BAD_PUSH: I64 = 2

fun fail_vec<T>(vec: Collections.Vec(T)) impure I64
  vec_free(vec)
  return BAD_PUSH
end

fun fill_squares(vec: &mut Collections.Vec(I64)) impure Result(Unit, Collections.Error)
  var i: I64 = 0
  while i < 5
    try vec_push(vec, i * i)
    i = i + 1
  end
  return Ok(Unit)
end

pub fun main() impure I64
  val vec = match vec_new[I64]()
    Ok(v) => v
    Err(error) => return BAD_NEW
  end

  val fill_result = fill_squares(&mut vec)
  match fill_result
    Ok(Unit) => Unit
    Err(error) => return fail_vec(vec)
  end

  val view = vec_view(&vec)
  val n = slice_len(view)
  vec_free(vec)          # linear handle consumed exactly once
  return 0
end
```

Slices, array rest-patterns, and tail-recursive folds — [`tests/fixtures/repro/slice_test.cnb`](tests/fixtures/repro/slice_test.cnb):

```cinnabar
pub mod Slice
  pub nat fun len<T>(view: &[T]) Usize
end

use Slice.len as slice_len

fun slice_sum_acc(view: &[U8], acc: Usize) Usize
  match view
    [] => return acc
    [first, rest @ ..] => return slice_sum_acc(rest, acc + Usize.from(first))
  end
end

fun slice_sum(view: &[U8]) Usize
  return slice_sum_acc(view, 0)
end

pub const MAGIC_BYTE_0: U8 = 0x0D
pub const MAGIC_BYTE_1: U8 = 0xF0
pub const MAGIC_BYTE_2: U8 = 0xAD
pub const MAGIC_BYTE_3: U8 = 0x0B
pub const EXPECTED_SUM: Usize = 437

fun array_as_slice() Usize
  val bytes: [U8; 4] = [MAGIC_BYTE_0, MAGIC_BYTE_1, MAGIC_BYTE_2, MAGIC_BYTE_3]
  return slice_sum(&bytes)
end

pub fun main() I64
  if array_as_slice() == EXPECTED_SUM
    return 0
  end
  return 1
end
```

Multi-file modules, resolved automatically from `use` statements — [`tests/fixtures/multi_file/`](tests/fixtures/multi_file/), where `use Math.add` in `main.cnb` loads the sibling file `Math.cnb`:

```cinnabar
# main.cnb
use Math.add

pub fun main() I64
  return add(10, 20)
end
```

```cinnabar
# Math.cnb
pub fun add(a: I64, b: I64) I64
  return a + b
end
```

More real examples live in [`tests/fixtures/`](tests/fixtures/), especially [`tests/fixtures/spec.cnb`](tests/fixtures/spec.cnb) — the immutable reference implementation fixture, which doubles as an executable language tour (traits, `impl`, checksum-style dispatch, and more).

## Building the compiler

Cinnabar targets **LLVM 21** (via the `inkwell` crate) and requires `clang`/`llc`/`opt` on `PATH`, plus a static musl libc to link Cinnabar binaries against. The project ships a Nix flake that provisions all of this:

```bash
nix develop
cargo build --release
```

Outside of `nix develop`, `cargo build`/`cargo clippy` will fail unless you have a matching LLVM 21 toolchain and `MUSL_LIBC_A` (pointing at a static `libc.a`) configured yourself — see [`build.rs`](build.rs) and [`flake.nix`](flake.nix) for the exact discovery logic and paths.

### Docker Desktop and Windows worktrees

Windows contributors can run the same Nix environment in one reusable Docker Compose service. Its named Nix and Cargo caches survive branch changes, while every worktree receives an isolated Rust `target` volume. The setup also includes the linked-worktree Git mounts needed by Nix and a rust-analyzer wrapper that runs inside `nix develop`.

See [`CONTAINER_DEVELOPMENT.md`](CONTAINER_DEVELOPMENT.md) for setup, VS Code attachment, worktree switching, and verification commands. Native Linux development remains Nix-first and does not require Docker.

## Using the compiler

```
cinnabar <FILE> [-o|--output PATH] [--dump-ast] [--dump-typed-ast] [--print-layout]
                [--emit-llvm] [--emit-obj] [--explain-borrow] [--run]
                [-O|--opt-level {0,1,2,3,s,z}]
```

| Flag | Description |
|---|---|
| `<FILE>` | Input Cinnabar source file (positional, required), conventionally `.cnb` |
| `-o, --output <PATH>` | Output binary path (defaults to the input path with `.cnb` stripped) |
| `--dump-ast` | Parse only, pretty-print the AST, and exit (no resolve/typecheck/borrow-check/codegen) |
| `--dump-typed-ast` | Run the full front-end, then print the node arena with every attached fact (resolved symbols, canonical type keys, linearity flags, variant tags, field facts) and exit |
| `--print-layout` | Run the full front-end, then print ABI size, alignment, field offsets, and enum variant tags for every concrete struct/enum/native handle and exit |
| `--emit-llvm` | Write the emitter's LLVM IR (before optimization) to the input path with `.ll` and stop |
| `--emit-obj` | Optimize and assemble to a relocatable object at the input path with `.o`, skipping the static link |
| `--explain-borrow` | Attach secondary labels to borrow/linearity errors: which paths consume a value, where it was bound (and its linear type), where it was previously moved |
| `--run` | Execute the produced binary after a successful build and propagate its exit code |
| `-O, --opt-level <LEVEL>` | LLVM optimization level: `0`, `1`, `2`, `3`, `s`, `z` (default `2`) |

Examples:

```bash
cargo run -- tests/fixtures/spec.cnb                 # compiles spec.cnb -> tests/fixtures/spec
cargo run -- tests/fixtures/multi_file/main.cnb --run # compiles and runs, following `use Math.add`
cargo run -- my_program.cnb --dump-ast                # inspect the parsed AST
```

On success, the compiler prints `Successfully compiled <input> to '<output>'.` and exits `0`. Any lex, parse, resolve, typecheck, borrow-check, or codegen failure is rendered as one or more source-located diagnostics (via [`ariadne`](https://github.com/zesterer/ariadne)) and exits non-zero.

## Language server

The repository also builds `cinnabar-lsp`, a Language Server Protocol server over the same compiler pipeline:

```bash
cargo build --release --bin cinnabar-lsp
```

It speaks stdio and provides diagnostics (with the borrow checker's explanatory notes as related information and code lenses), hover (attached types and signatures, linearity), go-to-definition and find-references across the module graph, completion (resolver-visible symbols and `use` paths, lexically scoped locals, struct fields after `.`, enum variants, keywords), and signature help. Full front-end checks are debounced after edits and run off the protocol loop; generation checks prevent superseded results from being published. Point any LSP client at the binary for `.cnb` files — e.g. in VS Code via a generic LSP extension, or in Neovim:

```lua
vim.lsp.start({ name = "cinnabar", cmd = { "/path/to/cinnabar-lsp" }, root_dir = vim.fn.getcwd() })
```

Every answer is read from the facts the pipeline attaches (resolved symbol ids, canonical type keys); the server contains no second implementation of name resolution or type inference.

## Compiler architecture

Cinnabar is a single fixed pipeline:

```
lexer → parser → module_loader → resolver → typechecker → borrow_checker → codegen
```

Every stage computes its facts exactly once and attaches them to the program representation for later stages to read — nothing is silently re-derived downstream. See [`ARCHITECTURE.md`](ARCHITECTURE.md) for a full technical walkthrough of each stage, the compiler's unusual flat-array/arena internal representation, and the codegen/linking pipeline.

## Repository layout

```
src/
  lib.rs            Library crate exposing the pipeline to the CLI and tooling
  main.rs           CLI driver, pipeline wiring, AST dumper
  bin/cinnabar_lsp.rs  Language server (JSON-RPC shell over analysis.rs)
  lexer.rs          Hand-written byte-level lexer
  parser.rs         Recursive-descent parser
  ast.rs            Flat node-arena AST representation and opcode constants
  module_loader.rs  Multi-file module discovery/loading (with editor-buffer overlay)
  resolver.rs       Name resolution, scoping, casing enforcement
  typecheck.rs      Type checking, canonical type keys, linearity inference
  borrow.rs         Flow-sensitive borrow/linearity checker (CFG dataflow, explainer notes)
  analysis.rs       IDE queries over attached facts (hover, definition, references, ...)
  inspect.rs        --dump-typed-ast arena serialization
  codegen/          LLVM IR generation (via inkwell), layout report, native linking
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

### Faster local test profiles

The default `full` profile preserves exhaustive gate coverage. For quicker local feedback, `balanced` and `smoke` reduce the randomized corpus sizes and the number of successful fixtures that are linked and executed. Successful cases not selected for execution still pass through parsing, resolution, typechecking, borrow checking, code generation, and LLVM IR emission; the reduced profiles mainly avoid repeated `llc` and static-link work. Rejected fixtures remain checked.

| Profile | Fuzz corpus | Native fuzz runs | Native expected-fixture runs | Record-only runs |
| --- | ---: | ---: | ---: | ---: |
| `full` | 80 valid + 80 invalid | all 80 valid cases | all | all |
| `balanced` | 32 valid + 32 invalid | 8 | 10 | 2 |
| `smoke` | 8 valid + 8 invalid | 2 | 4 | 0 |

```bash
# Full coverage (the default)
nix develop --command cargo test --quiet

# Routine local iteration
nix develop --command cargo test --quiet --features test-profile-balanced

# Fastest structural feedback
nix develop --command cargo test --quiet --features test-profile-smoke
```

Individual budgets can be overridden when a reduced profile is still broader or narrower than needed. The full profile ignores these variables, so an exported local override cannot silently reduce `pre_commit_check.sh` coverage:

| Environment variable | Controls |
| --- | --- |
| `CINNABAR_FUZZ_POSITIVE_CASES` | Generated valid programs compiled |
| `CINNABAR_FUZZ_NEGATIVE_CASES` | Generated invalid linearity programs rejected |
| `CINNABAR_FUZZ_RUN_CASES` | Valid fuzz programs additionally linked and executed |
| `CINNABAR_REPRO_RUN_CASES` | Expected-success fixtures additionally linked and executed |
| `CINNABAR_REPRO_RECORD_CASES` | Record-only fixtures compiled and run |
| `CINNABAR_REPRO_LINK_COMPILE_ONLY` | Whether blocking compile-only fixtures are linked (`true`) instead of stopping at LLVM IR (`false`) |
| `CINNABAR_TEST_RUN_TIMEOUT_SECS` | Per-program execution timeout |
| `CINNABAR_TEST_COMPILE_TIMEOUT_SECS` | Per-program fuzz compilation timeout |

Case budgets use an even sample across each ordered corpus instead of taking only its first entries. These controls are intended for local iteration; run `nix develop --command ./pre_commit_check.sh` with no profile override before submitting a change.

## Status

Cinnabar is under active early development. See [`ROADMAP.md`](ROADMAP.md) for what's resolved and what's planned next (the fixed-width integer suite, string literals, native OS surfaces, a project manifest format, diagnostic quality improvements, and formal verification work). Self-hosting — Cinnabar compiling itself — is a long-term goal and completeness test, not a gate for any individual feature.

## License

Apache-2.0 WITH LLVM-exception. See [`LICENSE`](LICENSE).
