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
- **`ast.rs`:** add the new builtin kinds, and the width and signedness functions every stage
  reads them through — `builtin_int_width`, `builtin_int_is_signed`, `builtin_int_mask`
  (the width function returning 16 for the 16-bit types).
- **`typecheck.rs`:** `is_int_key` and the signedness predicate live here, over those
  descriptors; `codegen/emitter.rs` reads the same descriptors through its own `key_is_signed`,
  which must cover every signed width rather than only `Int`.
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

## Milestone 4 — Native OS Surfaces (COMPLETE)

### Direct system calls
`src/codegen/syscall.rs` emits the kernel entry point as inline assembly. Everything
architecture-specific lives in that one file and is derived from the module's own target triple:
the instruction (`syscall` on x86_64, `svc #0` on AArch64), the register constraints, and the
syscall numbers. `Memory.allocate`/`deallocate` issue `mmap`/`munmap`, `Terminal.print`/`print_line`
/`eprint` and `Terminal.read_line` issue `write`/`read`, and the whole `File` surface issues
`openat`/`read`/`write`/`close`. `mem_byte_access.cnb` and `file_roundtrip.cnb` both compile to IR
that declares **no libc function at all**.

`Collections` keeps the libc allocator and `Net` keeps the socket wrappers, deliberately: a
growable container needs `realloc` semantics rather than whole mappings, and a `sockaddr` marshaller
is real code rather than a thin syscall shim. That is the roadmap's "where a target genuinely cannot
avoid libc" case, and it is still a static musl link, never a dynamic one.

Details that mattered:
- A Linux syscall reports failure as a **negative errno in the result register**, not through a
  separate `errno` variable, so the syscall path needs no `__errno_location` — `Net`, on the libc
  wrappers, still uses it. `mmap` in particular reports failure that way rather than as a null
  pointer, so the failure test is `< 0`.
- x86_64 passes the fourth argument in `r10`, not `rcx`, because the `syscall` instruction
  overwrites `rcx` and `r11`. `mmap` takes six arguments and is where that matters.
- AArch64's Linux ABI has no `open` at all, only `openat`, so the file surface uses `openat` with
  `AT_FDCWD` on **both** architectures rather than branching per target.
- Both architectures declare a memory clobber. Without it the optimizer could move loads and stores
  across a call that fills or reads a buffer.
- `munmap` needs the mapping's length as well as its address — exactly the handle field Milestone 3
  made sure is always initialized. A garbage length there would unmap memory the program still owns.
- An architecture with no implemented table is a compile error naming the triple, never a guess: a
  syscall number is meaningless on an architecture it was not assigned for, and emitting one anyway
  would call an arbitrary kernel entry point.

### Surfaces
- **`File`** — a linear `File.Handle` with `open`/`read`/`write`/`close`. The mode is a Cinnabar
  enum (`ReadOnly`, `WriteTruncate`, `WriteAppend`) rather than an integer of flag bits, so a
  program never writes an operating-system constant; the variant tags come from the program's own
  declaration through `variant_tag_of`, keyed by name rather than by declaration order. `read` and
  `write` return the count actually transferred instead of looping, because a short count is
  information the caller needs — a zero count from `read` is how end of file is observed. A path
  arrives as a `&[U8]` carrying its own length but `openat` wants a terminator, so `open` copies it
  into a `PATH_MAX` stack buffer; the length is **clamped before** the copy rather than only tested,
  so an over-long path cannot overrun the buffer between the test and the `memcpy`, and it is
  rejected with `ENAMETOOLONG` — the code the kernel would itself have returned.
- **`Terminal.read_line`** — reads standard input byte at a time, which is the design rather than an
  oversight: a larger read would consume bytes past the newline, and an unbuffered descriptor has
  nowhere to put them back, so the next read would silently lose them. Buffering belongs to a reader
  the program owns. The newline is consumed but excluded; end of input with nothing read is
  `Err(EndOfInput)` rather than an empty string, so a blank line stays distinguishable from "no more
  lines". `Terminal.print`, by contrast, *does* loop, because a `write` may transfer fewer bytes
  than asked and `print` returns `Unit` — it has no channel on which to report a short write.
- **`Runtime.args`** — returns `&[Collections.String]`, a shared borrow rather than an owned
  collection. The argument strings live in memory the kernel set up for the process and last for the
  whole run, so there is no moment at which freeing one would be correct. Linearity enforces that by
  construction rather than by convention: a `String` cannot be moved out of a slice, so a program
  can read an argument but can never hand one to `string_free`. Getting at `argv` required the entry
  point to become `main(int argc, char **argv)` — the C runtime passes the command line there and
  nowhere else — with the two values stashed in module globals. The argument table is built lazily
  and once, so a program that never asks pays two stores and allocates nothing.

### Two fixes this milestone forced
**A use-after-free that compiled.** Writing the `File` fixtures surfaced a soundness hole older than
this milestone: borrowing a linear value after it was moved was not checked at all. `apply_move`
caught *moving* a moved value, but `deallocate(block)` followed by `read_u8(&block, 0)` compiled
cleanly and produced a binary that reads freed memory — a direct violation of the Crucible Rule, and
the more dangerous of the two shapes, since the move it follows has already released the resource.
`OP_BORROW`/`OP_BORROW_M` now check the binding's linear state. Only `ST_MOVED` is reported, never
`ST_PARTIAL`: a partially moved struct still has live fields, and reaching them is what partial-move
tracking is for. `borrow_after_move.cnb` pins all four shapes.

**A fixture passing on undefined behaviour.** `mem_probe.cnb` recursed over a block it had only
allocated, never written. LLVM knows freshly `malloc`'d memory is undef and folded the byte test to
whichever branch it liked — which happened to be the one the expected exit code wanted. `mmap`'s
anonymous pages arrive genuinely zero-filled, so the read now returns a real `0` and the fixture
fails honestly. It seeds the byte it means to read; the point was always 500000 tail-recursive
frames each doing an opaque heap read, not what an unwritten byte contains.

### Verification
- `file_roundtrip.cnb` writes, reads back byte for byte, appends and confirms the file grew rather
  than being replaced, and checks that opening a missing path fails with a real errno rather than
  succeeding with a handle to nothing. `file_unclosed.cnb` pins the linear obligations on a
  descriptor: leaked, closed on one path only, closed twice, used after closing.
- `runtime_io.cnb` checks that `args` reports exactly the program name when invoked with none, that
  the name is non-empty (proving the handle was built from the real `argv` rather than left zeroed),
  that calling twice yields the same view (the table is built once), and that `read_line` reports
  end of input rather than an empty string when there is no input.
- The syscall ABI table is checked by a **second, independently hand-written copy** of the Linux
  numbers, plus tests that numbers are distinct within an architecture, that `r10`/`x8` are used
  where the ABI requires, that both architectures clobber memory, and that an unimplemented
  architecture reports as unknown rather than falling back to one of the two tables.
- The repro harness now gives every fixture a null standard input. `Terminal.read_line` blocks until
  a line or end of input arrives, so an inherited descriptor would make a fixture's exit code depend
  on whether the suite was run from a terminal, a pipe, or CI.

### Carried forward
`Memory.allocate` maps whole pages, so a small allocation costs a page. That is the right trade for
a raw-memory quarantine and the wrong one for many small allocations; if `Memory` ever needs to be
allocation-dense, a suballocator over `mmap` belongs in the surface rather than a return to libc.

---

## Milestone 5 — `build.cnb` Project Manifest (COMPLETE)

`build.cnb` is Cinnabar source, read by the compiler's own front end.

```
pub const NAME: &[U8] = "cinnabar"
pub const ENTRY: &[U8] = "main.cnb"
pub const TESTS: &[U8] = "tests"
```

Field names are `SCREAMING_SNAKE_CASE` because the compiler enforces that casing on `const`
declarations — a lowercase field is a compile error, not a style complaint. Manifest fields are
`&[U8]`, decided in Milestone 2 and not reopened: `Collections.String` is native, heap-backed
and built by an `impure` call, and is simply not what a manifest field is.

### It is parsed, not scanned
`load_manifest` runs `analysis::analyze` and reads `ITEM_CONST` nodes, taking each field's
declared type and folded value from what the resolver and typechecker already attached. It had
been a hand-rolled `key = value` line splitter: a second implementation of the front end living
beside the real one, in a file whose extension claims it is Cinnabar.

A field's type is checked against the canonical descriptors — `TYD_REF` over `TYD_SLICE` over
`BUILTIN_U8` — never against the spelling of a type name. Duplicate fields need no check of
their own: two `pub const NAME` declarations are a duplicate symbol, and the resolver already
owns that fact.

### `NAME` names the artifact, and is checked because it reaches the disk
`cinnabar build` names its output after `NAME` rather than after whichever file happens to be
the entry point, so renaming an entry source does not rename the project.

That makes `NAME` a path, so it is validated as one: a single component, no separator, no parent
step, no root, no drive prefix. `ENTRY` and `TESTS` are confined because a path is visibly a
path; `NAME` reaches the same filesystem through a field that does not look like one, which is
why it was worth checking rather than trusting.

### Verification
Manifest parsing, a missing required field, a wrong field type, a path escaping the root, a
non-`pub` item, a name escaping its component, and a `build.cnb` that is not valid Cinnabar at
all. The two symlink-escape tests are unchanged in what they test.

---

## Milestone 6 — Diagnostic Quality (PARTIAL)

### Shipped
Definition-site labels, rendered by default rather than behind a flag: a duplicate symbol labels
the first declaration, an immutable assignment labels the `val` binding, an unhandled
`Result`/`Option` labels the producing function's return type, and a return-type or
constant-initializer mismatch labels the declaration whose type it violates. `--explain-borrow` still gates only the borrow checker's own
explanations.

`src/suggest.rs` offers near matches for unresolved names, drawn from the scope facts the
resolver materialized rather than a second walk. Every suggestion is hedged — "did you mean" is the
only phrasing emitted, and the accepted-hedge list holds only what is emitted — an ambiguous
match names no candidate and falls back to the neutral message, and no suggestion points
toward a local bandaid. That wording contract is asserted directly:
each suggestion carries a hedge, none carries bandaid vocabulary, and a tie stays silent.

### Dead code is a rejection

An item nothing reachable from `main` needs is a compile error — functions,
constants, structs, enums, traits and native declarations alike, public and
private without distinction.

`pub` is not an exemption. It says who may name a thing, not whether anything
does, and exempting it would make one word the way to silence this diagnostic.
That is what was wrong with the first attempt at this check: it spared `pub`,
so the only remedy it left anyone was to mark demonstration code public until
the compiler stopped objecting — the local bandaid this milestone forbids a
diagnostic from steering toward.

Reachability is a graph walk from `main`, taken to a fixpoint, so two dead
functions that call each other are still dead. It is reported after type and
borrow checking: a program that does not type-check is told what is wrong with
it, not which of its functions nothing calls. Reported from the resolver it
would stop the pipeline first, which is the shadowing that left
`invalid_resolver_and_typechecker.cnb` asserting four of its twenty-four cases.

A unit with no `main` is exempt, because it is not a whole program: a module
compiled alone, and `build.cnb`. Nothing that can be built into a binary
escapes that way, since codegen requires `main`.

### Carried forward
- **Dead-code suggestions.** The rejection exists; what the suggestion engine
  should *say* alongside it does not.
- **Definition-site labels do not yet cover every type mismatch.** Return-type
  and constant-initializer mismatches carry one; variant-value and call-result
  mismatches do not. Nothing structural stands in the way — the declaring node
  is already resolved at both sites.

---

## Milestone 7 — Cinnabook and Mushlings (COMPLETE)
- `cinnabar burn`: local web server, static content, bundled at compile time, matching the exact
  installed compiler version. There is one binary and it is called `cinnabar`; no shortened
  `cinna` is shipped, aliased, or packaged.
- Mushlings: rustlings-style broken/diagnostic/fixed exercises, each using a real compiler
  diagnostic verbatim rather than new exercise text.
- Depends on Milestone 6 (diagnostic rendering) and a stable language surface.

### Shipped
`cinnabar burn` serves version-pinned Cinnabook documentation locally.

Mushlings ships **eight** exercises, each sourced from a failure class that has a real compiler
diagnostic: mixed struct/enum body, unconsumed linear value, unhandled `Result`, ambiguous
returned borrow, compile-time-zero division, fixed-width literal range, non-tail recursion, and
dropped `pub`.

### Discards are rejected, and the exercise teaches that

This entry once listed discard patterns among the classes to source an
exercise from, on the basis that each already had a diagnostic to use
verbatim. That was not true of this one: a bare underscore as a match arm, as
a binding, and as the prefix of one all compiled. The exercise written from it
taught a casing rule while claiming to teach discards, and was withdrawn.

The rule now exists. An identifier may not be a lone underscore and may not
begin with one, in any position, enforced in the lexer where casing is — so it
holds in a file that would fail later for other reasons. The match arm is the
consequential case: a catch-all makes any match trivially exhaustive, so adding
a variant to an enum would stop forcing anyone to handle it.

Exercise 09 is restored and teaches the rule it names.

---

## Milestone 8 — Verification (PARTIAL)

### The memory-checker gate, and the link mode it needed
Every valid program in the expected-success corpus runs under Valgrind memcheck, failing on any
error or definite leak, and separately asserted to still exit with the code it is supposed to —
a link mode that changed what a program computes would make every clean report meaningless.

That needed a second link mode. A shipped binary is static, `-nostdlib`, `-no-pie`, against a
musl `libc.a` embedded in the compiler, so it carries no dynamic section and memcheck has no
`malloc` to interpose on: it reports `0 allocs, 0 frees` for a program that demonstrably
allocates. `LinkMode::Instrumented` hands the *same object file* to the driver without the flags
that cut it off from the host libc, keeping `-no-pie` because `llc` emits a non-relocatable
object. The static-only rule for shipped output is not relaxed and no binary a user receives is
built this way.

Two tests exist to keep the gate from being vacuous. Memcheck must report real allocations for
the instrumented build, or the gate is running and seeing nothing; and a C program leaking 64
bytes must fail the same runner, or the gate is running without objecting to anything.

The gate lives in `cargo test`, which `pre_commit_check.sh` already invokes, rather than as a
new step in that script.

### Undefined behaviour, proven rather than instrumented
UBSan's checks are emitted by Clang's front end while lowering C, so there is no pass to run over
a Cinnabar module — and almost every class it checks is one this language designed out rather than
left unchecked. Arithmetic wraps per width and the emitter carries no `nsw` or `nuw` to say
otherwise; shift counts mask by `width - 1`; division returns `Result`; a constant index out of
range is a compile error and a dynamic one returns `Result`; the raw memory surface bounds-checks;
there is no dereference operator.

Adding runtime checks for those would be checking conditions the language does not admit, so the
property asserted is the stronger static one: **the compiler does not emit IR whose behaviour is
undefined**. It holds for every program the compiler can produce rather than only the ones a
corpus executes. Scanned across the whole expected-success corpus: no overflow flag on any
arithmetic instruction, no `exact`, no `poison`, and `unreachable` only in control-flow joins that
every path diverges out of — the not-taken edge of a match's last pattern test, which
exhaustiveness proves cannot be taken, and the join after a branch whose arms all return.

A second test keeps the scan from being vacuous: it must match arithmetic in a fixture exercising
every width and operator, since a scanner matching nothing would pass every fixture in silence.

### Carried forward
- **ASan.** The runtime links and works in the dev shell, but the instrumentation is an IR pass
  that has to run between `opt` and `llc` in the compiler's own pipeline. Valgrind covers the
  heap errors and leaks that motivated the milestone; ASan would widen it to stack and global
  overflows.
- **Type soundness — progress and preservation.** Not started, and the largest piece left.
  `cinnabar soundness` emits machine-checkable front-end evidence and says `formal_proof: false`,
  which is honest about being a count of what the front end accepted rather than a mechanized
  proof. Monomorphization, trait dispatch, and nested sum-type destructuring are the cases it has
  to survive.
- **The fuzzer's second half.** `tests/fuzz_generalization.rs` generates random well-typed
  programs already, and `generate_negative` covers two fixed linearity-probe shapes. What it
  does not do is generate memory-unsafe programs and assert the borrow checker rejects them.

---

## Self-Hosting (a goal, not a gate)
Once the language is complete enough, Cinnabar compiles itself and the compiler becomes a
Cinnabar-emitted binary bound by every principle in `MANIFESTO.md`. This is a completeness test —
it proves the language can express a real compiler — and a hardening exercise, not a criterion any
feature above must satisfy to ship. Where a compiler feature is temporarily unrepresentable, the
boundary is marked explicitly with a native declaration and rewritten in Cinnabar once the language
can express it.
