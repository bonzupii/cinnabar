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
 * What is here is which capabilities there are, which group each belongs to,
 * what order they appear in, the icon each is drawn with, and where in the
 * rendered document each links to. The titles and the descriptions are prose
 * and live in src/app/roadmap/content.md, in the `@capabilities` block, keyed
 * by the slug of the heading.
 *
 * Everything in that block is drawn from a milestone the roadmap marks
 * COMPLETE, or from its "Resolved" section. Titles name the thing rather than
 * describe it: "Division and modulo", not "arithmetic that never traps".
 */

export type Capability = {
  /** Keys this capability's `###` section in the roadmap route's content.md. */
  slug: string;
  icon: typeof LinearIcon;
  /** Anchor into the rendered roadmap, for the reader who wants the detail. */
  anchor: string;
};

/**
 * The six a reader needs first — the properties that decide whether the
 * language is worth their time. Shown up front on the roadmap page.
 */
export const SHIPPED_LEAD: readonly Capability[] = [
  { slug: "linear-types", icon: LinearIcon, anchor: "#resolved" },
  { slug: "o1-call-stack-recursion", icon: RunIcon, anchor: "#resolved" },
  {
    slug: "division-and-modulo-return-result",
    icon: CheckIcon,
    anchor: "#resolved",
  },
  {
    slug: "direct-system-calls",
    icon: StaticLinkIcon,
    anchor: "#milestone-4--native-os-surfaces-complete",
  },
  {
    slug: "static-freestanding-binaries",
    icon: CodegenIcon,
    anchor: "#milestone-4--native-os-surfaces-complete",
  },
  { slug: "language-server", icon: BorrowIcon, anchor: "#resolved" },
] as const;

/** The rest of what has shipped. Folded away on the page by default. */
export const SHIPPED_REST: readonly Capability[] = [
  {
    slug: "fixed-width-integers",
    icon: CodegenIcon,
    anchor: "#milestone-1--fixed-width-integer-suite-complete",
  },
  {
    slug: "string-literals",
    icon: FmtIcon,
    anchor: "#milestone-2--string-literals-complete",
  },
  {
    slug: "buildcnb-manifest",
    icon: BuildIcon,
    anchor: "#milestone-5--buildcnb-project-manifest-complete",
  },
  {
    slug: "definition-site-diagnostic-labels",
    icon: DiagnosticIcon,
    anchor: "#milestone-6--diagnostic-quality-partial",
  },
  {
    slug: "documentation-and-exercises",
    icon: DocIcon,
    anchor: "#milestone-7--cinnabook-and-mushlings-complete",
  },
  {
    slug: "valgrind-gate",
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
    slug: "diagnostic-quality",
    icon: DiagnosticIcon,
    anchor: "#milestone-6--diagnostic-quality-partial",
  },
  {
    slug: "verification",
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

/** The next horizon. Its title and reasoning are in the `@horizon` block. */
export const HORIZON = {
  slug: "self-hosting",
  anchor: "#self-hosting-a-goal-not-a-gate",
} as const;
