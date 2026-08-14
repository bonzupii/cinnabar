<!-- @lede -->

Six of the eight milestones in `ROADMAP.md` are complete; two are marked
partial. Below is what the language and its toolchain do today, what is still
being widened, and what comes after that.

<!-- @shipped-note -->

Every entry cites the milestone that shipped it

<!-- @shipped-more -->

The other six capabilities

<!-- @capabilities -->

<!--
  Every capability on this page, shipped and partial, in one block. Which
  group each belongs to and what order they appear in is decided in
  src/content/roadmap.ts, which finds each section by the slug of its heading
  — so rewording a heading means updating that file too.
-->

### Linear types

Native handles carry a consumption obligation the borrow checker tracks on every
path. Aliasing exclusivity, field-level partial moves and rejection of ambiguous
returned borrows come out of the same flow-sensitive analysis.

### O(1) call-stack recursion

Self-recursion must be in strict tail position, checked by the typechecker. LLVM
turns those calls into jumps at -O2, and the runtime stack guard is gone: no
per-entry checks, no getrlimit, no stack-overflow message.

### Division and modulo return Result

Euclidean semantics, so the remainder is never negative. A divisor the compiler
can prove is zero is a compile error whatever the numerator; a runtime zero is
Err(DivByZero).

### Direct system calls

Memory, Terminal and File emit the kernel entry point as inline assembly rather
than calling libc. Their fixtures compile to IR that declares no libc function at
all.

### Static, freestanding binaries

Programs link against a musl libc staged into the compiler at build time. The
output has no dynamic section and no dependency on the host's libc.

### Language server

cinnabar-lsp answers hover, go-to-definition, find-references and completion from
the facts the compiler already attached. It contains no second implementation of
name resolution or type inference.

### Fixed-width integers

U8 through U64, I8 through I64, and pointer-sized Usize and Isize. Int was
retired rather than kept as an alias, so no type has two spellings in a
diagnostic.

### String literals

Double-quoted, five escapes, no line spanning, type &[U8]. The borrow checker
learned that static data is an origin, which is what lets a function return a
literal without an untraceable loan.

### build.cnb manifest

The manifest is Cinnabar source, read back through the compiler's own front end
rather than scanned by a key=value splitter. A mistake in it is an ordinary
diagnostic pointing at the line.

### Definition-site diagnostic labels

A duplicate symbol labels the first declaration; an immutable assignment labels
the val binding; an unhandled Result labels the producing return type. Near-match
suggestions come from the resolver's own scope facts and are always hedged.

### Documentation and exercises

cinnabar burn serves version-pinned documentation locally. Mushlings ships eight
exercises, each sourced from a failure class with a real compiler diagnostic
quoted verbatim.

### Valgrind gate

Every valid program in the corpus runs under memcheck, through a second link mode
that keeps the host libc so there is an allocator to interpose on. Shipped
binaries are unaffected: still static, nostdlib, no-pie.

### Diagnostic quality

Definition-site labels and near-match suggestions have shipped. Widening that
treatment to the rest of the front end's error surface is the part still open.

### Verification

The memcheck gate is in place. Type soundness — progress and preservation — has
not been started, and cinnabar soundness reports formal_proof: false because it
counts what the front end accepted rather than proving anything.

<!-- @horizon -->

### Self-hosting

Cinnabar compiling itself, with the compiler becoming a Cinnabar-emitted binary
bound by every principle in the manifesto. It is a completeness test — it proves
the language can express a real compiler — and a hardening exercise. It is not a
gate: no feature above had to help get there in order to ship.

<!-- @progress -->

Two milestones are marked partial: the work has shipped in part and is still
being widened. Neither blocks anything above.

<!-- @horizon-note -->

A completeness test, not a gate

<!-- @source -->

Milestones are justified by general-purpose systems programming — kernels,
firmware, network stacks, runtimes and compilers — not by whether they help
rewrite the compiler in Cinnabar. `ROADMAP.md` is rendered below in full, at
build time.

<!-- @document -->

ROADMAP.md, in full
