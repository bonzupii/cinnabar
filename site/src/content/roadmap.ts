import {
  BorrowIcon,
  BuildIcon,
  CheckIcon,
  CodegenIcon,
  DiagnosticIcon,
  DocIcon,
  FmtIcon,
  LinearIcon,
  RunIcon,
  StaticLinkIcon,
  TestIcon,
} from "@/components/brand/icons";

/*
 * The roadmap, stated as capabilities rather than as a list of milestones.
 *
 * ROADMAP.md is organised the way the work was done — eight numbered
 * milestones, each with its decisions and its verification. That is the right
 * shape for the people doing it and the wrong shape for someone deciding
 * whether to use the language, who wants to know what it can do today. The
 * document is still rendered in full further down the page; this is the
 * summary that reads first.
 *
 * Everything here is drawn from a milestone the roadmap marks COMPLETE, or
 * from its "Resolved" section.
 */

export type Capability = {
  title: string;
  detail: string;
  icon: typeof LinearIcon;
  /** Anchor into the rendered roadmap, for the reader who wants the detail. */
  anchor: string;
};

/** What the language and its toolchain do today. */
export const SHIPPED: readonly Capability[] = [
  {
    title: "Linear types, checked on every path",
    detail:
      "Native handles must be consumed exactly once. Aliasing exclusivity, field-level partial moves, and rejection of ambiguous returned borrows all fall out of one flow-sensitive analysis.",
    icon: LinearIcon,
    anchor: "#resolved",
  },
  {
    title: "The full fixed-width integer grid",
    detail:
      "U8 through U64, I8 through I64, and pointer-sized Usize and Isize. Int was retired rather than kept as an alias, so no type has two spellings in a diagnostic.",
    icon: CodegenIcon,
    anchor: "#milestone-1--fixed-width-integer-suite-complete",
  },
  {
    title: "O(1) call-stack recursion",
    detail:
      "Self-recursion must be in strict tail position, enforced at compile time. LLVM turns it into a jump, and the runtime stack guard is gone entirely — no per-entry checks, no stack-overflow message.",
    icon: RunIcon,
    anchor: "#resolved",
  },
  {
    title: "Arithmetic that never traps",
    detail:
      "Division and modulo return Result and use Euclidean semantics, so the remainder is never negative. A provably-zero divisor is a compile error, whatever the numerator.",
    icon: CheckIcon,
    anchor: "#resolved",
  },
  {
    title: "String literals, fixed at lex time",
    detail:
      "Double-quoted, with exactly five escapes and no line spanning. The borrow checker learned that static data is an origin, which is what lets a function return a literal without an untraceable loan.",
    icon: FmtIcon,
    anchor: "#milestone-2--string-literals-complete",
  },
  {
    title: "Direct system calls",
    detail:
      "Memory, Terminal and File issue the kernel entry point as inline assembly. Their fixtures compile to IR that declares no libc function at all.",
    icon: StaticLinkIcon,
    anchor: "#milestone-4--native-os-surfaces-complete",
  },
  {
    title: "A manifest the compiler parses",
    detail:
      "build.cnb is Cinnabar source, read back through the front end rather than scanned by a key=value splitter — so a mistake in it is an ordinary diagnostic pointing at the line.",
    icon: BuildIcon,
    anchor: "#milestone-5--buildcnb-project-manifest-complete",
  },
  {
    title: "Diagnostics that name the cause",
    detail:
      "Definition-site labels render by default, and near-match suggestions come from the resolver's own scope facts. Every suggestion is hedged, and an ambiguous match stays silent.",
    icon: DiagnosticIcon,
    anchor: "#milestone-6--diagnostic-quality-partial",
  },
  {
    title: "Documentation and exercises",
    detail:
      "cinnabar burn serves version-pinned documentation locally, and eight Mushlings exercises each teach through a real compiler diagnostic quoted verbatim.",
    icon: DocIcon,
    anchor: "#milestone-7--cinnabook-and-mushlings-complete",
  },
  {
    title: "A memory-checker gate",
    detail:
      "Every valid program in the corpus runs under Valgrind, through a second link mode that keeps the host libc — the shipped binary stays static, nostdlib and no-pie.",
    icon: TestIcon,
    anchor: "#milestone-8--verification-partial",
  },
  {
    title: "A language server over the same pipeline",
    detail:
      "Hover, go-to-definition, find-references and completion are all read from the facts the compiler already attached. There is no second implementation of name resolution.",
    icon: BorrowIcon,
    anchor: "#resolved",
  },
  {
    title: "Static, freestanding binaries",
    detail:
      "Every program links against a musl libc staged into the compiler at build time. The output has no dynamic section and no dependency on the host's libc.",
    icon: CodegenIcon,
    anchor: "#milestone-4--native-os-surfaces-complete",
  },
] as const;

/** Work the roadmap marks PARTIAL — shipped in part, still open in part. */
export const IN_PROGRESS: readonly Capability[] = [
  {
    title: "Diagnostic quality",
    detail:
      "Definition-site labels and suggestions have shipped. What remains is widening that treatment to the rest of the front end's error surface.",
    icon: DiagnosticIcon,
    anchor: "#milestone-6--diagnostic-quality-partial",
  },
  {
    title: "Verification",
    detail:
      "The memory-checker gate is in place. Proving the absence of undefined behaviour, rather than instrumenting for it, is the part still open.",
    icon: TestIcon,
    anchor: "#milestone-8--verification-partial",
  },
] as const;

/**
 * Milestone tallies, as ROADMAP.md marks them: six COMPLETE and two PARTIAL.
 * Stated here rather than derived, because the capability list above is
 * organised by what the language does rather than by which milestone shipped
 * it, and no longer carries a status per entry.
 */
export const MILESTONE_TALLY = { complete: 6, total: 8 } as const;

/** The next horizon, and what it is and is not. */
export const HORIZON = {
  title: "Self-hosting",
  detail:
    "Cinnabar compiling itself, and the compiler becoming a Cinnabar-emitted binary bound by every principle in the manifesto. It is a completeness test — it proves the language can express a real compiler — and a hardening exercise. It is deliberately not a gate: no feature above had to help get there in order to ship.",
  anchor: "#self-hosting-a-goal-not-a-gate",
} as const;
