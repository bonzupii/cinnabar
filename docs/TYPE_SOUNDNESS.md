# Type soundness evidence

`cinnabar soundness PATH` emits `target/soundness-evidence.json` after the complete front end accepts a program. The artifact identifies the compiler version and entry source, records successful resolution, typechecking, and borrow checking, and counts typed-arena facts including expressions, type nodes, generic instantiations, and trait dispatch records.

This is reproducible compiler evidence, not a formal proof. It makes the exact checked artifact inspectable without claiming that compiler acceptance proves the metatheory.

The Milestone 7 formalization must still define the core language's static and dynamic semantics and prove at least:

1. Progress: a well-typed non-final program can take a step.
2. Preservation: evaluation preserves the program's type.
3. Linearity preservation: evaluation neither duplicates nor silently loses a live linear value.
4. Borrow safety: evaluation cannot create simultaneous conflicting aliases or use a value after move.
5. Lowering correspondence: typed source operations map to LLVM operations with compatible representations and control flow.

The versioned JSON schema is intended as a bridge to that work: proof tooling can reject stale evidence, locate the checked entry, and compare the attributed-arena population used by the compiler run.
