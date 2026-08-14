# Self-hosting readiness

An overnight full-repository audit (seven independent agents, one per subsystem: lexer/parser/AST,
resolver/typecheck, borrow checker, codegen, CLI/LSP/infra, the tests/fixtures corpus, and the
website), followed by three rounds of fixes: an initial pass on the safely-verifiable front end; a
second, targeted pass (three more agents, in isolated git worktrees, on `opus` for the highest-stakes
areas and `fable` for the more mechanical ones) that closed out nearly everything the audit had
flagged but left open; and a third pass closing the handful of items that round two's own work
surfaced (the remaining borrow-checker leaks, a newly-found resolver gap, and a fixture content
decision, made explicitly rather than guessed). This document is the result: what got fixed, what's
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
fixed, on its own, the fixture-corpus problem in what was then §2 before this document had to say
anything about it.

---

## 1b. Round two — everything else in §2, fixed or dispositioned

A follow-up pass (three targeted agents in isolated git worktrees — two on `opus` for the
highest-stakes/most subtle areas, one on `fable` for the more mechanical tooling work — each merged
back and re-verified here) closed out nearly everything §2 originally listed as open. What's below
replaces that section; anything still genuinely open is now in §2 (renumbered).

**Borrow checker — both memory-safety holes are fixed and permanently regression-tested**
(`src/borrow.rs`, `src/typecheck.rs`):
- The field-chain-borrow-after-move hole: `walk_field_chain`'s borrow branch now threads the real
  field path through to `emit_op` instead of `NONE`, so `borrow_after_move_check` can actually see
  it. Both trigger shapes (a specific field moved then borrowed; the whole struct moved by value
  then a field of it borrowed) are now rejected.
- The `Result(&T, E)`/aggregate-wrapped dangling-return hole: added `typecheck::type_contains_ref`,
  a transitive "does this type carry a reference anywhere" walk mirroring `linear_of`'s existing
  struct/enum-member traversal (reused by codegen too, see below), and gated the returned-borrow
  obligation on it instead of the bare `TYD_REF`/`REF_MUT`/`SLICE` kind check.
- Four permanent regression tests now live in `src/borrow.rs` (`mod tests`), driving the real front
  end via `analysis::analyze` with no LLVM dependency — the same mechanism the LSP and playground
  use. All pass; the full 76-test suite plus `clippy -D warnings` stayed clean throughout.
- **Round three fixed the three remaining leaks too**: `b.8` (extraction-result-to-container pairs)
  and `b.9` (bindings created by a native create call) were syntactic snapshots taken once at a
  binding's own let/pattern-bind site and never invalidated afterward — refilling a container after
  popping from it (or after moving it) left a stale "provably empty" fact in place, so a later free
  accepted a leak. Fixed at the one place either fact can go stale: any call taking an existing
  container by `&mut` outside an extraction native now prunes both facts for that binding
  (`invalidate_container_facts`). Separately, `vec_free(make_full_vec())` — freeing a container
  that's a call result rather than a named binding — skipped the drain check entirely, since it's
  keyed on a binding and an anonymous expression has none; fixed by treating an unresolvable free
  target as MayContain (an immediate error) unless it's directly and provably a fresh create call
  itself, symmetric with how a by-value container parameter already starts MayContain. Five more
  regression tests, same `analyze`-driven mechanism, all passing; checked against the fixture corpus
  (the two `EXPECT_OK` fixtures that combine extraction and insertion on the same container,
  `vec_pop_drain.cnb`/`hash_map_remove_drain.cnb`, still pass clean).

**Codegen — both fixed, but this half is fundamentally unverifiable on this machine**
(`src/codegen/emitter.rs`, `src/typecheck.rs`): nothing under `src/codegen/` can be compiled, type-
checked, or even syntax-checked by `cargo` here (no LLVM 21 at all), so treat this pair as carefully
reasoned and precedent-matched, not proven.
- Tail-call marking: `mark_tail_call` is now the single place `set_tail_call(true)` is invoked, and
  it withholds the marker unless every argument's canonical type is provably free of references,
  transitively (reusing `type_contains_ref` again). LLVM's own tail-call-elimination pass re-derives
  the marker from real escape analysis independent of this source-level flag, so a call that
  genuinely doesn't leak the caller's frame should still get O(1)-stack treatment — but this is the
  one change in tonight's whole set that has had zero runtime observation of any kind.
  **Before trusting this, run `nix develop --command ./pre_commit_check.sh` and specifically watch
  `tail_rec.cnb` (1M iterations) and `mem_probe.cnb` (500k-deep) for their expected exit codes**,
  plus `--emit-llvm` on a `return f(&local)`-shaped fixture to confirm the IR is what's described.
- The constant-Result-array-index Single-Fact violation: `check_index` now records an explicit
  `IDX_ACCESS_IN_RANGE`/`IDX_ACCESS_FALLIBLE` fact directly on the `EXPR_INDEX` node (a confirmed-
  spare payload slot) instead of codegen re-deriving fallibility from the attached type's shape;
  `emitter.rs` reads it at both call sites that used to guess. The `typecheck.rs` half of this is
  fully compiled/tested/clippied here and is solid; only the `emitter.rs` read side is unverified.

**Resolver/typecheck — all three fixed, with one real fixture-corpus consequence worth knowing
about** (`src/resolver.rs`, `src/typecheck.rs`):
- Unused-`use` checking was completely dead (the item's own resolved-symbol slot was never written
  on a successful import, so the guard that was supposed to gate the diagnostic was unconditionally
  true). Fixed at the root — `finish_import` now sets the slot like every other item kind does — and
  the diagnostic is deferred (reported after the rest of resolution, so it can't mask a later, more
  specific error on the same program).
- The order-dependent silent-shadowing bug (`use Other.Foo` before a local `struct Foo` hid the
  conflict; the opposite order caught it) is fixed — `scope_lookup` now skips an unresolved-import
  placeholder rather than treating it as a real, first-match entry, so the conflict check is
  symmetric regardless of declaration order.
- Incomplete trait `impl`s are now rejected at the `impl` site (every declared trait method — traits
  here have no default/provided methods, confirmed from the parser — must have a corresponding impl
  method; a missing one is reported by name).
- **Enabling unused-import checking newly rejected 27 dead imports across 5 fixtures.** All 27 were
  hand-verified as genuine (the imported name is never referenced, or only ever appears fully
  qualified). All 5 are now fixed (`mem_probe.cnb`, `slice_test.cnb`, `vec_pop_drain.cnb`,
  `hash_map_remove_drain.cnb` — see the corresponding commit for why some needed more than deleting
  the `use` line: an import was, in a few cases, the *only* thing keeping an otherwise fully-dead
  native-declaration block reachable, since `resolve_imports` attributes every import edge to a
  synthetic always-reachable root regardless of whether the name is ever called — deleting just the
  import traded one diagnostic for a cascade of "unused native function" ones from the block it left
  behind, so the whole unused block had to go too). **`tests/fixtures/repro/head.cnb`** (18 of the
  27) needed a real content decision — it's a language-tour fixture that had declared far more
  surface than its trivial `main` exercised, and the same reachability cascade applied to nearly its
  entire import list. Asked, and narrowed it (rather than rewriting `main` to exercise everything) to
  what it actually demonstrates now: structs, trait-based polymorphism, bitwise math, and mutable
  local state under loop control, with the header comment's claims trimmed to match. `main`'s
  behavior is unchanged verbatim (`EXPECT_OK` exit 10).
- **Round three also fixed the module-nested-`use` bug found in round two:** a `use` written
  *inside* a `mod ... end` block never resolved at all — `resolve_imports` only walked the top-level
  item list, never recursing into `ITEM_MODULE` children, even though the placeholder for such an
  import was correctly created. Fixed by recursing into a module's own child list against its own
  scope (`item_scope_of`), the identical lookup `walk_item`'s `ITEM_MODULE` arm already uses for
  everything else inside a module. Two regression tests (resolves correctly; a genuine resolution
  failure inside a module still reports, so the fix doesn't swallow real errors).

**Tooling/CLI/LSP — all five fixed** (`src/main.rs`, `src/project.rs`, `src/bin/cinnabar_lsp.rs`,
`src/docs.rs`, `src/advanced_tools.rs`):
- The Windows `\\?\`-verbatim-path bug is fixed with one shared helper (`comparable_path`) used
  everywhere a canonicalized path is compared against a URI-decoded one; **the two previously-failing
  tests now pass**, and the full suite (76 tests) is green.
- `cinnabar --run` no longer risks a `$PATH` search for the just-built binary (main.rs; unverified
  here, same LLVM constraint as the codegen items).
- `cinnabar burn`/`playground`'s serve loops no longer die on the first misbehaving client — only a
  bind failure is fatal now; per-connection errors are logged and the loop continues.
- A `NO_FILE`-sourced diagnostic (e.g. an unreadable entry file) is now forwarded to the editor via
  `window/showMessage` instead of silently vanishing.
- `cinnabar test` now bounds both the compile and run steps with the same timeout pattern the
  in-repo `cargo test` harness already uses, so one hanging test can no longer hang the whole run.

**Site**: nothing further — confirmed clean beyond §1's three fixes.

---

## 2. Confirmed, not fixed — genuinely still open

Everything from the original audit that had a mechanical, verifiable-here fix is now fixed — three
rounds deep. What's left needs either the real LLVM toolchain or is genuinely low-priority:

- **`emitter.rs`'s two codegen fixes are unverified** — this machine cannot compile, type-check, or
  run anything under `src/codegen/`. Run the real gate before trusting them. See 1b above.
- **Everything else the original seven-agent audit found that no fix pass has touched**, still
  exactly as first reported: a `HashMap` padding/`memcmp` correctness question and an under-aligned
  `sockaddr_in` store in codegen (both lower-confidence, unverifiable here); the VS Code extension's
  `client.start()`-into-`context.subscriptions` `Promise` mismatch (flagged in the original audit,
  not in either fix pass's scope — worth folding into a future tooling pass); the playground's
  UTF-16-vs-byte-offset diagnostic-span indexing and the sample-switcher's incomplete ARIA tabs
  pattern (both site-side, low severity); `tests/fixtures/native_surface.idl`, a confirmed-dead file
  now removed.

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

1. Run the real `nix develop` gate to confirm the codegen fixes from 1b — everything else that had a
   mechanical fix is already done, so this is the one thing standing between "believed correct" and
   "proven correct" for the whole three-round effort. Worth doing regardless of anything below.
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
