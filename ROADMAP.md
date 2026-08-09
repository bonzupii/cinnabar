# Cinnabar — Next Milestones

## 1. String literals

- Lexer: `TOK_STRING`, double-quoted, standard escapes (`\n`, `\t`, `\0`, `\"`, `\\`).
- Type: literal evaluates to `&[U8]`, a static `.rodata` byte array + `{ ptr, len }` slice. No heap allocation, no lifetime.
- UTF-8 validity of a literal is known at compile time — validate at parse time, not via a runtime `Result`.
- `Terminal.print`/`print_line` accept `&[U8]` directly, alongside `&Collections.String`.
- No dependency on anything below. Ships first.

## 2. Recursion depth guard

RESOLVED. Every user function's entry block checks its consumed stack
(`llvm.frameaddress(0)` vs the process's own `RLIMIT_STACK` soft limit,
read once in the `main` wrapper, minus a guard margin); past the limit it
calls a runtime that writes `Cinnabar: stack overflow` to stderr and
exits 70 (EX_SOFTWARE) instead of the OS SIGSEGV. Tail recursion is
handled separately by tail-call elimination (see section 8).
`mem_probe.cnb` graduated to `EXPECT_OK("mem_probe", 70)` — the guard,
not TCO, is what makes it pass, exactly as this milestone specified.

Two hardening details since the milestone landed, both now documented in
MANIFESTO.md's Runtime Guarantees: an unlimited `RLIMIT_STACK` soft limit
(`ulimit -s unlimited`, RLIM_INFINITY) is clamped to the POSIX default
8 MiB so the guard is never silently disabled (verified: `mem_probe`
still exits 70 under `ulimit -s unlimited`), and a failed `getrlimit`
falls back to that same default. The `EXPECT_OK("mem_probe", 70)`
assertion therefore assumes a finite stack soft limit (the default 8 MiB)
— under `ulimit -s unlimited` the fixture's recursion completes with exit
0 instead of tripping the guard — which the fixture header documents.

## 3. Euclidean division/modulo

- Constant folding (`typecheck.rs`): `.div_euclid()` / `.rem_euclid()` in place of `wrapping_div`/`wrapping_rem`.
- Codegen (`emitter.rs`): non-negative remainder for signed integers, `0 <= r < |divisor|`.
- `div_zero_const.cnb` (compile-time-provable-zero divisor) must be a hard compile error; runtime-unknown divisor returns `Result(T, DivError)`.

## 4. Native memory bugs (from the notes-file triage)

- `Memory.write_u8` overreads the allocation by 7 bytes (valgrind-confirmed). Fix the width of the store — this is almost certainly also the root cause of the `deallocate`-frees-garbage-pointer bug; re-test that one after this fix, don't fix it independently.
- Backward-jump `while` loop crash (`vm10`-class repro): RESOLVED. Root cause was neither an LLVM `-O2` miscompilation nor nested `continue` — codegen emitted every loop-body `alloca` at the active insertion point, and an LLVM `alloca` executed inside a loop allocates fresh stack per iteration (freed only at function exit), so long/infinite loops grew the frame ~80B/iteration and SIGSEGV'd around 104k iterations. `alloca_raw` in `emitter.rs` now hoists every stack slot into the function's entry block (constant O(1) frame). The `vm5`/`vm10` repros are genuinely non-terminating interpreter loops (verified: they now run forever without crashing); the repro harness kills any binary still running after 10s (`RUN_TIMEOUT_SECS`, `tests/repro_harness.rs`), recording `run=124` instead of hanging the suite. `vm.cnb` was reconciled to its documented program: its PROG bytes had decoded to DECC/op-3/ACCM instead of the documented JZC/ACCM/DECC, making it an accidental infinite loop; the encoding now matches the header and the fixture is `EXPECT_OK("vm", 120)`.

## 5. Native OS surfaces

- `Terminal.read_line() -> Result(Collections.String, Error)`
- `File.open`/`read`/`write`/`close`: linear `File` handle
- `Runtime.args() -> &[Collections.String]`
- Static linking only. No dynamic linking, no shared-library mode, ever — this is not configurable per-target. `Memory.allocate`/`deallocate`, `Terminal.print`/`read_line`, `File.*` lower directly to OS syscalls (`sys_mmap`/`sys_munmap`, `sys_write`/`sys_read`, `sys_open`/`sys_close` on x86_64/AArch64) rather than through libc. Where a target genuinely cannot avoid libc, link it statically (musl) — never dynamically, under any circumstance.
## 6. `build.cnb` project manifest

- Before designing the format: decide how a compile-time string constant works, since `Collections.String` is native, heap-backed, and constructed via an `impure` call — not usable as a `const` initializer as currently specified. Either define a distinct compile-time-only string representation for manifest fields, or resolve this via Milestone 1's literal-as-`&[U8]` representation and keep manifest fields as `&[U8]`, not `String`.
- CLI: `cinnabar build`, `cinnabar run`, `cinnabar test`, `cinnabar check`.

## 7. Diagnostic quality

- Multi-label Ariadne rendering: error site + relevant definition site in one diagnostic.
- Suggestion engine (dead code, unresolved-name matches): every suggestion is hedged ("possible match," "did you mean") regardless of source — cache-derived or purely structural. Never state inferred developer intent as fact; an inconclusive cache or ambiguous match always falls back to the neutral, non-presumptive message. No suggestion may point toward a local bandaid (suppress, stub, comment out) over a structurally correct fix.

## 8. Verification

- Type soundness: progress + preservation, checked against monomorphization, trait dispatch, nested sum-type destructuring.
- AST/type fuzzer generating random well-typed programs, checked that the typechecker/borrow checker never accepts an ill-typed or memory-unsafe one.
- Sanitizer gate in `pre_commit_check.sh`: all fixture binaries under UBSan, ASan, Valgrind. Zero UB, zero leaks, zero unhandled traps on every valid program. Since shipped binaries are static/syscall-direct, build a separate instrumented target for the sanitizer gate (dynamically linked against sanitizer runtimes, as UBSan/ASan require) rather than relaxing the static-only rule for shipped output. The sanitizer build is test infrastructure, not a release artifact.
- Tail-call optimization for genuinely self-tail-recursive functions only — this is a real, separate optimization, not a fix for Milestone 2's stack-depth problem. RESOLVED: a call in tail position (the direct value of a `return`) is marked `tail` in `emit_call`/`emit_native_call`/`emit_deferred_trait_call` (argument subexpressions are explicitly cleared, so `return x + f(y)` never marks `f`), and LLVM's tail-call elimination converts self-tail-recursion into O(1)-stack jumps at `-O2`. `tail_rec.cnb` (1M iterations, `EXPECT_OK("tail_rec", 64)`) locks it in. The O(1)-stack behavior for self-tail-recursion is now a documented runtime guarantee (MANIFESTO.md, Runtime Guarantees); the `opt default<O2>` step added to `assemble()` is what actually runs the elimination, since `llc -O2` alone is backend-only and never ran LLVM's module optimization pipeline.

## 9. Cinnabook and Mushlings

- `cinna burn`: local web server, static content, bundled at compile time, matches the exact installed compiler version.
- Mushlings: rustlings-style broken/diagnostic/fixed exercises. Source directly from the failure classes already found this session (dropped `pub`, mixed struct/enum body, unhandled `Result`/`Option`, discard patterns, unconsumed linear value, ambiguous returned borrow, compile-time-zero division) — each already has a real compiler diagnostic; use it verbatim rather than writing new exercise text from scratch.
