import type { Metadata } from "next";
import DocBody from "@/components/DocBody";
import PageHeader, { Eyebrow } from "@/components/PageHeader";
import SectionHeading from "@/components/SectionHeading";
import Reveal from "@/components/Reveal";
import { ArrowLink, Callout, Panel, SourceNote, Stat } from "@/components/ui";
import { CheckIcon, LinearIcon, RunIcon } from "@/components/brand/icons";
import {
  HORIZON,
  IN_PROGRESS,
  MILESTONE_TALLY,
  SHIPPED,
  type Capability,
} from "@/content/roadmap";
import { CONTAINER, ICON } from "@/lib/constants";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent } from "@/lib/page-content";
import { linkRepoFile, readRepoDoc } from "@/lib/repo-docs";

/** Social image copy, rendered by ./og-image/route.tsx. */
export const og = {
  eyebrow: "Project status",
  title: "Eight milestones down.",
  description:
    "What the language and its toolchain do today, what is still being widened, and why self-hosting is a completeness test rather than a gate.",
  alt: "Cinnabar social card — the project status.",
};

export const metadata: Metadata = {
  title: "Roadmap",
  description:
    "What Cinnabar does today: linear types checked on every path, direct system calls, freestanding static binaries — and self-hosting on the horizon.",
  alternates: { canonical: "/roadmap/" },
  ...ogImageMetadata("/roadmap/", og),
};


/** One capability, as a cell in the hairline grid. */
function CapabilityCard({
  capability,
  delay,
}: {
  capability: Capability;
  delay: number;
}) {
  const Icon = capability.icon;
  return (
    <Reveal
      delay={delay}
      className="bg-panel hover:bg-panel-raised panel-hover flex flex-col gap-5 p-8"
    >
      <Icon size={ICON.card} className="text-text" />
      <h3 className="text-[17px] leading-snug font-bold tracking-[-0.015em]">
        <a
          href={capability.anchor}
          className="text-text hover:text-cinnabar-text panel-hover"
        >
          {capability.title}
        </a>
      </h3>
      <p className="text-secondary text-[14.5px] leading-[1.65] text-pretty">
        {capability.detail}
      </p>
    </Reveal>
  );
}

export default async function RoadmapPage() {
  const [document, content] = await Promise.all([
    readRepoDoc("ROADMAP.md"),
    readPageContent("roadmap"),
  ]);

  return (
    <article className="pb-28">
      <PageHeader
        section="Roadmap"
        note="A milestone is done only when fixtures, gate and spec agree"
        icon={CheckIcon}
        title="Eight milestones down."
        lede={content.block("lede")}
      />

      {/* The shape of the project in three numbers. */}
      <section className={`${CONTAINER} pt-14`}>
        <div className="rule-grid grid grid-cols-2 sm:grid-cols-4">
          <Stat value={SHIPPED.length} label="Capabilities shipped" />
          <Stat
            value={`${MILESTONE_TALLY.complete} / ${MILESTONE_TALLY.total}`}
            label="Milestones complete"
          />
          <Stat value={IN_PROGRESS.length} label="Still widening" />
          <Panel className="bg-ground justify-center gap-2 p-6">
            <span className="text-secondary text-[13px] leading-[1.55] text-pretty">
              Self-hosting is the next horizon, and a completeness test rather than a
              gate.
            </span>
          </Panel>
        </div>
      </section>

      {/* What the language does today. */}
      <section className={`${CONTAINER} pt-20`}>
        <SectionHeading
          title="What Cinnabar does today"
          note={content.block("shipped-note")}
          icon={CheckIcon}
        />
        <div className="rule-grid mt-11 grid sm:grid-cols-2 lg:grid-cols-3">
          {SHIPPED.map((capability, index) => (
            <CapabilityCard
              key={capability.title}
              capability={capability}
              delay={Math.min(index * 0.03, 0.18)}
            />
          ))}
        </div>
      </section>

      {/* The two partials, stated plainly rather than as open tickets. */}
      <section className={`${CONTAINER} pt-24`}>
        <SectionHeading title="Still widening" note="Marked partial" icon={RunIcon} />
        <Reveal className="mt-9">
          <p className="text-secondary max-w-[86ch] text-[16.5px] leading-[1.75] text-pretty">
            {content.block("progress")}
          </p>
        </Reveal>
        <div className="rule-grid mt-9 grid sm:grid-cols-2">
          {IN_PROGRESS.map((capability, index) => (
            <CapabilityCard
              key={capability.title}
              capability={capability}
              delay={index * 0.04}
            />
          ))}
        </div>
      </section>

      {/* The horizon. */}
      <section className={`${CONTAINER} pt-24`}>
        <SectionHeading
          title="On the horizon"
          note={content.block("horizon-note")}
          icon={LinearIcon}
        />
        <Reveal className="mt-11">
          <Callout>
            <Eyebrow>Next</Eyebrow>
            <h3 className="text-text text-[28px] leading-tight font-bold tracking-[-0.025em] sm:text-[36px]">
              {HORIZON.title}
            </h3>
            <p className="text-bright max-w-[80ch] text-[17px] leading-[1.7] text-pretty">
              {HORIZON.detail}
            </p>
            <ArrowLink href={HORIZON.anchor}>Read the reasoning</ArrowLink>
          </Callout>
        </Reveal>
      </section>

      <section className={`${CONTAINER} pt-24 pb-12`}>
        <SectionHeading title="The full record" note="ROADMAP.md" />
        <SourceNote className="mt-10">
          {linkRepoFile(content.block("source"), "ROADMAP.md")}
        </SourceNote>
      </section>

      <DocBody markdown={document} tocLabel="Milestones" />
    </article>
  );
}
