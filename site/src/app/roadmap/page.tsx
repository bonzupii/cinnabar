import type { Metadata } from "next";
import DocBody from "@/components/DocBody";
import PageHeader from "@/components/PageHeader";
import SectionHeading from "@/components/SectionHeading";
import Reveal from "@/components/Reveal";
import { Panel, SourceNote, Stat } from "@/components/ui";
import { CheckIcon, DiagnosticIcon, RunIcon } from "@/components/brand/icons";
import { MILESTONES, STATUS_LABEL, type MilestoneStatus } from "@/content/roadmap";
import { readPageContent } from "@/lib/page-content";
import { linkRepoFile, readRepoDoc } from "@/lib/repo-docs";

export const metadata: Metadata = {
  title: "Roadmap",
  description:
    "What is resolved and what is planned in Cinnabar: eight milestones, their status, and why self-hosting is a goal rather than a gate.",
  alternates: { canonical: "/roadmap/" },
};

/** Social image copy, consumed by ./opengraph-image.tsx. */
export const og = {
  eyebrow: "Project status",
  title: "Resolved, and planned.",
  description:
    "Eight milestones and their status, and why self-hosting is a completeness test rather than a gate.",
  alt: "Cinnabar social card — the project status.",
};

/*
 * Status is carried by a shape as well as a colour. The board allows one
 * accent, so "partial" cannot simply be a second hue — and colour alone would
 * not carry the distinction for a reader who cannot see it, which is why each
 * state also has its own icon and fill treatment.
 */
const STATUS_STYLE: Record<MilestoneStatus, { chip: string; node: string }> = {
  complete: { chip: "bg-cinnabar text-on-cinnabar", node: "bg-cinnabar border-cinnabar" },
  partial: {
    chip: "border border-cinnabar text-cinnabar-text",
    node: "border-cinnabar bg-ground",
  },
  goal: {
    chip: "border border-hairline-strong text-label",
    node: "border-hairline-strong bg-ground",
  },
};

const STATUS_ICON = {
  complete: CheckIcon,
  partial: RunIcon,
  goal: DiagnosticIcon,
} as const;

const SUMMARY_STATES = [
  ["complete", "Complete"],
  ["partial", "Partial"],
  ["goal", "Long-term goal"],
] as const;

export default async function RoadmapPage() {
  const [document, content] = await Promise.all([
    readRepoDoc("ROADMAP.md"),
    readPageContent("roadmap"),
  ]);

  const counts = MILESTONES.reduce<Record<string, number>>((totals, milestone) => {
    totals[milestone.status] = (totals[milestone.status] ?? 0) + 1;
    return totals;
  }, {});

  return (
    <article className="pb-28">
      <PageHeader
        section="Roadmap"
        note="A milestone is done only when fixtures, gate and spec agree"
        icon={CheckIcon}
        title="Resolved, and planned."
        lede={content.block("lede")}
      />

      {/* A count per state, so the shape of the project is legible at a glance. */}
      <section className="mx-auto max-w-[1400px] px-6 pt-14 sm:px-10">
        <div className="rule-grid grid grid-cols-2 sm:grid-cols-4">
          {SUMMARY_STATES.map(([status, label]) => (
            <Stat key={status} value={counts[status] ?? 0} label={label} />
          ))}
          <Panel className="justify-center gap-2 bg-ground p-6">
            <span className="text-secondary text-[13px] leading-[1.55] text-pretty">
              Self-hosting is a completeness test, not a gate.
            </span>
          </Panel>
        </div>
      </section>

      {/* A rail rather than a grid: the milestones are ordered, and the order
          is information. */}
      <section className="mx-auto max-w-[1400px] px-6 pt-20 sm:px-10">
        <SectionHeading
          title="Milestones"
          note={content.block("milestones-note")}
          icon={CheckIcon}
        />

        <ol className="mt-12 flex list-none flex-col">
          {MILESTONES.map((milestone, index) => {
            const style = STATUS_STYLE[milestone.status];
            const Icon = STATUS_ICON[milestone.status];
            const last = index === MILESTONES.length - 1;

            return (
              <Reveal
                key={milestone.number}
                as="li"
                delay={Math.min(index * 0.03, 0.18)}
                className="grid grid-cols-[28px_minmax(0,1fr)] gap-x-6 sm:grid-cols-[64px_minmax(0,1fr)]"
              >
                <div className="flex flex-col items-center">
                  <span
                    aria-hidden="true"
                    className={`mt-1.5 h-3.5 w-3.5 flex-none border-2 ${style.node}`}
                  />
                  {!last ? (
                    <span
                      aria-hidden="true"
                      className="bg-hairline w-px flex-1"
                      style={{ minHeight: 24 }}
                    />
                  ) : null}
                </div>

                <div className={`min-w-0 ${last ? "pb-0" : "pb-12"}`}>
                  <div className="flex flex-wrap items-center gap-3">
                    <span className="text-label font-mono text-[11px] tracking-[0.16em]">
                      {milestone.number === "—" ? "GOAL" : `MILESTONE ${milestone.number}`}
                    </span>
                    <span
                      className={`inline-flex items-center gap-1.5 px-2.5 py-1 font-mono text-[10px] tracking-[0.14em] uppercase ${style.chip}`}
                    >
                      <Icon size={12} />
                      {STATUS_LABEL[milestone.status]}
                    </span>
                  </div>

                  <h3 className="mt-3 text-[20px] leading-tight font-bold tracking-[-0.02em] sm:text-[24px]">
                    <a
                      href={milestone.anchor}
                      className="text-text hover:text-cinnabar-text panel-hover"
                    >
                      {milestone.title}
                    </a>
                  </h3>

                  <p className="text-secondary mt-3 max-w-[86ch] text-[15.5px] leading-[1.7] text-pretty">
                    {milestone.summary}
                  </p>
                </div>
              </Reveal>
            );
          })}
        </ol>
      </section>

      <section className="mx-auto max-w-[1400px] px-6 pt-24 pb-12 sm:px-10">
        <SectionHeading title="Full roadmap" note="ROADMAP.md" />
        <SourceNote className="mt-10">
          {linkRepoFile(content.block("source"), "ROADMAP.md")}
        </SourceNote>
      </section>

      <DocBody markdown={document} tocLabel="Sections" />
    </article>
  );
}
