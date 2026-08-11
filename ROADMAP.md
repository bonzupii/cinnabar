# Cinnabar — Roadmap

The language spec is `MANIFESTO.md` (normative); the reference implementation fixture is
`tests/fixtures/spec.cnb` (immutable). Features are justified by general-purpose systems
programming — kernels, firmware, network stacks, runtimes, and compilers — not by whether
they help rewrite the compiler in Cinnabar. Self-hosting is a goal and a completeness test,
not the gate for any feature. A milestone is done only when the fixtures, the sanitizer gate,
and the spec all agree.

Ordering below respects dependencies and broad systems utility.

---

## Resolved

**O(1) call-stack execution; dynamic recursion guard retired.** Call-stack exhaustion is prevented
at compile time, not detected at runtime. The typechecker rejects any self-recursive call that is
not in strict tail position (the direct value of a `return`, or the non-diverging result expression
of a tail-positioned match) with a source-located error; non-tail recursion must be rewritten with
an explicit accumulator or a user-managed work stack. The runtime stack guard is gone: no per-entry
stack checks, no `getrlimit`/`RLIMIT_STACK` measurement, no `Cinnabar: stack overflow` message, and
no `exit(70)` in any emitted binary. `mem_probe.cnb` (a 500k-deep tail-recursive probe) is now
`EXPECT_OK("mem_probe", 0)`; `rec_test` and `hanoi` were rewritten to tail-recursive accumulator
form with unchanged exit codes 120 and 255. `non_tail_recursion.cnb` is `EXPECT_REJECTED`.

**Self-tail-recursion in O(1) stack.** A call in tail position is marked `tail` in
`emit_call`/`emit_native_call`/`emit_deferred_trait_call`; tailness propagates through the value
expression of a tail-positioned match arm and is cleared everywhere else, so argument subexpressions
and scrutinees are never marked (`return x + f(y)` and `match` scrutinees never mark a call). LLVM
tail-call elimination converts self-tail-recursion to jumps at `-O2`; the `opt default<O2>` step in
`assemble()` runs the module optimization pipeline. `tail_rec.cnb` (1M iterations,
`EXPECT_OK("tail_rec", 64)`) and `mem_probe.cnb`'s 500k-deep probe lock it in. With the guard
retired this is the only mechanism keeping recursion in O(1) stack — which is why non-tail
self-recursion is a compile error.

**Euclidean division/modulo.** `/` and `%` return `Result(T, DivError)`; the remainder is always
non-negative (`0 <= r < |divisor|`) regardless of operand signs. Constant folding and codegen agree,
including `INT_MIN / -1 == INT_MIN` and `INT_MIN % -1 == 0`. A compile-time-provable-zero divisor
is a hard compile error; a runtime zero is `Err(DivByZero)`.

**Loop alloca hoisting.** Codegen hoists every stack slot into the function entry block
(`alloca_raw`), so a `while` body no longer allocates fresh stack per iteration. Fixed the
`vm10`-class SIGSEGV around 104k iterations; `vm5`/`vm10` are genuinely non-terminating and are
killed by the harness after `RUN_TIMEOUT_SECS`. `vm.cnb` reconciled to its documented encoding,
`EXPECT_OK("vm", 120)`.

**Soundness patch set.** The borrow-checker / typechecker / codegen hardening: conservative
linearity for type parameters, backward reassignment tracking in `origin_owners_of`, struct-field
loan retention, linear-element consumption on container insert, linear-join retraction at
fixpoint convergence, boolean/rest/variant/struct-literal pattern and arity enforcement,
duplicate-impl rejection, literal-pattern scalar unification, `impure` call-site enforcement,
struct field visibility, nested-block-comment skip, hex range validation, sign-aware Euclidean
adjustment without `wrapping_abs`, shift-amount masking by operand width, empty-statement-list
lowering, HashMap padding zero-fill, UTF-8 overlong/surrogate rejection, and `main` return-layout
enforcement. This is the correctness floor the remaining milestones build on.

**Resolvability Rule implemented.** Native containers may hold linear elements
only if the container's native surface provides a by-value extraction
function (vec_pop / hash_map_remove). Storing a linear element in a
container without an extraction surface is a compile-time error. Native
extraction primitives (vec_pop, hash_map_remove) added to the Collections
native surface. Container free validation requires containers holding
linear elements to be fully drained before vec_free / hash_map_free.

---

## Milestone 1 — Fixed-Width Integer Suite (COMPLETE)

Implement the full standard suite of fixed-width, non-floating-point integer types. This is a core
capability of any general-purpose systems language, independent of whether the compiler needs it
today: ABI/FFI boundaries, binary file formats, network protocol headers, hardware registers and
memory-mapped I/O, and cryptography all require precise widths. Representing everything as `Int`
(i64) misrepresents these domains and forces unsafe implicit truncation/extension at every
boundary.

### Type set
- Unsigned: `U8`, `U16`, `U32`, `U64`
- Signed: `I8`, `I16`, `I32`, `I64`
- Pointer-sized: `Usize` (existing). Decide whether `Isize` (pointer-sized signed) is needed for
  syscall return values and pointer arithmetic; add it only if a concrete systems use case exists.
- `Bool` remains `i1`.

Decision — fate of `Int`: either (a) retain `Int` as a builtin alias for `I64`, or (b) retire it
in favor of explicit `I64`. Recommend (a) for now: `Int` is documented as `I64`, integer literals
default to `Int`, and explicit-width types are used wherever a width matters.

### Decisions to lock before implementation
1. **Literal typing and range.** Literals are width-agnostic; they adopt the expected type in a
   typed context and default to `Int` otherwise. A literal adopted into a narrower width is
   range-checked at compile time (`300` as `U8` is an error; `-1` as `U16` is an error). Hex
   literals are range-checked against the target width, not just `i64::MAX`.
2. **Conversions.** No implicit conversions (per `MANIFESTO`). Design an explicit, width- and
   sign-aware conversion surface. Recommend a single `T.from(value)` constructor per integer type
   where the typechecker accepts any integer argument and codegen lowers to the correct LLVM cast
   (`zext`/`sext`/`trunc`) based on source and destination width and signedness. Derive everything
   from the canonical type descriptor's width and signedness — no pass matches type names.
3. **Overflow semantics.** Cinnabar arithmetic wraps. Wrapping must be per-width. Confirm whether
   the current approach (fold at `i64`, mask at emission in `const_int_of`) is correct for every
   width and every operator — especially division, remainder, shifts, and signed vs unsigned
   comparison — or make the constant folder width-aware. Do not leave this implicit.

### Implementation across the pipeline
- **`ast.rs`:** add the new builtin kinds; extend `is_int_key`, a signedness predicate
  (`key_is_signed` must cover every signed width, not only `Int`), and a bit-width function
  (returning 16 for the 16-bit types).
- **`typecheck.rs`:** seed all new builtins; generalize `from_u8` into the width/sign-aware
  conversion native; add literal range-checking; verify division/`Result` and arithmetic/comparison
  typing hold for every width.
- **`codegen/types.rs`:** map each builtin to its LLVM integer type (`i8`/`i16`/`i32`/`i64`).
- **`codegen/emitter.rs`:** extend `const_int_of` to mask/emit every width; verify the conversion
  native chooses zero-extend vs sign-extend vs truncate correctly; confirm shift masking uses the
  operand's actual width.
- **`borrow.rs`:** no change required (integers are never linear), but confirm the new builtins
  report non-linear.

### Verification
- Extend `spec.cnb` and the repro corpus to exercise every width: arithmetic, bitwise, comparison,
  division/modulo, shifts at the width boundary, literal range acceptance and rejection, and at
  least one widening and one narrowing conversion per width.
- Add fixtures asserting wrap-around at each width's max/min and correct signed vs unsigned
  comparison.
- The sanitizer gate (Milestone 7) must run the whole suite clean on every width before this
  milestone is considered done.

---

## Milestone 2 — String Literals
- Lexer: `TOK_STRING`, double-quoted, standard escapes (`\n`, `\t`, `\0`, `\"`, `\\`).
- Type: a literal evaluates to `&[U8]` — a static `.rodata` byte array plus a `{ ptr, len }` slice.
  No heap allocation, no lifetime.
- UTF-8 validity of a literal is known at compile time — validate at parse time, not via a runtime
  `Result`.
- `Terminal.print`/`print_line` accept `&[U8]` directly alongside `&Collections.String`.
- No dependency on any later milestone. Ships early because diagnostics, `read_line` ergonomics,
  and text processing all want it.

---

## Milestone 3 — Native Memory Bugs
- `Memory.write_u8` overreads the allocation by 7 bytes (valgrind-confirmed). Fix the store width.
  This is almost certainly also the root cause of the `deallocate`-frees-garbage-pointer bug;
  re-test that one after this fix, do not fix it independently.
- Re-run the affected fixtures under Valgrind/ASan to confirm both are gone.

---

## Milestone 4 — Native OS Surfaces
- `Terminal.read_line() -> Result(Collections.String, Error)`
- `File.open`/`read`/`write`/`close` with a linear `File` handle.
- `Runtime.args() -> &[Collections.String]`
- Static linking only, ever — not configurable per target. `Memory`, `Terminal`, `File` lower
  directly to OS syscalls (`sys_mmap`/`sys_munmap`, `sys_write`/`sys_read`, `sys_open`/`sys_close`
  on x86_64/AArch64) rather than through libc. Where a target genuinely cannot avoid libc, link it
  statically (musl) — never dynamically.
- Depends on Milestone 1 (syscall argument/return widths) and Milestone 2 (`read_line` returns a
  `String`; `args` yields slices).

---

## Milestone 5 — `build.cnb` Project Manifest
- Before designing the format, decide how a compile-time string constant works, since
  `Collections.String` is native, heap-backed, and constructed via an `impure` call — not usable as
  a `const` initializer. Either define a distinct compile-time-only string representation for
  manifest fields, or resolve it via Milestone 2's literal-as-`&[U8]` representation and keep
  manifest fields as `&[U8]`, not `String`.
- CLI: `cinnabar build`, `cinnabar run`, `cinnabar test`, `cinnabar check`.
- Depends on Milestone 2.

---

## Milestone 6 — Diagnostic Quality
- Multi-label Ariadne rendering: error site + relevant definition site in one diagnostic.
- Suggestion engine (dead code, unresolved-name matches): every suggestion is hedged
  ("possible match", "did you mean") regardless of source — cache-derived or purely structural.
  Never state inferred developer intent as fact; an inconclusive cache or ambiguous match falls
  back to the neutral, non-presumptive message. No suggestion may point toward a local bandaid
  (suppress, stub, comment out) over a structurally correct fix.

---

## Milestone 7 — Verification
- Type soundness: progress + preservation, checked against monomorphization, trait dispatch, and
  nested sum-type destructuring.
- AST/type fuzzer generating random well-typed programs, asserting the typechecker/borrow checker
  never accepts an ill-typed or memory-unsafe one.
- Sanitizer gate in `pre_commit_check.sh`: all fixture binaries under UBSan, ASan, and Valgrind.
  Zero UB, zero leaks, zero unhandled traps on every valid program. Since shipped binaries are
  static/syscall-direct, build a separate instrumented target for the sanitizer gate (dynamically
  linked against sanitizer runtimes, as UBSan/ASan require) rather than relaxing the static-only
  rule for shipped output. The sanitizer build is test infrastructure, not a release artifact.

---

## Milestone 8 — Cinnabook and Mushlings
- `cinna burn`: local web server, static content, bundled at compile time, matching the exact
  installed compiler version.
- Mushlings: rustlings-style broken/diagnostic/fixed exercises. Source directly from the failure
  classes already found (dropped `pub`, mixed struct/enum body, unhandled `Result`/`Option`,
  discard patterns, unconsumed linear value, ambiguous returned borrow, compile-time-zero division,
  and the fixed-width overflow/range cases from Milestone 1) — each already has a real compiler
  diagnostic; use it verbatim rather than writing new exercise text from scratch.
- Depends on Milestone 6 (diagnostic rendering) and a stable language surface.

---

## Self-Hosting (a goal, not a gate)
Once the language is complete enough, Cinnabar compiles itself and the compiler becomes a
Cinnabar-emitted binary bound by every principle in `MANIFESTO.md`. This is a completeness test —
it proves the language can express a real compiler — and a hardening exercise, not a criterion any
feature above must satisfy to ship. Where a compiler feature is temporarily unrepresentable, the
boundary is marked explicitly with a native declaration and rewritten in Cinnabar once the language
can express it.
