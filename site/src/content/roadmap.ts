/*
 * Milestone status, summarised from ROADMAP.md.
 *
 * The `status` values are the roadmap's own parenthesised labels — COMPLETE
 * and PARTIAL — not a re-judgement made here. Self-hosting is deliberately not
 * a milestone: the roadmap calls it "a goal, not a gate".
 */

export type MilestoneStatus = "complete" | "partial" | "goal";

export type Milestone = {
  number: string;
  title: string;
  status: MilestoneStatus;
  summary: string;
  /** Anchor on the roadmap page, produced by rehype-slug from the heading. */
  anchor: string;
};

export const STATUS_LABEL: Record<MilestoneStatus, string> = {
  complete: "Complete",
  partial: "Partial",
  goal: "Goal",
};

export const MILESTONES: readonly Milestone[] = [
  {
    number: "01",
    title: "Fixed-width integer suite",
    status: "complete",
    summary:
      "The ten fixed-width types — U8/U16/U32/U64, I8/I16/I32/I64, Usize and Isize — with Int retired rather than kept as an alias, so no type has two spellings in any diagnostic.",
    anchor: "#milestone-1--fixed-width-integer-suite-complete",
  },
  {
    number: "02",
    title: "String literals",
    status: "complete",
    summary:
      "Double-quoted literals with exactly five escapes, fully determined at lex time. The borrow checker learned that static data is an origin, and comparison was stated as a scalar operation.",
    anchor: "#milestone-2--string-literals-complete",
  },
  {
    number: "03",
    title: "Native memory bugs",
    status: "complete",
    summary:
      "Two reported defects investigated together: the access-width report was not real, and the root cause of the second was found and fixed. Also settled why Valgrind and ASan cannot be the gate here.",
    anchor: "#milestone-3--native-memory-bugs-complete",
  },
  {
    number: "04",
    title: "Native OS surfaces",
    status: "complete",
    summary:
      "Memory, Terminal and File issue direct system calls as inline assembly rather than calling libc — their fixtures compile to IR that declares no libc function at all.",
    anchor: "#milestone-4--native-os-surfaces-complete",
  },
  {
    number: "05",
    title: "build.cnb project manifest",
    status: "complete",
    summary:
      "The manifest is Cinnabar source, parsed by the compiler's own front end rather than scanned by a key=value splitter, so a mistake in it is an ordinary diagnostic.",
    anchor: "#milestone-5--buildcnb-project-manifest-complete",
  },
  {
    number: "06",
    title: "Diagnostic quality",
    status: "partial",
    summary:
      "Definition-site labels rendered by default, and near-match suggestions drawn from the resolver's own scope facts — every one hedged, and silent on a tie. Dead code became a rejection.",
    anchor: "#milestone-6--diagnostic-quality-partial",
  },
  {
    number: "07",
    title: "Cinnabook and Mushlings",
    status: "complete",
    summary:
      "`cinnabar burn` serves version-pinned documentation locally, and eight Mushlings exercises each teach through a real compiler diagnostic quoted verbatim.",
    anchor: "#milestone-7--cinnabook-and-mushlings-complete",
  },
  {
    number: "08",
    title: "Verification",
    status: "partial",
    summary:
      "Every valid program in the corpus runs under Valgrind memcheck through a second, instrumented link mode — the shipped static-only rule is not relaxed for it.",
    anchor: "#milestone-8--verification-partial",
  },
  {
    number: "—",
    title: "Self-hosting",
    status: "goal",
    summary:
      "Cinnabar compiling itself is a completeness test and a hardening exercise — it proves the language can express a real compiler. It is not a criterion any feature above must satisfy to ship.",
    anchor: "#self-hosting-a-goal-not-a-gate",
  },
] as const;
