# Self-hosting readiness

An overnight full-repository audit (seven independent agents, one per subsystem: lexer/parser/AST,
resolver/typecheck, borrow checker, codegen, CLI/LSP/infra, the tests/fixtures corpus, and the
website) plus manual synthesis and fixing. This document is the result: what got fixed, what's
still broken, and — the thing actually asked for — what stands between here and ROADMAP.md's
"Self-Hosting" goal (Cinnabar compiling itself).

Scope note on verification: this machine has `cargo`/`rustc` but no LLVM 21 and no Nix devshell, so
`cargo build`/`cargo check` with the default `codegen` feature fails immediately (`llvm-sys` can't
find LLVM) and `pre_commit_check.sh` cannot run at all here. Everything below that touches
`src/codegen/*` or `src/main.rs` is **read-only analysis**, not a compiled-and-tested fix. Everything
that touches the front end (`lexer.rs` through `borrow.rs`, `resolver.rs`, `analysis.rs`,
`cinnabar-lsp`) *was* compiled and tested here, via `cargo check --no-default-features` / `cargo
test --no-default-features --lib` / `cargo clippy --no-default-features -- -D warnings` — the
`codegen` feature (and its LLVM dependency) is off in that mode, exactly the way
`crates/cinnabar-wasm` already builds it. **Run the real gate before merging anything below**:
`nix develop --command ./pre_commit_check.sh`.

---

## 1. What changed tonight

Six fixes landed in the compiler front end, verified compiling, all 54 pre-existing unit tests
still passing (2 unrelated pre-existing failures, see §2), and zero new `cargo clippy -D warnings`
findings:

1. **`src/parser.rs` — `accept`/`expect` could consume a string literal as a keyword or piece of
   punctuation.** `tok_text_is` matched token *text* only, and text is interned in the same table as
   string-literal and doc-comment bodies. `f(a "," b)` parsed as `f(a, b)` with the string silently
   swallowed; `val x "=" 5` parsed as `val x = 5`. Fixed by requiring the token *kind* implied by the
   expected text (an all-letter keyword must be `TOK_IDENT`, punctuation must be `TOK_SYM`) in
   addition to the text match.
2. **`src/parser.rs` — removed a duplicate `list_first`/`list_len`** that shadowed the identical
   `pub` versions already glob-imported from `ast.rs`. Two implementations of one fact, exactly the
   shape `AGENTS.md` bans.
3. **`src/lexer.rs` — non-ASCII source produced one cascading error per byte with spans that split
   the character.** `report_unexpected` advanced and spanned by exactly one byte regardless of the
   character's real width; a two-byte character produced two overlapping "unexpected character"
   errors, the second one starting mid-character. Fixed to consume/span the whole `char`.
4. **`src/lexer.rs` — integer/hex-literal diagnostics excluded the offending byte.** `invalid
   character in integer literal`, `invalid digit in hexadecimal literal`, and the two
   too-large-literal messages all spanned one byte short of the character that actually triggered
   them. Fixed.
5. **`src/resolver.rs` — imported constants and enum variants were checked against the wrong casing
   rule.** `finish_import` applied `PascalCase` to every type-namespace import and `snake_case` to
   every value-namespace import — but the value namespace also holds `SCREAMING_SNAKE_CASE`
   constants and `PascalCase` variants. `use Config.MAX_LEN` or `use Colors.Red` were rejected with a
   spurious casing error. Fixed by deriving the expected casing from the imported symbol's own kind.
6. **`src/typecheck.rs` — `&&`/`||` constant-folding didn't require `Bool` operands.**
   `fold_bin`'s `BIN_AND`/`BIN_OR` arms folded any two integers through bitwise `&`/`|` before
   checking their type, so `const C: Bool = 1 && 2` silently accepted and folded to `false` while the
   identical `val c: Bool = 1 && 2` was correctly rejected by `check_binary` — the same source text
   typing two different ways depending on whether it's a `const` or a `var`, which is precisely the
   class of bug the Milestone 1 literal-typing rule exists to rule out. Fixed to apply the same
   `is_bool_key` gate the runtime path already uses.
7. **`src/typecheck.rs` — unary `-` let a typed (non-literal) operand implicitly adopt the expected
   width.** `check_unary`'s `UN_NEG` arm adopted the position's expected type whenever it was an
   integer type, with no check that the operand was actually a bare literal — so `val b: I8 = -a`
   with `a: U8` compiled, silently reinterpreting a `U8` value as `I8`. Fixed to gate the
   expected-width adoption on `int_literal_expr(operand)`, the same "is this actually an untyped
   literal" predicate every other operator already uses. As a side effect this also fixed the
   companion bug where negating an *unsigned* non-literal value produced no diagnostic at all unless
   an expected type happened to be present in context.

Each of these was checked against the fixture corpus (`grep`, since nothing here can be executed) to
confirm no existing accepted fixture relies on the old behavior; none do. Fix 7 in particular:
no fixture negates anything but a bare integer literal.

Three fixes landed in the website (`site/`), verified with `npx tsc --noEmit`, `npx eslint`, and
`npx vitest run` (220/220 passing):

8. **`site/src/lib/cinnabar-wasm-client.ts` — a failed wasm load was cached forever.**
   `checkerPromise` was assigned before the load settled and never cleared on rejection, so one
   transient failure (flaky network, an ad-blocker, a captive portal) permanently broke every future
   check until a full page reload — directly contradicting the "close this and try again" recovery
   copy elsewhere in the UI. Fixed: the promise is cleared on rejection so the next call retries.
9. **`site/src/components/PlaygroundDiagnostics.tsx` — a checker failure rendered nothing.** When
   `checkSource` rejected, the resulting report had empty `diagnostics` and a `serialization_error`
   string that nothing ever displayed — the panel just showed an empty `<pre>`. Fixed: a
   `serialization_error` with no diagnostics now renders as an error line.
10. **`site/src/components/PlaygroundEditor.tsx` — every failure was mislabeled "the checker failed
    to load".** Fixed to surface the real error message, which also stops conflating a load failure
    with a later failure inside an already-loaded checker.
11. **`site/src/lib/constants.ts` — removed three unreferenced exports** (`SECTION_PADDING`,
    `MEASURE`, `SPLIT`), confirmed dead by grep across `src/` and `tests/`.

Fixes 8–10 sit in files you already had uncommitted work in (the WASM playground). They're left
**uncommitted**, sitting in your working tree alongside that work, rather than committed over it —
review them as part of your own diff. Everything else (fixes 1–7, 11, and this document) is
committed separately with paths scoped to exactly those files.

One thing worth knowing about even though it's not a bug: partway through this session `git status`
showed a clean fast-forward pull landed on `main` from `origin` (one commit, "add native slice test
fixture") — nobody here ran `git pull`; it just happened between turns. It's relevant because it
fixed, on its own, the fixture-corpus problem in the next section before this document had to say
anything about it.

---

## 2. Confirmed, not fixed here — do these next

Ranked by how directly they threaten correctness, independent of self-hosting.

### 2.1 Borrow checker — two real memory-safety holes

The borrow checker is the single highest-value place in the compiler for a bug (a hole here means a
binary the compiler calls sound but isn't), so these get top billing even though fixing them safely
needs the real toolchain and fixture suite this machine doesn't have.

- **Field-path borrows skip the use-after-move check entirely.** `walk_field_chain` emits every
  dotted borrow (`&s.b`) with `path = NONE` (`src/borrow.rs:1318`), and `borrow_after_move_check`
  returns immediately on `path < 0` (`src/borrow.rs:2580-2589`). The Milestone-4 fix that made
  `OP_BORROW`/`OP_BORROW_M` check moved-state only ever applied to whole-binding borrows —
  `borrow_after_move.cnb` pins exactly those, no field-chain shape. Concretely: move `s.b` (e.g.
  `deallocate(s.b)`), then `Memory.read_u8(&s.b, 0)` — accepted, reads freed memory.
- **`Result(&T, E)` / ref-in-struct returns bypass the ambiguous-returned-borrow rule entirely.**
  The returned-borrow obligation is emitted only when the *declared return type itself* is
  `TYD_REF`/`REF_MUT`/`SLICE` (`src/borrow.rs:934-940`, `:3291-3295`); a function returning
  `Result(&T, E)` or a struct with a reference field emits no obligation at all, so returning a
  borrow of a local wrapped in either is never checked — a dangling reference the caller can freely
  read.

Three more (linear-element leaks from stale container-emptiness facts, and one from an unresolved
free target) are documented in full, with exact trigger shapes, in the transcript of the
borrow-checker audit agent — ask for it if you want the complete writeup; it's long enough that
inlining all five here would bury the two above.

### 2.2 Codegen — unverifiable here, two look real

Nothing in `src/codegen/` compiles on this machine at all (not even a syntax check — `llvm-sys`
fails before reaching it), so treat these as leads, not diffs to apply blind:

- **`tail` is marked on every call in tail position with no check on what the arguments carry.**
  `emit_call` (`src/codegen/emitter.rs:1889-1908`) sets LLVM's `tail` marker — a promise that the
  callee never touches the caller's frame — on any tail call, including `return f(&x)` where `x` is
  a local. That's IR whose behavior is undefined at `-O2`, and it directly contradicts the
  Milestone 8 "no UB-shaped IR" claim. Needs `--emit-llvm` on a two-line fixture in `nix develop` to
  confirm, then a fix scoped to "no argument's type can carry a frame pointer" (or a stated,
  enforced rule for the genuinely hard case: a self-tail-call passing a borrow of the caller's own
  local, which can't simultaneously reuse the frame *and* keep the borrow valid).
- **A constant index into an array of `Result` elements takes the fallible-index codegen path.**
  `typecheck.rs` attaches the bare element type to a proven-in-range constant index
  (`typecheck.rs:2960-2968`), but `emitter.rs:1819-1822` decides fallibility from the *shape* of the
  attached type rather than from a fact typecheck already computed — so `[Result(T, E); N]` indexed
  by a constant either hits an internal-error diagnostic (if `E != IndexError`) or silently
  re-wraps and copies the wrong byte range as if the element were `Result(T, IndexError)`. This is a
  Single-Fact Rule violation with a real payload-corruption consequence; fix by attaching an explicit
  fallible-index fact at typecheck instead of re-deriving it in codegen.

Full detail (plus a `HashMap` padding/`memcmp` issue and an under-aligned `sockaddr_in` store, both
lower-confidence) is in the codegen audit transcript.

### 2.3 Windows-specific — confirmed live on this machine

- **The LSP is broken on Windows for any real project.** `project::load_manifest` canonicalizes
  paths (`src/project.rs:83,170`), and on Windows `fs::canonicalize` returns
  `\\?\C:\...`-prefixed paths — which then never equal the plain paths URIs decode to
  (`uri_to_path`, `cinnabar_lsp.rs:1220-1240`). Consequence: the unsaved-editor-buffer overlay never
  matches, and hover/go-to-definition/find-references/completion all silently return nothing for
  project files. **This isn't hypothetical** — it's independently confirmed by two now-failing unit
  tests surfaced while verifying tonight's fixes:
  `project::tests::cinnabar_manifest_parses_folded_string_fields` and
  `project::tests::omitted_tests_field_uses_tests_directory` (`cargo test --no-default-features
  --lib`), both asserting a canonicalized path equals a plain one and both failing with exactly the
  `\\?\` mismatch described above. Confirmed pre-existing (fails identically with tonight's other
  changes stashed out). Fix at one boundary: strip the verbatim prefix and case-fold the drive
  letter wherever a filesystem path and a URI-derived path are compared.
- **`cinnabar file.cnb --run` with a bare output name runs via PATH lookup, not the binary just
  built.** `default_out_path` strips `.cnb` with no leading `./` (`main.rs:1309-1315`), and
  `Command::new(path)` on a separator-less path is a PATH search on Unix (masked on Windows, where
  the cwd is searched first) — so `--run` can execute a *different* `main` if one happens to be on
  `PATH`.

### 2.4 Everything else confirmed, by area (not re-verified here, not applied)

- **Resolver/typecheck**: unused-`use` checking is entirely dead (`resolver.rs:1979-1982` early-returns
  before it can ever fire — not applied tonight because turning it on could newly reject anything in
  the fixture corpus that has a genuinely-unused import today, and that needs the real test suite to
  check); a `use`-imported type can silently shadow an earlier same-name local type depending on
  source order; incomplete trait `impl`s are accepted until a missing method is actually called.
- **Tooling/CLI/LSP**: `cinnabar burn`/`playground`'s serve loops die on the first client that closes
  a connection mid-write instead of logging and continuing; a `NO_FILE`-sourced diagnostic (e.g. the
  entry file itself being unreadable) is silently dropped by the LSP instead of surfaced; the VS
  Code extension pushes `client.start()`'s `Promise<void>` into `context.subscriptions`, which throws
  on deactivate under `vscode-languageclient` ^9; `cinnabar test` has no per-test timeout, so one
  hanging test hangs the whole run (the in-repo `cargo test` harness has one, the user-facing runner
  doesn't).
- **Site**: nothing further beyond §1's fixes — the audit came back clean on the rest of the
  finished-looking surface (all routes resolve, no orphaned components, a11y is mostly solid). Two
  worth knowing about without fixing: diagnostic spans in the playground are indexed as UTF-16 code
  units against UTF-8 byte offsets (display-only corruption, only reachable with non-ASCII source);
  the sample-switcher tabs use `role="tablist"`/`role="tab"` without the rest of the ARIA tabs
  contract (no arrow-key nav, no `aria-controls`).
- **Tests/fixtures corpus**: now clean. The one real gap (`native_slice_view.cnb` registered
  `EXPECT_OK` in three suites with no file on disk, and no commit ever adding one) was fixed by the
  fast-forward pull mentioned in §1, before this document needed to ask for it. One genuinely dead
  file remains: `tests/fixtures/native_surface.idl` is referenced nowhere — every `native-stub` test
  generates or inlines its own IDL instead.

---

## 3. Self-hosting: what "ready" actually requires

ROADMAP.md is explicit that self-hosting is "a goal and a completeness test, not the gate for any
feature" — so this is a gap analysis, not a blocker list. Two independent kinds of gap, and they
don't share a fix.

### 3.1 The pervasive one: non-tail recursion, everywhere

The Crucible Rule (self-recursive calls must be in strict tail position) is enforced today, which
means it would reject large parts of the compiler's *own* implementation if that implementation were
rewritten in Cinnabar as-is. Every one of the seven subsystem audits flagged the same shape
independently:

- **Parser**: `parse_expr → parse_binary → parse_unary → parse_postfix → parse_primary → parse_expr`
  and the sibling chains for types/patterns/items — deep mutual recursion, not tail.
- **Resolver/typecheck**: `walk_expr`/`walk_stmt`, `fold_const`, `canon_ty`, `subst_key`/`subst_list`,
  `unify_key`, `linear_of` — same shape.
- **Borrow checker**: `expr_effects`/`call_effects`/`match_effects` (mutual, fine under the rule as
  currently interpreted) but `origin_owners_of`, `collect_origin_loans`, `trace_origin`,
  `create_provenance`, `materialize_linear_subpaths` are non-tail *self*-recursive — outright compile
  errors under today's rule.
- **Codegen**: `emit_expr`/`emit_stmt`/`emit_pattern`/`get_or_emit_fn`/`llvm_type` — the same tree-walk
  shape, over ~4,500 lines in `emitter.rs` alone.

None of this is a soundness problem in the *current* Rust implementation — it's the largest single
mechanical cost of ever porting it, bigger than any individual missing native surface. Every one of
these needs an explicit `Vec`-backed work-stack rewrite (turning the call stack into an owned data
structure the algorithm manages itself) before it could be re-expressed in Cinnabar. The good news,
independently confirmed by multiple audits: the *data* shapes underneath — the flat arena, `Vec`-of-
`Vec` adjacency lists, struct-of-arrays state — are already exactly Cinnabar-shaped and need no
rework. This is a mechanical, bounded, well-understood cost, not an open design question. It's also
the kind of change that's straightforward to do incrementally, stage by stage, well before a full
self-hosted compiler is attempted.

### 3.2 The native-surface gaps

What the current `Memory`/`Terminal`/`File`/`Net` surface doesn't cover, needed by different parts of
today's Rust implementation, roughly ordered by how many things depend on it:

1. **Process spawning.** The single biggest concrete gap. `cinnabar test`, `mushlings verify`, `fuzz
   replay`/`minimize`, the playground, and `inspect` all shell out to the compiler itself or to built
   binaries, capturing stdout/stderr/exit status — and codegen's own final step shells out to
   `opt`/`llc`/`clang`. Nothing in the native surface exposes fork/exec/wait or pipes. Fits the
   existing `syscall.rs` pattern (a handful of rows per architecture for
   `fork`/`execve`/`wait4`/`clone`), but `execve` needs a way to build a NUL-terminated pointer array
   for `argv`/`envp`, which today's `Memory.Block` (byte-at-a-time `write_u8`) can't construct — a
   pointer-slot write primitive or a dedicated argv-building native is a prerequisite.
2. **Directory enumeration, `stat`, and `realpath`.** `cinnabar test`'s recursive test-file walk, and
   — load-bearing — the Milestone 5 path-confinement checks that make `build.cnb`'s
   `ENTRY`/`TESTS`/sidecar paths safe all depend on `fs::canonicalize`. `File` today is
   open/read/write/close only.
3. **A dynamic string/formatting surface.** Every diagnostic, every generated HTML page, every JSON
   payload in the current implementation leans on `format!` and growable strings.
   `Collections.String` exists; integer-to-decimal/hex rendering, join, and split don't yet.
4. **Threads, channels, and a monotonic clock.** The LSP's debounce/staleness protocol (a worker
   thread, a channel, `Instant` deadlines) and the playground's wall-clock timeout both need this.
   Cinnabar has no async and no concurrency story today by design — this is a real design question
   the compiler-in-Cinnabar effort will have to answer (native threads vs. a deliberately
   single-threaded LSP redesign), not just a missing native.
5. **Environment variables and temp-file/asset staging.** `build.rs`'s musl-libc discovery
   (`MUSL_LIBC_A`, `/nix/store` search) and the embedded `include_bytes!` of `libc.a`/crt objects have
   no Cinnabar-side equivalent yet; the pragmatic answer is shipping those archives beside the binary
   and opening them via `File` plus a way to locate the running executable (`/proc/self/exe`, i.e. one
   more `readlinkat`-shaped native), not a compile-time embedding story.
6. **Not a gap**: HTTP. `cinnabar burn`/`playground` are already hand-rolled over raw sockets, and
   `Net`'s BSD-socket surface plus a string/formatting surface would carry them as-is. JSON is the
   same story — a JSON encoder/decoder is pure computation over byte slices, entirely writable in
   Cinnabar today, no native surface needed.

### 3.3 The hard part: codegen's dependency on LLVM's C++ API

This is the one piece of the pipeline that can't be closed by adding native surfaces in the pattern
above, because Cinnabar has no general FFI — `nat` is a fixed, compiler-known opcode set
(`dispatch_native` in `emitter.rs`), not a user-extensible binding surface, and the language bans
user-visible pointers by design. In-process LLVM (via `inkwell`) is used today for three genuinely
different things, and they don't all cost the same to replace:

- **Printing IR and shelling out** (`module.print_to_string()` then `opt`/`llc`/`clang` as
  subprocesses) — cheap to replace once process-spawning exists (§3.2.1): build the `.ll` text
  directly as a `Vec(U8)` and write it with `File`, then hand it to the same external tools.
- **Module verification** — recoverable as `opt -passes=verify` through the same subprocess path
  covered above.
- **`TargetData` ABI queries** (`get_abi_size`/`get_abi_alignment`/`offset_of_element`) — the one
  genuinely load-bearing use, driving enum payload layout, `memcpy` sizes, container element strides,
  and the entire `--print-layout` report. This is *not* an FFI problem: for the two blessed triples
  (`x86_64`/`aarch64` Linux, both LP64), the ABI layout algorithm is closed and well-known (sizes
  1/2/4/8, alignment = min(size, 8), standard struct rounding — `codegen/types.rs`'s existing
  `round_up` is already half of it) and can simply be computed directly in Cinnabar, removing the
  last load-bearing reason to touch LLVM's C++ API at all.

So the realistic path for a self-hosted codegen is **not** "bind LLVM from Cinnabar" — it's "emit
`.ll` text and compute the two target layouts directly," at which point `opt`/`llc`/`clang` stay
exactly what they are today: external tools invoked as subprocesses, the same relationship the
current Rust implementation already has with them. The embedded-musl-libc boundary and clang-as-linker
stay permanently native regardless (this matches the "marked-native boundary forever" language
`ROADMAP.md` already uses for comparable cases).

### 3.4 Suggested ordering

Roughly cheapest-and-most-widely-useful first, each step independently valuable regardless of
whether the ones after it ever happen:

1. Fix the confirmed bugs in §2 — several of them (the borrow-checker holes especially) are real
   soundness problems today, orthogonal to self-hosting, and shouldn't wait on it.
2. Add the `Process` native surface (§3.2.1) plus the `File` extensions (§3.2.2) — both are useful to
   ordinary Cinnabar programs immediately, not just to a future self-hosted compiler.
3. Add a string-formatting surface (§3.2.3) — same argument, broadly useful on its own.
4. Start the non-tail-recursion rewrites (§3.1) stage by stage, starting with the smallest/most
   self-contained (the parser is more tractable than `emitter.rs`), each one a real, independently
   shippable improvement to the Rust implementation's own structure (explicit work stacks are
   generally easier to reason about than deep mutual recursion anyway) and *also* progress toward a
   self-hostable version of that stage.
5. Prototype textual `.ll` emission plus the self-computed target-data calculation (§3.3) for a
   throwaway single-function case, to validate the approach before committing to it for the whole
   backend.
6. Only after 2-5 are individually solid: attempt an actual line-by-line port of a pipeline stage,
   starting with the lexer (smallest, most self-contained, least mutual recursion) as the real proof
   of concept.
7. Concurrency/threading for the LSP (§3.2.4) can be deferred indefinitely — it only matters if the
   LSP itself is ever part of what gets self-hosted, which ROADMAP.md doesn't require.

None of the above needs to happen before any other planned milestone; per ROADMAP.md's own framing,
this is a completeness test to run once the language is complete enough, not a precondition for
anything else on the roadmap.
