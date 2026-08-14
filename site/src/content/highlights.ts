import {
  BorrowIcon,
  CheckIcon,
  CodegenIcon,
  DiagnosticIcon,
  FmtIcon,
  LinearIcon,
  RunIcon,
  StaticLinkIcon,
} from "@/components/brand/icons";

/*
 * The language highlights, taken from README.md's own list. The wording is the
 * repository's; only the icon pairing is chosen here, and each pairing uses the
 * icon from plate 07 whose meaning actually matches the claim.
 */

export type Highlight = {
  title: string;
  body: string;
  icon: typeof LinearIcon;
};

export const HIGHLIGHTS: readonly Highlight[] = [
  {
    title: "Linear resource management",
    body: "Native handles — Memory.Block, Vec(T), String, HashMap(K, V) — must be consumed exactly once on every path. No double-free, no use-after-move, no leaks, checked statically.",
    icon: LinearIcon,
  },
  {
    title: "No lifetime annotations",
    body: "Borrow scopes are flow-sensitive and inferred by the compiler. An ambiguous returned borrow is a compile error, resolved by restructuring the API, not by annotating.",
    icon: BorrowIcon,
  },
  {
    title: "No dereference operator",
    body: "There is no * and no ->. References are reached through field access, method calls and pattern matching; the compiler manages the indirection internally.",
    icon: FmtIcon,
  },
  {
    title: "Errors only, never warnings",
    body: "There is no lint severity and no #[allow]. A program either compiles cleanly or is rejected with a real diagnostic.",
    icon: DiagnosticIcon,
  },
  {
    title: "No panics reachable from user code",
    body: "Division, modulo and dynamic indexing return Result instead of trapping. Provable zero-division and out-of-range constant indices are compile-time errors instead.",
    icon: CheckIcon,
  },
  {
    title: "O(1) call-stack recursion",
    body: "Every self-recursive call must be in strict tail position. LLVM turns it into a jump, so there is no runtime stack guard and no stack-overflow crash.",
    icon: RunIcon,
  },
  {
    title: "Explicit everything",
    body: "val/var, pub, impure, try — and casing itself — are compiler-enforced grammar, not convention. A mis-cased identifier is a lexical error.",
    icon: CodegenIcon,
  },
  {
    title: "Static, freestanding binaries",
    body: "Every program links statically against a staged musl libc. No dynamic-linker dependency in the output binary, and no dependency on the host's libc.",
    icon: StaticLinkIcon,
  },
] as const;
