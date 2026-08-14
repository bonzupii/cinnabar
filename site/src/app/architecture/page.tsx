import type { Metadata } from "next";
import Disclosure from "@/components/Disclosure";
import DocBody from "@/components/DocBody";
import PageHeader, { Eyebrow } from "@/components/PageHeader";
import SectionHeading from "@/components/SectionHeading";
import Markdown, { InlineMarkdown } from "@/components/Markdown";
import Reveal from "@/components/Reveal";
import { Callout, MarkedList, Panel, SourceNote } from "@/components/ui";
import { BuildIcon, LinearIcon, StaticLinkIcon } from "@/components/brand/icons";
import { ARENAS, STAGES } from "@/content/pipeline";
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
  "text-secondary text-[14.5px] leading-[1.65] text-pretty [&_code]:border [&_code]:border-hairline-strong [&_code]:px-[4px] [&_code]:font-mono [&_code]:text-[0.9em] [&_code]:text-bright";

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
                <InlineMarkdown>{content.block(`stage-${stage.slug}`)}</InlineMarkdown>
              </div>
            </Reveal>
          ))}

          {/* The eighth cell states what the seven have in common. */}
          <Reveal as="li" className="bg-ground flex flex-col justify-center gap-4 p-7">
            <LinearIcon size={22} className="text-cinnabar-text" />
            <div className="text-bright text-[14px] leading-[1.6] text-pretty [&_p:first-child]:mt-0">
              <InlineMarkdown>{content.block("stages-halt")}</InlineMarkdown>
            </div>
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
            <h3 className="text-text text-[28px] leading-[1.1] font-bold tracking-tight sm:text-[36px]">
              {content.block("arena-title")}
            </h3>
            <div className="[&_p:first-child]:mt-0">
              <Markdown>{content.block("arena")}</Markdown>
            </div>
            <MarkedList items={content.list("arena-properties")} accent className="mt-2" />
          </Reveal>

          {/*
            `self-start` is load-bearing. A grid item is stretched to the height
            of the tallest column, and this one is a `.rule-grid`, which paints
            `--hairline` as its own background so the 1px seams between its
            children read as rules. Three cards sized by their own text do not
            grow with the column, and the uncovered remainder showed as a grey
            band across the bottom of the stack.

            The window frame solved the same problem by growing its body, which
            is right there: a terminal's ground reaching the frame edge is what
            a terminal looks like. It is the wrong answer here. A card's height
            is its content's, and stretching these three — or distributing the
            slack between them — would tie how tall a card is to how long the
            prose in the *other* column happens to run. Not stretching is the
            honest fix, and it is also self-correcting: below `lg` the columns
            stack, nothing stretches, and `self-start` does nothing.
          */}
          <Reveal delay={0.06} className="rule-grid flex min-w-0 flex-col self-start">
            {ARENAS.map((arena) => (
              <Panel key={arena.name} className="gap-3 p-7">
                <div className="flex items-baseline gap-3">
                  <span className="text-text font-mono text-[15px] font-semibold">
                    {arena.name}
                  </span>
                  <span className="text-label font-mono text-[13px]">{arena.type}</span>
                </div>
                <p className="text-secondary text-[14px] leading-[1.65] text-pretty">
                  {content.block(`arena-${arena.name}`)}
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
              {content.block("single-fact-rule")}
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
