# The Cinnabar Manifesto

Cinnabar is a systems programming language designed for building compilers, runtimes, and low-level infrastructure. It exists because existing systems languages force an unacceptable trade-off: either accept hidden complexity (implicit lifetimes, dereferencing, warnings-as-errors theater) or abandon safety. Cinnabar rejects both.

The language is named after Cantharellus cinnabarinus, the Cinnabar Chanterelle mushroom. Not an acronym. Not a committee compromise. A name from the trail.

## Core Principles

### 1. No Information Loss
Every fact the compiler computes is attached to the tree once and consumed downstream. Nothing is recomputed. The resolver attaches symbol ids; the typechecker attaches type keys; codegen reads both. If a stage needs a fact, it reads the attachment — it never re-derives it. This is not an optimization. It is the architectural contract.

### 2. No Hidden Control Flow
There is no implicit dereferencing. There are no lifetime annotations. There is no operator overloading. There are no macros. Every transformation the compiler performs is visible in the source or explicitly declared. If you cannot see what the code does by reading it, the language has failed.

### 3. Errors Only, Never Warnings
The compiler emits errors or silence. There is no warning severity. There is no `#[allow]`. There is no lint configuration. If something is wrong, it is an error. If it is not an error, it is valid. The middle ground of "probably wrong but we'll let you decide" is a design flaw that produces codebases full of suppressed diagnostics and forgotten intent.

### 4. Explicit Over Implicit, Always
`val` and `var` are distinct keywords. Mutability is declared, never inferred.
`pub` is required for visibility. Everything is private by default.
`impure` is required for side effects. Everything is pure by default. A function not declared `impure` cannot call an `impure` function or native; invoking an impure callee from a pure context is a compile error.
`nat` is required for native declarations — a function or type implemented outside Cinnabar, with no body in Cinnabar source. Native handles are opaque; user code never sees pointers.
`try` is explicit propagation. No implicit unwrapping.
Imports are explicit `use` statements with optional `as` aliases. No glob imports. No implicit prelude beyond builtins.
Return types are always declared. No inference of function signatures.

### 5. No Lifetime Annotations
Borrow scopes are flow-sensitive and determined by the compiler. Returned borrows must be unambiguous — if the compiler cannot determine which input a returned reference derives from, it is a compile error. The programmer resolves this by restructuring the API, not by annotating lifetimes. Lifetime annotation syntax (`'a`) does not exist in the language.

### 6. No Dereference Operator
There is no `*ptr`. There is no `->`. References are accessed through field access, method calls, and pattern matching. The compiler manages indirection internally. This eliminates an entire class of memory safety bugs and makes the borrow checker's job tractable without annotations.

### 7. Linear Types for Resource Management
Native handles (`Memory.Block`, `Collections.Vec`, `Collections.String`, `Collections.HashMap`) are linear. They must be consumed exactly once on every execution path. They cannot be copied. They cannot be used after move. They cannot be implicitly dropped. Every allocation has an explicit corresponding deallocation. This is enforced at compile time.

Native containers are linear regardless of their element type. `Vec(T)` and `HashMap(K, V)` own heap storage that requires an explicit corresponding free, independent of what they hold; their linearity does not derive from their element. Inserting a value into a container (`vec_push`, `hash_map_insert`) moves the element: a linear element is consumed by the insertion and cannot be reused afterward.

Type parameters are conservatively linear. A generic function's type parameter has no linearity bound in the grammar, so its instantiation is unknown at definition time; a value of a type-parameter type must therefore be consumed exactly once on every execution path, exactly as a native handle. A generic body that moves a type-parameter value is subject to the same consumption checks as one that moves a `Block`.

Linearity is container-aware for user-defined aggregates: a struct, enum, or array whose fields, payloads, or elements are linear is itself linear, and moving a linear value into a struct constructor consumes it.

### 8. Casing Is Syntax
Casing conventions are enforced by the lexer, not by lints:
`snake_case`: local bindings, functions, parameters
`PascalCase`: types, traits, modules, enum variants
`SCREAMING_SNAKE_CASE`: constants
A violation is a lexical error. The parser never sees a mis-cased identifier. This is not a style guide. It is grammar.

### 9. Comments Are Structured
Four comment forms, each with distinct semantics:
`#` — ordinary comment, discarded
`#!` — documentation comment, attached to the following item
`#| ... |#` — block comment, discarded
`#!| ... |#` — block documentation comment, attached to the following item
Block comments may not nest (nesting is a lexical error). Trailing comments may follow any line of code. These rules are enforced by the lexer.

### 10. The Compiler Is a Pure Pipeline Over a Flat Arena
The compiler operates on a flat node arena with fixed-width records rather than a heap of boxed/reference-counted tree nodes. Trees are integer ids. Each stage is a pure function (or set of functions) that reads the arena and writes attachments — no stage retains internal mutable state past its own return. This is exactly the data structure a Cinnabar program would use to represent itself. When the language is self-hosted, the compiler's architecture does not change — only the host language does.

### 11. The Crucible Rule
If a program compiles, it runs without crashing or panicking. Every runtime failure mode that can be moved to compile time must be: bounds are `Result` errors on the native surface and on array/slice indexing, never traps; division/modulo by a compile-time-provable zero is a compile-time error; recursion must not be able to exhaust the stack. The compiler is judged by the binaries it produces — disassembled, executed, and checked for memory safety — not by whether the frontend accepts a program.

### 12. Self-Hosting Discipline
Once Cinnabar compiles itself, the compiler is a Cinnabar-emitted binary and is bound by every principle above, including the Crucible Rule — it is not exempt as tooling. Where a compiler feature is temporarily unrepresentable in Cinnabar itself, the boundary is marked explicitly with a native declaration and the temporary status is visible, not hidden; the feature is rewritten in Cinnabar once the language can express it.

## Language Surface (Authoritative)

This section is normative. If a fixture, harness, or existing program contradicts it, the fixture is wrong. Never treat an existing `.cnb` file as authoritative.

### Syntax
Statements are newline-separated. There is no `;` statement terminator; a semicolon is a syntax error.

Multi-line expressions are valid: a const initializer, argument list, struct literal, or any expression may span lines. The parser handles continuation across newlines inside delimiters (`()`, `[]`, `{}`).

Comments: `#`, `#!`, `#| |#`, `#!| |#`. Block comments do not nest. Trailing comments may follow any line.

### Control Flow
`if` / `elif` / `else` are all supported. `elif` is a single keyword, not `else if`. An `if` without `else`, an `if` with `else` but no `elif`, and any combination are valid.

`while` loops support `break` and `continue`, valid at any nesting depth inside the loop body.

`match` arms are single-expression bodies separated by newlines. No semicolons. Match is exhaustive — every variant, array length, and rest pattern must be covered; missing coverage is a compile error.

The program entry point is `main`. It must return `Unit`, a builtin integer scalar (`Int`, `U8`, `U32`, `Usize`), or an exit-status enum. An exit-status enum's first declared variant denotes success (exit code 0), its second denotes failure (exit code 1), and it may declare one further variant carrying an `Int` payload used as the process exit code. Any other `main` return type is a compile error.

### Operators
Arithmetic: `+`, `-`, `*`, `/`, `%`. Modulo has the same precedence as `*` and `/`.
Bitwise: `&`, `|`, `^`, `<<`, `>>`.
Comparison: `==`, `!=`, `<`, `>`, `<=`, `>=`.
Logical: `&&`, `||`.

All operators require operands of the same type. No implicit widening.

Division `/` and modulo `%` return `Result(T, DivError)`: any compile-time-provable-zero divisor — a literal, a folded const reference, or any arithmetic combination of them (`N / 0`, `5 / 0`, `x / (3 - 3)`) — is a compile-time error, whatever the numerator. A runtime zero produces `Err(DivByZero)`, handled with `try` or `match` like every other fallible operation. Division never traps and is never undefined behavior.

`/` and `%` use Euclidean division: the remainder is always non-negative (`0 <= r < abs(divisor)`), regardless of operand signs. This is deliberate — it is the only common convention where the remainder's sign never depends on either operand's sign.

### Indexing
`arr[i]`, `&arr[i]`, and `&mut arr[i]` index fixed-size arrays `[T; N]` and slices `&[T]`. The index must be `Usize`.

A compile-time-constant index against a fixed-size array is checked at compile time: an out-of-range constant (`i < 0` or `i >= N`) is a hard compile error; an in-range constant evaluates directly to the element type `T` (or `&T` / `&mut T` when borrowed) with no `Result` wrapper and no runtime bounds check, because safety is proven statically.

Any runtime-computed index, and every index into a slice (whose length is dynamic), evaluates to `Result(T, IndexError)` — or `Result(&T, IndexError)` / `Result(&mut T, IndexError)` when borrowed — carrying `IndexOutOfBounds(index, length)` on failure, handled with `try` or `match` like every other fallible operation.

Indexing a linear-element array or slice by value is a compile error ("cannot move linear element out of array by index: borrow with & or &mut instead"): an indexed element is never moved out of its container, only read or borrowed.

### Assignment
`x = value` assigns a mutable local (`var`); assigning to a `val` or a const is a compile error.

Field assignment `target.field = value` (and deeper chains `a.b.c = value`) is valid whenever `target` is a mutable local `var` or has type `&mut T`. The target expression is a place, not a value: `pt.x = 5`, `ref.x = 5` for a `&mut Point`, and `(try &mut arr[i]).x = 42` all write in place, through the borrow. There is no dereference operator; the compiler manages the indirection internally.

Writing through a shared `&T` reference is a hard compile error ("cannot assign to field 'x' through shared reference: assignment requires &mut"): mutation requires an exclusive borrow.

Reassigning an effectively-Live linear field without consuming the previous handle is a hard compile error ("linear value 'a.b' is reassigned without being consumed"). Consuming the field first (a move into a call) transitions it to Moved, after which assignment re-initializes it and restores its moved-out ancestors.

### Types
Compiler builtins: `Unit`, `Result(T, E)`, `Option(T)`, `IndexError`, `Bool`, `Int`, `U8`, `U32`, `Usize`. They are always available; no program may declare them.

Fixed-size arrays `[T; N]`: indexed with `arr[i]`, `&arr[i]`, and `&mut arr[i]` (see Indexing), and destructured with `match arr [a, b, c] => ...` and rest patterns. Array length is always statically known, so a constant index is proven at compile time.

Slices `&[T]`: produced by `vec_view` or by the sanctioned coercion `&[T; N]` → `&[T]`.

References: `&T` (shared), `&mut T` (exclusive mutable).

Struct literals must initialize every declared field; omitting a field is a compile error. There is no default or partial initialization.

Enum variant constructors require exactly their declared payload. Using a payload-carrying variant with no payload, or with the wrong number of values, is a compile error.

### Linear Types
`Memory.Block`, `Collections.Vec(T)`, `Collections.String`, `Collections.HashMap(K, V)` are linear. `Vec(T)` and `HashMap(K, V)` are linear regardless of their element type; they own heap storage requiring an explicit free.

Linear values must be consumed exactly once on every execution path.

Type parameters are conservatively linear: a value of a generic type-parameter type must be consumed exactly once on every path, because its instantiation is unknown at definition time.

Inserting a value into a container (`vec_push`, `hash_map_insert`) moves it; a linear element is consumed by the insertion.

Linear values may be stored in structs; moving a linear value into a struct constructor consumes it.

Error paths must free linear values before returning.

### Visibility
Private by default. `pub` exposes.
`pub` on a local `val`/`var` is a compile error.
`pub` inside a private `mod` is legal but not externally reachable until the enclosing `mod` becomes `pub`.

Struct fields have independent visibility. A field is private by default; `pub` on the field exposes it. Reading or writing a field from outside its declaring module requires the field to be `pub`.

### Generics
Type constructors: `Vec(T)`, `Result(T, E)`.
Function type parameters: `fun f<T>(...)`.
Explicit instantiation: `f[U8]()`.
Trait bounds: `fun f<T: Checksum>(...)`.
Return-type-only type parameters are inferred from call-site context.

A type parameter is conservatively linear (see Linear Types).

At most one `impl` of a given trait may exist for a given type. A duplicate `impl Trait for Type` is a compile error.

### Runtime Guarantees
Recursion cannot exhaust the stack. Every compiled function checks, at entry, how much process stack it has consumed against the process's own `RLIMIT_STACK` soft limit, measured once in the real `main` wrapper's entry and reduced by a guard margin. Past the limit, the binary writes `Cinnabar: stack overflow` to stderr and exits with status 70 (`EX_SOFTWARE`) — a defined, observable failure — instead of dying with the OS's SIGSEGV on the guard page. An unlimited soft limit (`ulimit -s unlimited`, `RLIM_INFINITY`) is clamped to the POSIX default main-thread limit (8 MiB) so the guard is never silently disabled; a failed `getrlimit` uses the same default.

Self-tail-recursion runs in O(1) stack. A call that is the direct value of a `return` statement is a tail call: codegen marks it `tail` and LLVM's tail-call elimination turns self-tail-recursive calls into jumps at `-O2`, so a self-recursive function's depth does not consume stack. Marking is precise — argument subexpressions are never marked, so `return x + f(y)` does not treat `f` as a tail call. Non-tail recursion is bounded by the recursion guard above.

## What Is NOT Cinnabar
Semicolons as statement separators
User-declared `Unit`, `Result`, `Option`, or `IndexError` (they are builtins)
Panic-based error handling, exceptions, null, undefined behavior as a language feature
Everything listed under Anti-Principles below

## Language Goals

### Self-Hosting
Cinnabar must be able to compile itself. The language's own data structures, control flow, and resource management patterns must be sufficient to express a complete compiler frontend and codegen backend. Every language feature exists because the compiler needs it.

### Systems Programming Without Unsafety
Memory safety without garbage collection. Resource safety without RAII magic. Concurrency safety without mutexes (message passing and linear handles). The language must be suitable for writing kernels, embedded firmware, network stacks, and compilers — domains where GC pauses, implicit allocations, and runtime overhead are unacceptable.

### Compile-Time Correctness
Every invariant that can be checked at compile time must be checked at compile time. Exhaustive matching. Linear consumption. Borrow exclusivity. Visibility. Casing. Unused imports. Unhandled `Result`/`Option` values. Constant division by zero. Type mismatches. Effect purity. If the compiler accepts a program, the program satisfies all stated invariants — and, by the Crucible Rule, runs without crashing. Runtime checks exist only for genuinely dynamic conditions (bounds checking on native surfaces, UTF-8 validation on string construction).

### Minimal Surface Area
Every feature must justify its existence against the self-hosting requirement. Features that exist only for ergonomics, convention, or compatibility with other languages are rejected. The language is small because the problem domain demands precision, not expressiveness.

### Honest Diagnostics
Every diagnostic points to a real source location, or, where a fact genuinely has no source origin (a builtin declaration, a compiler-synthesized wrapper), says so explicitly rather than fabricating a placeholder location. Internal compiler failures carry no fabricated span. Toolchain failures name the tool and its exit status. The diagnostic model never invents information.

### Portability Through Simplicity
The compiler targets a small, auditable dependency graph and avoids abstracting away the machine: no proc macros, no build scripts that generate code, no plugin systems. The native surface favors direct, minimal system interfaces over broad standard-library abstractions.

## Anti-Principles (Things Cinnabar Will Never Have)
Lifetime annotations (`'a`, `<'a>`, etc.)
Dereference operators (`*`, `->`)
Warning severities or lint configuration
Macros or metaprogramming
Operator overloading
Implicit conversions or coercions (other than the single sanctioned `&[T; N]` → `&[T]` slice coercion)
Glob imports or implicit preludes
Garbage collection or reference counting
Mutexes or shared mutable state primitives
Trait objects or dynamic dispatch
Async/await or coroutine syntax
Null or nil values
Exceptions or panic-based error handling
Undefined behavior as a language feature
Style guides enforced separately from grammar

These are not features we haven't gotten to. They are features we have explicitly rejected. Adding any of them would violate the principles above.
