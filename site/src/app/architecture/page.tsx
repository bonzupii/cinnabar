import type { Metadata } from "next";
import Disclosure from "@/components/Disclosure";
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
    "Seven stages over a flat node arena, where every fact is computed once and attached for later stages to read.",
  alt: "Cinnabar social card — the compiler internals.",
};

export const metadata: Metadata = {
  title: "Architecture",
  description:
    "How the Cinnabar compiler is built: one fixed pipeline of seven stages over a flat node arena, with every fact computed once and attached for later stages to read.",
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
        The stages as a run, in order, with each summary folded away.
        ARCHITECTURE.md draws the pipeline as an ASCII column; the order is the
        part worth seeing first, so the rows carry that and nothing else.
      */}
      <section className={`${CONTAINER} pt-16`}>
        <SectionHeading
          title="The pipeline"
          note={content.block("stages-note")}
          icon={BuildIcon}
        />

        <ol className="mt-11 flex list-none flex-col">
          {STAGES.map((stage, index) => (
            <Reveal
              key={stage.name}
              as="li"
              delay={Math.min(index * 0.03, 0.18)}
              className="min-w-0"
            >
              <Disclosure summary={`${stage.index} — ${stage.name}`}>
                <div className="flex flex-col gap-3 pt-1">
                  <span className="text-label font-mono text-[11px] break-all">
                    {stage.file}
                  </span>
                  <div className={`${STAGE_PROSE} max-w-[86ch]`}>
                    <InlineMarkdown>{stage.summary}</InlineMarkdown>
                  </div>
                </div>
              </Disclosure>
            </Reveal>
          ))}
        </ol>

        {/* What the seven have in common, stated once under the run. */}
        <Reveal className="mt-8">
          <Panel className="bg-ground gap-4 p-6">
            <LinearIcon size={22} className="text-cinnabar-text" />
            <p className="text-bright max-w-[80ch] text-[14.5px] leading-[1.65] text-pretty">
              A failure at any stage halts the pipeline and prints source-located
              diagnostics. There is no partial output: a build either produces its
              artifact or produces diagnostics.
            </p>
          </Panel>
        </Reveal>
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
            <h3 className="text-text text-[28px] leading-[1.1] font-bold tracking-tight sm:text-[36px]">
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

      <div className={CONTAINER}>
        <Disclosure summary={content.block("document")}>
          <DocBody markdown={document} tocLabel="Sections" />
        </Disclosure>
      </div>
    </article>
  );
}
