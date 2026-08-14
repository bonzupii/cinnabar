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
 * document is still reachable in full further down the page; this is the
 * summary that reads first.
 *
 * Everything here is drawn from a milestone the roadmap marks COMPLETE, or
 * from its "Resolved" section. Titles name the thing rather than describe it:
 * "Division and modulo", not "arithmetic that never traps".
 */

export type Capability = {
  title: string;
  detail: string;
  icon: typeof LinearIcon;
  /** Anchor into the rendered roadmap, for the reader who wants the detail. */
  anchor: string;
};

/**
 * The six a reader needs first — the properties that decide whether the
 * language is worth their time. Shown up front on the roadmap page.
 */
export const SHIPPED_LEAD: readonly Capability[] = [
  {
    title: "Linear types",
    detail:
      "Native handles carry a consumption obligation the borrow checker tracks on every path. Aliasing exclusivity, field-level partial moves and rejection of ambiguous returned borrows come out of the same flow-sensitive analysis.",
    icon: LinearIcon,
    anchor: "#resolved",
  },
  {
    title: "O(1) call-stack recursion",
    detail:
      "Self-recursion must be in strict tail position, checked by the typechecker. LLVM turns those calls into jumps at -O2, and the runtime stack guard is gone: no per-entry checks, no getrlimit, no stack-overflow message.",
    icon: RunIcon,
    anchor: "#resolved",
  },
  {
    title: "Division and modulo return Result",
    detail:
      "Euclidean semantics, so the remainder is never negative. A divisor the compiler can prove is zero is a compile error whatever the numerator; a runtime zero is Err(DivByZero).",
    icon: CheckIcon,
    anchor: "#resolved",
  },
  {
    title: "Direct system calls",
    detail:
      "Memory, Terminal and File emit the kernel entry point as inline assembly rather than calling libc. Their fixtures compile to IR that declares no libc function at all.",
    icon: StaticLinkIcon,
    anchor: "#milestone-4--native-os-surfaces-complete",
  },
  {
    title: "Static, freestanding binaries",
    detail:
      "Programs link against a musl libc staged into the compiler at build time. The output has no dynamic section and no dependency on the host's libc.",
    icon: CodegenIcon,
    anchor: "#milestone-4--native-os-surfaces-complete",
  },
  {
    title: "Language server",
    detail:
      "cinnabar-lsp answers hover, go-to-definition, find-references and completion from the facts the compiler already attached. It contains no second implementation of name resolution or type inference.",
    icon: BorrowIcon,
    anchor: "#resolved",
  },
] as const;

/** The rest of what has shipped. Folded away on the page by default. */
export const SHIPPED_REST: readonly Capability[] = [
  {
    title: "Fixed-width integers",
    detail:
      "U8 through U64, I8 through I64, and pointer-sized Usize and Isize. Int was retired rather than kept as an alias, so no type has two spellings in a diagnostic.",
    icon: CodegenIcon,
    anchor: "#milestone-1--fixed-width-integer-suite-complete",
  },
  {
    title: "String literals",
    detail:
      "Double-quoted, five escapes, no line spanning, type &[U8]. The borrow checker learned that static data is an origin, which is what lets a function return a literal without an untraceable loan.",
    icon: FmtIcon,
    anchor: "#milestone-2--string-literals-complete",
  },
  {
    title: "build.cnb manifest",
    detail:
      "The manifest is Cinnabar source, read back through the compiler's own front end rather than scanned by a key=value splitter. A mistake in it is an ordinary diagnostic pointing at the line.",
    icon: BuildIcon,
    anchor: "#milestone-5--buildcnb-project-manifest-complete",
  },
  {
    title: "Definition-site diagnostic labels",
    detail:
      "A duplicate symbol labels the first declaration; an immutable assignment labels the val binding; an unhandled Result labels the producing return type. Near-match suggestions come from the resolver's own scope facts and are always hedged.",
    icon: DiagnosticIcon,
    anchor: "#milestone-6--diagnostic-quality-partial",
  },
  {
    title: "Documentation and exercises",
    detail:
      "cinnabar burn serves version-pinned documentation locally. Mushlings ships eight exercises, each sourced from a failure class with a real compiler diagnostic quoted verbatim.",
    icon: DocIcon,
    anchor: "#milestone-7--cinnabook-and-mushlings-complete",
  },
  {
    title: "Valgrind gate",
    detail:
      "Every valid program in the corpus runs under memcheck, through a second link mode that keeps the host libc so there is an allocator to interpose on. Shipped binaries are unaffected: still static, nostdlib, no-pie.",
    icon: TestIcon,
    anchor: "#milestone-8--verification-partial",
  },
] as const;

/** What the language and its toolchain do today. */
export const SHIPPED: readonly Capability[] = [
  ...SHIPPED_LEAD,
  ...SHIPPED_REST,
] as const;

/** Work the roadmap marks PARTIAL — shipped in part, still open in part. */
export const IN_PROGRESS: readonly Capability[] = [
  {
    title: "Diagnostic quality",
    detail:
      "Definition-site labels and near-match suggestions have shipped. Widening that treatment to the rest of the front end's error surface is the part still open.",
    icon: DiagnosticIcon,
    anchor: "#milestone-6--diagnostic-quality-partial",
  },
  {
    title: "Verification",
    detail:
      "The memcheck gate is in place. Type soundness — progress and preservation — has not been started, and cinnabar soundness reports formal_proof: false because it counts what the front end accepted rather than proving anything.",
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
    "Cinnabar compiling itself, with the compiler becoming a Cinnabar-emitted binary bound by every principle in the manifesto. It is a completeness test — it proves the language can express a real compiler — and a hardening exercise. It is not a gate: no feature above had to help get there in order to ship.",
  anchor: "#self-hosting-a-goal-not-a-gate",
} as const;
