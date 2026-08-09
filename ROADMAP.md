# Cinnabar — Next Milestones

## 1. String literals

- Lexer: `TOK_STRING`, double-quoted, standard escapes (`\n`, `\t`, `\0`, `\"`, `\\`).
- Type: literal evaluates to `&[U8]`, a static `.rodata` byte array + `{ ptr, len }` slice. No heap allocation, no lifetime.
- UTF-8 validity of a literal is known at compile time — validate at parse time, not via a runtime `Result`.
- `Terminal.print`/`print_line` accept `&[U8]` directly, alongside `&Collections.String`.
- No dependency on anything below. Ships first.

## 2. Recursion depth guard

- Runtime stack-depth counter threaded through function calls; exceeding a limit raises a catchable runtime error instead of an OS SIGSEGV.
- This is not tail-call optimization and is not fixed by it — `recurse`'s `return recurse(n - 1) + 1` is not in tail position, so `musttail` doesn't apply here. Track as a separate fix.
- Promote `mem_probe.cnb` from `RECORD_ONLY` to `EXPECT_OK` only after this lands, not after TCO.

## 3. Euclidean division/modulo

- Constant folding (`typecheck.rs`): `.div_euclid()` / `.rem_euclid()` in place of `wrapping_div`/`wrapping_rem`.
- Codegen (`emitter.rs`): non-negative remainder for signed integers, `0 <= r < |divisor|`.
- `div_zero_const.cnb` (compile-time-provable-zero divisor) must be a hard compile error; runtime-unknown divisor returns `Result(T, DivError)`.

## 4. Native memory bugs (from the notes-file triage)

- `Memory.write_u8` overreads the allocation by 7 bytes (valgrind-confirmed). Fix the width of the store — this is almost certainly also the root cause of the `deallocate`-frees-garbage-pointer bug; re-test that one after this fix, don't fix it independently.
- Backward-jump `while` loop crash (`vm10`-class repro): one untested hypothesis remains from the bisection log — `continue` nested two levels deep. Isolate that before assuming this is an LLVM `-O2` miscompilation; test `-O0` on the same repro either way to rule the optimizer in or out.

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
- Tail-call optimization for genuinely self-tail-recursive functions only — this is a real, separate optimization, not a fix for Milestone 2's stack-depth problem.

## 9. Cinnabook and Mushlings

- `cinna burn`: local web server, static content, bundled at compile time, matches the exact installed compiler version.
- Mushlings: rustlings-style broken/diagnostic/fixed exercises. Source directly from the failure classes already found this session (dropped `pub`, mixed struct/enum body, unhandled `Result`/`Option`, discard patterns, unconsumed linear value, ambiguous returned borrow, compile-time-zero division) — each already has a real compiler diagnostic; use it verbatim rather than writing new exercise text from scratch.
