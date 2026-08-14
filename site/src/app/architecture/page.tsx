import type { Metadata } from "next";
import DocBody from "@/components/DocBody";
import PageHeader, { Eyebrow } from "@/components/PageHeader";
import SectionHeading from "@/components/SectionHeading";
import Markdown, { InlineMarkdown } from "@/components/Markdown";
import Reveal from "@/components/Reveal";
import { Callout, MarkedList, Panel, SourceNote } from "@/components/ui";
import { BuildIcon, LinearIcon, StaticLinkIcon } from "@/components/brand/icons";
import { ARENA_PROPERTIES, ARENAS, SINGLE_FACT_RULE, STAGES } from "@/content/pipeline";
import { CONTAINER } from "@/lib/constants";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent } from "@/lib/page-content";
import { linkRepoFile, readRepoDoc } from "@/lib/repo-docs";

/** Social image copy, rendered by ./og-image/route.tsx. */
export const og = {
  eyebrow: "Compiler internals",
  title: "One fixed pipeline.",
  description:
    "Seven stages over a flat node arena, where every fact is computed exactly once and attached for later stages to read.",
  alt: "Cinnabar social card — the compiler internals.",
};

export const metadata: Metadata = {
  title: "Architecture",
  description:
    "How the Cinnabar compiler is built: one fixed pipeline of seven stages over a flat node arena, with every fact computed exactly once.",
  alternates: { canonical: "/architecture/" },
  ...ogImageMetadata("/architecture/", og),
};


/** Inline code inside a stage summary, which quotes identifiers in backticks. */
const STAGE_PROSE =
  "text-secondary text-[14.5px] leading-[1.65] text-pretty [&_code]:border [&_code]:border-[color:var(--hairline-strong)] [&_code]:px-[4px] [&_code]:font-mono [&_code]:text-[0.9em] [&_code]:text-[color:var(--bright)]";

export default async function ArchitecturePage() {
  const [document, content] = await Promise.all([
    readRepoDoc("ARCHITECTURE.md"),
    readPageContent("architecture"),
  ]);

  return (
    <article className="pb-28">
      <PageHeader
        section="Architecture"
        note="ARCHITECTURE.md · read from the source"
        icon={BuildIcon}
        title="One fixed pipeline."
        lede={content.block("lede")}
      />

      {/*
        The pipeline as a figure before the prose. ARCHITECTURE.md draws it as
        an ASCII column; at this width it reads better as a run of stages.
      */}
      <section className={`${CONTAINER} pt-16`}>
        <SectionHeading
          title="The pipeline"
          note={content.block("stages-note")}
          icon={BuildIcon}
        />

        <ol className="rule-grid mt-11 grid list-none sm:grid-cols-2 lg:grid-cols-4">
          {STAGES.map((stage, index) => (
            <Reveal
              key={stage.name}
              as="li"
              delay={Math.min(index * 0.04, 0.2)}
              className="bg-panel hover:bg-panel-raised panel-hover flex flex-col gap-4 p-7"
            >
              <span className="text-cinnabar-text font-mono text-[10px] tracking-[0.16em]">
                {stage.index}
              </span>
              <h3 className="text-text text-[17px] font-bold tracking-[-0.015em]">
                {stage.name}
              </h3>
              <span className="text-label font-mono text-[11px] break-all">
                {stage.file}
              </span>
              <div className={STAGE_PROSE}>
                <InlineMarkdown>{stage.summary}</InlineMarkdown>
              </div>
            </Reveal>
          ))}

          {/* The eighth cell states what the seven have in common. */}
          <Reveal as="li" className="bg-ground flex flex-col justify-center gap-4 p-7">
            <LinearIcon size={22} className="text-cinnabar-text" />
            <p className="text-bright text-[14px] leading-[1.6] text-pretty">
              A failure at any stage halts the pipeline and prints source-located
              diagnostics. There is no partial output.
            </p>
          </Reveal>
        </ol>
      </section>

      {/* The representation, which is the genuinely unusual part. */}
      <section className={`${CONTAINER} pt-24`}>
        <SectionHeading
          title="The core representation"
          note="Struct of arrays, not a tree"
          icon={StaticLinkIcon}
        />

        <div className="mt-11 grid gap-10 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)] lg:gap-16">
          <Reveal className="flex flex-col gap-6">
            <h3 className="text-text text-[28px] leading-[1.1] font-bold tracking-[-0.025em] sm:text-[36px]">
              {content.block("arena-title")}
            </h3>
            <div className="[&_p:first-child]:mt-0">
              <Markdown>{content.block("arena")}</Markdown>
            </div>
            <MarkedList items={ARENA_PROPERTIES} accent className="mt-2" />
          </Reveal>

          <Reveal delay={0.06} className="rule-grid flex min-w-0 flex-col">
            {ARENAS.map((arena) => (
              <Panel key={arena.name} className="gap-3 p-7">
                <div className="flex items-baseline gap-3">
                  <span className="text-text font-mono text-[15px] font-semibold">
                    {arena.name}
                  </span>
                  <span className="text-label font-mono text-[13px]">{arena.type}</span>
                </div>
                <p className="text-secondary text-[14px] leading-[1.65] text-pretty">
                  {arena.summary}
                </p>
              </Panel>
            ))}
          </Reveal>
        </div>
      </section>

      {/* The governing rule, given the weight the document gives it. */}
      <section className={`${CONTAINER} pt-24`}>
        <Reveal>
          <Callout>
            <Eyebrow>The Single-Fact Rule</Eyebrow>
            <p className="text-bright text-[17px] leading-[1.7] text-pretty sm:text-[19px]">
              {SINGLE_FACT_RULE}
            </p>
          </Callout>
        </Reveal>
      </section>

      <section className={`${CONTAINER} pt-24 pb-12`}>
        <SectionHeading title="Full walkthrough" note="ARCHITECTURE.md" />
        <SourceNote className="mt-10">
          {linkRepoFile(content.block("source"), "ARCHITECTURE.md")}
        </SourceNote>
      </section>

      <DocBody markdown={document} tocLabel="Sections" />
    </article>
  );
}
