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

### Type set (shipped)
- Unsigned: `U8`, `U16`, `U32`, `U64`
- Signed: `I8`, `I16`, `I32`, `I64`
- Pointer-sized: `Usize` and `Isize`. `Isize` shipped: signed syscall returns (a negative errno)
  and signed pointer displacement both need a pointer-sized signed type, and neither is expressible
  as `Usize` without a lossy reinterpretation at every boundary.
- `Bool` remains `i1`.

Decision — fate of `Int`: **(b), retired.** The integer grid is the ten fixed-width types and
nothing else; `I64` is the sole 64-bit signed type and the width an untyped integer literal
defaults to. Keeping `Int` as an alias would have left two spellings for one type in every
diagnostic, every `T.from` receiver, and every fixture. `duplicate_builtin_int.cnb` locks the
builtin against redeclaration.

### Decisions locked
1. **Literal typing and range.** Literals are width-agnostic; they adopt the expected type in a
   typed context and default to `I64` otherwise. A literal adopted into a narrower width is
   range-checked at compile time (`300` as `U8` is an error; `-1` as `U16` is an error). Hex
   literals are range-checked against the target width, not just `i64::MAX`. `range_check_literal`
   derives the bound from the canonical width and signedness, and checks a negated literal against
   the signed half of the width so an out-of-range hex magnitude cannot wrap back into range.
   Rejection is fixed by `int_literal_range.cnb` and `int_unsigned_neg.cnb`.

   **What counts as a typed context — decided, not left implicit.** Two things supply a literal's
   type and nothing else does: (a) the type expected of the position it occupies (declared type of
   a `const`/`val`/`var`, parameter type, return type, array element type, struct field type), and
   (b) the type of the peer operand of a binary operator, when exactly one side is a *bare literal
   expression* — one built out of nothing but integer literals, unary `-`, and integer-valued
   binary operators. `MANIFESTO.md`'s "Literal typing context" paragraph is the normative
   statement.

   Deciding (b) the other way — literals take an expected type but never a peer's — was the
   defensible alternative, since `MANIFESTO` forbids implicit conversions. It was rejected for two
   reasons. First, the language already depended on it: `a + 1` with `a: U16` typed the `1` as
   `U16` from its peer long before the rule was written down, and withdrawing that would have
   broken arithmetic at every width but `I64`. Second, it is not a conversion. A conversion changes
   a value that already has a type; a literal has no type to change, which is precisely why
   `T.from` exists for the cases that *are* conversions. The boundary that keeps this honest is
   that only a bare literal adopts: a path, call, index, field access, `match`, `try`, or `const`
   reference is a typed value, so `narrow == wide` across two widths stays an error. Because an
   adopted literal is range-checked, adoption strengthens diagnostics rather than weakening them —
   `narrowed != 300` with `narrowed: U8` reports the out-of-range literal instead of a type
   mismatch.

   **The const path and the runtime path implement one rule, not two.** `fold_const` pushed the
   declared type into a binary expression's operands while `check_binary` typed its operands with
   no expectation at all, so `pub const ISIZE_MIN: Isize = -9223372036854775807 - 1` was accepted
   and `var min: Isize = -9223372036854775807 - 1` was rejected with "cannot assign 'I64' to
   'Isize'" — and the `I64` form of the same text worked only by coincidence, because the default
   width happened to match. Both paths now call the same two helpers, `int_literal_expr` (which
   operand is bare-literal) and `binary_operand_expected` (what the result's expected type implies
   for the operands: nothing for a comparison, which yields `Bool`; the `Ok` payload for `/` and
   `%`, which yield `Result(T, DivError)`; the expected type itself otherwise), so the same source
   text cannot type one way in a `const` and another in a `var`.
2. **Conversions.** No implicit conversions (per `MANIFESTO`). A single `T.from(value)` constructor
   per integer type: `check_int_from` accepts any integer argument and keys the lowered conversion
   by both receiver and source type, and `coerce_int` selects `zext`/`sext`/`trunc` from the two
   widths and the source's signedness. Everything derives from the canonical type descriptor —
   no pass matches type names.
3. **Overflow semantics.** Cinnabar arithmetic wraps, per width. Fold-at-`i64`-then-mask was **not**
   sufficient: it gets shift-count masking and unsigned comparison wrong at every width below 64.
   The constant folder is therefore width-aware — `fold_bin` sign-extends signed operands, masks
   unsigned ones, masks shift counts by the operand width, orders comparisons by signedness, and
   stores the width-masked bit pattern that `const_int_of` masks again at emission. Euclidean
   division and the defined `MIN / -1` edge hold at every signed width.

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
- `tests/fixtures/verify_math/int_widths.cnb` prints a computed value per width — wrapping,
  signed vs unsigned comparison, shift-count masking, bitwise ops, `T.from` truncate/zero-extend/
  sign-extend, and Euclidean division/modulo below 64 bits — and `int_widths_oracle` in
  `tests/verify_math.rs` recomputes every line with independent Rust arithmetic and compares
  output line by line. Exit codes are not the assertion; the values are.
- `tests/fixtures/repro/int_width_grid.cnb` covers the same grid against the *other*
  implementation of the semantics, the width-aware constant folder: for all ten types it asserts
  a folded `const` equals the runtime result for wrap-around at both max and min, shift-left at
  the width boundary, shift-right past it, the three bitwise ops, division, modulo, signed vs
  unsigned comparison, and a widening and a narrowing conversion. Folder and codegen cannot drift
  apart silently.
- `tests/fixtures/repro/int_literal_context.cnb` pins the literal-typing context rule at all ten
  widths: for each type it asserts that a folded `const` and a runtime `var` given the *identical*
  initializer text agree at both of the width's wrap points, that a bare literal adopts the peer
  operand's type on either side of a comparison and on the left of an arithmetic operator, that a
  nested all-literal tree adopts the expected type through every level, and that a literal shift
  count adopts the shifted operand's type. The reported const-vs-local reproducer
  (`-9223372036854775807 - 1` as `Isize`) is check 1, verbatim.
  `tests/fixtures/repro/int_literal_no_peer.cnb` pins the boundary: two typed values of different
  widths or signedness never adopt each other, a `const` reference is a typed value rather than a
  literal, and a literal that does adopt a peer's type is range-checked against it — in the const
  path and the runtime path alike.
- `int_min_neg1.cnb` and `shift_mask.cnb` pin the `I64` edges (`MIN / -1`, `MIN % -1`,
  shift counts past the width) across both paths; `int_literal_range.cnb` and
  `int_unsigned_neg.cnb` pin literal range and unsigned-negation rejection.
- Carried forward, not a gate on this milestone: the sanitizer gate (Milestone 8) must run the
  whole suite clean on every width. Milestone 1 is complete on the fixture and oracle evidence
  above; the sanitizer pass is Milestone 8's acceptance criterion applied to this suite, and
  ordering it as a precondition here would make every milestone wait on the last one.

---

## Milestone 2 — String Literals (COMPLETE)

### Shipped
- **Lexer:** `TOK_STRING`, double-quoted, with exactly the five escapes `\n`, `\t`, `\0`, `\"`,
  `\\`. Any other escape is a lexical error rather than a passed-through backslash, so a literal's
  byte sequence is fully determined at lex time and no later stage re-reads the quoted source. A
  literal does not span lines: an unescaped newline before the closing quote is an error, which
  stops a missing quote from swallowing the rest of the file. Recovery keeps scanning the same
  literal after a bad escape, so one mistake reports one diagnostic.

  Unescaped text is copied out of the source as whole `&str` runs rather than byte by byte, so a
  multi-byte character passes through as the character it is. The decoded bytes are interned in the
  existing name arena — the arena that already stores byte sequences — which is also what makes
  equal literals one value downstream.
- **Type:** `&[U8]`, and exactly that; a string literal adopts nothing from its context, unlike an
  integer literal (see Milestone 1's literal-typing rule). It evaluates to a `private
  unnamed_addr constant` byte array — confirmed to land in `.rodata` in the linked binary — plus a
  `{ ptr, len }` slice over it. No heap allocation, no owner, no lifetime.
- **Compile-time UTF-8:** validity follows from the escape set rather than from a validation pass.
  Source text is UTF-8, every unescaped byte is therefore part of a well-formed sequence copied
  through unchanged, and all five escapes decode to ASCII, so no decoding step can introduce a
  malformed sequence. This is why there is **no** `\xNN` byte escape: one would let a literal name
  a lone continuation byte and put runtime validation back on a value that is supposed to be
  settled at compile time. Writing a validation pass over bytes that cannot be invalid would have
  been a check that never fires; excluding the escape that could break the invariant is what
  actually enforces it.
- **`Terminal.print`/`print_line` accept `&[U8]`** alongside `&Collections.String`. The language
  has no overloading, so this is one native whose *emitted body* reads the (data, length) pair out
  of whichever byte-sequence representation the program declared: a `&Collections.String` parameter
  is a pointer to a heap-owning handle whose fields are loaded through it, while a `&[U8]`
  parameter *is* the `{ ptr, len }` view. `byte_view_of` decides between them from the canonical
  type descriptor the typechecker attached to the parameter, never from the parameter's spelling.
- **Compile-time string constants**, which Milestone 5 asks to have decided before designing
  `build.cnb`: `pub const NAME: &[U8] = "Cinnabar"` works, and the answer is the second of the two
  options that milestone lists — manifest fields are `&[U8]`, resolved through Milestone 2's
  literal representation, with no distinct compile-time-only string type. A string constant folds
  to the interned name id of its bytes rather than to a number, and materializes through the *same*
  `.rodata` global as an inline literal of the same text, so `const` and inline strings are one
  representation rather than two.

### The borrow checker had to learn that static data is an origin
Returning a literal — `fun name() &[U8] return "cinnabar" end` — was rejected as *"returned borrow
has no traceable origin: it does not derive from any input reference parameter"*. That rule
(`MANIFESTO` principle 5) exists to answer "how long does the caller's data live?", and a literal
makes the question moot: its bytes are in read-only data for the whole run, so the borrow has an
origin, it just is not an input, and it outlives every caller. Returning a fixed message is one of
the things string literals exist for, so this had to be fixed rather than worked around.

A statically rooted return — a string literal, or a `const` of a reference type — emits no
returned-borrow obligation at all, because there is no loan to trace. Forwarding one needed the
function summary too: a summary previously recorded only *which input parameters* a returned borrow
derives from, and a reference-returning function that derives from none of them had no entry, which
a caller could not distinguish from "not analyzable". An empty source set is now recorded and means
"static", so `return literal_message()` stays static across the call. The set only ever grows, so
the call-graph fixpoint is still monotone and still converges.

Nothing about the rule is relaxed: `ret_borrow_ambiguous.cnb`, `ret_borrow_sole_input.cnb`, and
`ret_borrow_uaf.cnb` remain rejected, and returning a borrow of a local still reports "returned
borrow does not outlive the function". `string_static_borrow.cnb` pins the accepted side — direct
literal returns, a `const` return, several static returns on different paths, one and two levels of
forwarding, and a static borrow flowing through a struct field, an argument, and an array element.

### Comparison had to be stated as a scalar operation
`"a" == "b"` is the most natural thing to write once literals exist, and it made the **compiler
panic** — a Rust backtrace with no diagnostic and no span, in a codebase whose rule is that codegen
failures are never a panic. The typechecker's comparison branch required only that the two operands
have the *same* type, never that the type had a comparison, so codegen was handed an aggregate
where it expected a scalar and `into_int_value` aborted the process. This was a pre-existing hole
(`struct == struct` panicked identically and always had); string literals only made it trivial to
reach.

`comparable_key` now states the rule once: `==`/`!=` compare integers and `Bool`, the ordering
operators compare integers, and nothing else has a comparison. `check_binary` and the constant
folder both call it, so a `const` and a runtime expression agree on exactly which comparisons
exist. `MANIFESTO.md`'s Operators section records the rule normatively. Codegen is hardened
independently: an operand that is not a scalar is now an internal diagnostic carrying a real span
rather than a panic, so a future gap in the front end degrades to an honest error message.

`op_text` moved to `ast.rs` on the way — codegen needed to name an operator in that diagnostic, and
a second copy of the opcode-to-spelling mapping would be exactly the kind of parallel fact that
drifts.

### Toolchain consequences that were part of the work
- **The formatter had to learn about strings.** It extracts a comment-free "code view" of each line
  to decide indentation; without string awareness a `#` inside a literal would start a comment, a
  bracket inside one would change nesting depth, and `print("match")` would read as opening a match
  block and mis-indent every line after it. String literals now collapse to an empty pair in that
  view, with `\"` correctly not ending the literal. The formatter still never rewrites token text,
  so literal contents pass through verbatim.
- **Dumps stay text.** A decoded literal can contain a newline or a NUL, which would break a
  line-oriented dump or make it non-text. `escaped_literal_text` renders a literal back into the
  source form that would produce it, and both `--dump-ast` and `--dump-typed-ast` go through it —
  one implementation, not two. Fixing this also exposed and fixed a pre-existing bug in the AST
  dumper: `lit_kind_name` compared `LIT_*` values against `TOK_*` constants, which do not line up,
  so every integer literal was already being labelled with the wrong kind.

### Verification
- `tests/fixtures/repro/string_literal.cnb` asserts byte lengths (a literal's length is its byte
  count, not its character count: `"é€𝄞"` is 9), the decoded value of each of the five escapes in
  order, indexing into a literal, and that an inline literal compares byte-for-byte equal to a
  `const` of the same text — the observable consequence of both resolving to one global.
- `tests/fixtures/repro/string_print.cnb` prints a `&[U8]` constant and an inline literal through
  `Terminal.print_line`/`print`, while `spec.cnb` (immutable) keeps exercising the
  `&Collections.String` form of the same natives, so both parameter shapes stay covered.
- `tests/fixtures/repro/string_bad_escape.cnb` pins the lexical rejections: an unknown escape, a
  `\xNN` byte escape, and a literal running off its line. `string_not_an_int.cnb` pins the type
  boundary — a string is not an integer and an integer is not a byte slice, in constant
  initializers, arithmetic, comparison, and argument position.
- Lexer unit tests cover escape decoding, multi-byte UTF-8 preservation at the byte level, the
  empty literal, that equal literals intern to one name id (the property codegen's global reuse
  rests on), and each lexical rejection. Formatter unit tests cover keywords, `#`, brackets, and
  escaped quotes inside literals.

---

## Milestone 3 — Native Memory Bugs (COMPLETE)

### What the reports said, and what was actually there
The two reported defects were an `Memory.write_u8` that overreads its allocation by 7 bytes, and a
`Memory.deallocate` that frees a garbage pointer — the second suspected to share the first's root
cause. Investigating them together, as the milestone required, found one real defect, and it is
the second one's root cause rather than the first's.

**Access width: not present.** `write_u8` and `read_u8` compute their target with a
`getelementptr i8` over the block's data pointer, guard it with `offset < len`, and access exactly
one byte (`store i8` / `load i8`). A wide access would be visible in the emitted IR and is not
there. `mem_byte_access.cnb` confirms it behaviourally as well.

**Handle initialization: real, and fixed.** Every native handle — `Memory.Block`,
`Collections.Vec(T)`, `Collections.String`, `Collections.HashMap(K, V)` — lowers to one shared
layout, `{ ptr, i64, i64 }` (`native_llvm`). Handles are moved, passed, and returned *by value*:
`deallocate(block: Block)` lowers to `define { i64 } @Memory_deallocate({ ptr, i64, i64 })`, so the
caller loads all 24 bytes of the layout at the move. But `native_allocate` wrote only the data and
length fields, and `native_string_from_slice` likewise — neither surface uses the capacity field,
so neither initialized it. Every `Memory.deallocate` and every `String` move therefore read 8 bytes
of uninitialized stack. That is exactly the shape of "frees a garbage pointer": a handle carrying a
field that was never written.

The fix is category-level rather than one more store per constructor. `init_native_handle` zero-
fills a handle across its **whole lowered layout** in a single aggregate store of
`StructType::const_zero()`, derived from the handle's own LLVM type rather than a hand-counted run
of per-field stores, and every constructor calls it before storing its own fields. A constructor
cannot skip a field, and the handle layout can grow without silently reintroducing the bug.
`store_null_data`, which hardcoded three field indices, is gone.

### Why Valgrind and ASan cannot be the gate here
The milestone asked for the affected fixtures to be re-run under Valgrind/ASan. That is not
achievable for this toolchain's output, and the reason is structural rather than incidental. A
Cinnabar binary is linked `-static -nostdlib -no-pie` against a musl `libc.a` embedded in the
compiler, so it has **no dynamic section at all** (`readelf -d`: "There is no dynamic section in
this file"). Valgrind's memcheck works by interposing on the allocator through the dynamic linker;
with nothing to interpose on it reports `0 allocs, 0 frees, 0 bytes allocated` for a program that
demonstrably allocates — including with `--soname-synonyms=somalloc=NONE`, the documented recipe
for statically linked allocators, and at `-O 0` so that no allocation has been optimized away. ASan
is no better placed: its runtime requires the libc the link deliberately does not provide. Since
"Static linking only, ever" is a Milestone 4 commitment and not negotiable, the verification is
built in-tree instead.

### Verification
- `tests/fixtures/repro/mem_byte_access.cnb` is the in-language access-width oracle. It fills a
  16-byte block with a distinct non-zero value per offset, overwrites one offset with a probe byte
  sharing no set bit with any fill value, and requires every *other* offset to still hold its fill
  value. A store wider than one byte necessarily lands on an adjacent offset — lower ones on a
  big-endian target, higher on a little-endian one — so the probe runs at the first, a middle, and
  the last offset to catch a wide store in either direction; a wide *read* is caught by the
  readback returning a neighbour's bits. It also asserts that an out-of-bounds access reports the
  offset asked for and the block's own length, which reads the handle's length field back out
  after construction and borrowing. The oracle is mutation-checked: widening the `write_u8` store
  to `i64` makes it exit 23 ("neighbour clobbered at the first offset") instead of 0.
- `tests/native_memory.rs` asserts the same properties against the emitted LLVM IR, where an access
  width is stated literally: `write_u8` stores exactly `i8` to the byte it computed, `read_u8`
  loads exactly `i8` from it, `deallocate` frees the pointer loaded from field 0 of the handle, and
  every native-handle constructor zero-fills the full `{ ptr, i64, i64 }` layout *before* storing
  any field into it (ordering asserted, since a zero-fill emitted after the field stores would
  erase them). The IR assertions catch what no in-language check structurally can: an
  uninitialized handle field is read as garbage rather than producing an observably wrong value.

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
- **Decided in Milestone 2, not open.** Compile-time string constants resolve via the
  literal-as-`&[U8]` representation, and manifest fields are `&[U8]`, not `String`. There is no
  distinct compile-time-only string type: `pub const NAME: &[U8] = "cinnabar"` folds to the
  interned name id of its bytes and materializes through the same `.rodata` global as an inline
  literal of the same text. `Collections.String` stays what it was — native, heap-backed, and
  constructed by an `impure` call — and is simply not what a manifest field is.
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

## Milestone 7 — Cinnabook and Mushlings
- `cinna burn`: local web server, static content, bundled at compile time, matching the exact
  installed compiler version.
- Mushlings: rustlings-style broken/diagnostic/fixed exercises. Source directly from the failure
  classes already found (dropped `pub`, mixed struct/enum body, unhandled `Result`/`Option`,
  discard patterns, unconsumed linear value, ambiguous returned borrow, compile-time-zero division,
  and the fixed-width overflow/range cases from Milestone 1) — each already has a real compiler
  diagnostic; use it verbatim rather than writing new exercise text from scratch.
- Depends on Milestone 6 (diagnostic rendering) and a stable language surface.

---

## Milestone 8 — Verification
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

## Self-Hosting (a goal, not a gate)
Once the language is complete enough, Cinnabar compiles itself and the compiler becomes a
Cinnabar-emitted binary bound by every principle in `MANIFESTO.md`. This is a completeness test —
it proves the language can express a real compiler — and a hardening exercise, not a criterion any
feature above must satisfy to ship. Where a compiler feature is temporarily unrepresentable, the
boundary is marked explicitly with a native declaration and rewritten in Cinnabar once the language
can express it.
