import type { Metadata } from "next";
import DocBody from "@/components/DocBody";
import PageHeader, { Eyebrow } from "@/components/PageHeader";
import SectionHeading from "@/components/SectionHeading";
import Markdown, { InlineMarkdown } from "@/components/Markdown";
import Reveal from "@/components/Reveal";
import { BuildIcon, LinearIcon, StaticLinkIcon } from "@/components/brand/icons";
import { ARENA_PROPERTIES, ARENAS, SINGLE_FACT_RULE, STAGES } from "@/content/pipeline";
import { readPageContent } from "@/lib/page-content";
import { readRepoDoc, REPO_URL } from "@/lib/repo-docs";

export const metadata: Metadata = {
  title: "Architecture",
  description:
    "How the Cinnabar compiler is built: one fixed pipeline of seven stages over a flat node arena, with every fact computed exactly once.",
  alternates: { canonical: "/architecture/" },
};

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
        lede={
          <div className="text-secondary text-[18px] leading-[1.55] tracking-[-0.01em] text-pretty sm:text-[21px]">
            <InlineMarkdown>{content.block("lede")}</InlineMarkdown>
          </div>
        }
      />

      {/*
        The pipeline as a figure before the prose. ARCHITECTURE.md draws it as
        an ASCII column; at this width it reads better as a run of stages with
        the flow between them made explicit.
      */}
      <section className="mx-auto max-w-[1400px] px-6 pt-16 sm:px-10">
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
              <div className="flex items-baseline justify-between gap-3">
                <span className="text-cinnabar-text font-mono text-[10px] tracking-[0.16em]">
                  {stage.index}
                </span>
                {stage.size ? (
                  <span className="text-label font-mono text-[10px]">{stage.size}</span>
                ) : null}
              </div>
              <h3 className="text-text text-[17px] font-bold tracking-[-0.015em]">
                {stage.name}
              </h3>
              <span className="text-label font-mono text-[11px] break-all">
                {stage.file}
              </span>
              {/* The summaries quote identifiers with backticks, so they are
                  rendered as markdown rather than printed literally. */}
              <div className="text-secondary text-[14.5px] leading-[1.65] text-pretty [&_code]:border [&_code]:border-[color:var(--hairline-strong)] [&_code]:px-[4px] [&_code]:font-mono [&_code]:text-[0.9em] [&_code]:text-[color:var(--bright)]">
                <InlineMarkdown>{stage.summary}</InlineMarkdown>
              </div>
            </Reveal>
          ))}

          {/* The eighth cell states what the seven have in common. */}
          <Reveal
            as="li"
            className="bg-ground flex flex-col justify-center gap-4 p-7"
          >
            <LinearIcon size={22} className="text-cinnabar-text" />
            <p className="text-bright text-[14px] leading-[1.6] text-pretty">
              A failure at any stage halts the pipeline and prints source-located
              diagnostics. There is no partial output.
            </p>
          </Reveal>
        </ol>
      </section>

      {/* The representation, which is the genuinely unusual part. */}
      <section className="mx-auto max-w-[1400px] px-6 pt-24 sm:px-10">
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
            <ul className="mt-2 flex list-none flex-col gap-2.5 pl-0">
              {ARENA_PROPERTIES.map((property) => (
                <li
                  key={property}
                  className="text-secondary relative pl-6 text-[14.5px] leading-[1.6] before:absolute before:top-[0.55em] before:left-0 before:h-[6px] before:w-[6px] before:bg-[color:var(--cinnabar)] before:content-['']"
                >
                  {property}
                </li>
              ))}
            </ul>
          </Reveal>

          <Reveal delay={0.06} className="rule-grid flex min-w-0 flex-col">
            {ARENAS.map((arena) => (
              <div key={arena.name} className="bg-panel flex flex-col gap-3 p-7">
                <div className="flex items-baseline gap-3">
                  <span className="text-text font-mono text-[15px] font-semibold">
                    {arena.name}
                  </span>
                  <span className="text-label font-mono text-[13px]">
                    {arena.type}
                  </span>
                </div>
                <p className="text-secondary text-[14px] leading-[1.65] text-pretty">
                  {arena.summary}
                </p>
              </div>
            ))}
          </Reveal>
        </div>
      </section>

      {/* The governing rule, given the weight the document gives it. */}
      <section className="mx-auto max-w-[1400px] px-6 pt-24 sm:px-10">
        <Reveal className="border-hairline bg-panel flex flex-col gap-5 border p-8 sm:p-12">
          <Eyebrow>The Single-Fact Rule</Eyebrow>
          <p className="text-bright text-[17px] leading-[1.7] text-pretty sm:text-[19px]">
            {SINGLE_FACT_RULE}
          </p>
        </Reveal>
      </section>

      <section className="mx-auto max-w-[1400px] px-6 pt-24 pb-12 sm:px-10">
        <SectionHeading title="Full walkthrough" note="ARCHITECTURE.md" />
        <div className="border-cinnabar text-bright mt-10 border-l-2 pl-6 font-mono text-[13px] leading-[1.8] [&_a]:text-[color:var(--cinnabar-text)] [&_a]:underline [&_a]:underline-offset-[3px]">
          <InlineMarkdown>
            {content
              .block("source")
              .replace(
                "`ARCHITECTURE.md`",
                `[ARCHITECTURE.md](${REPO_URL}/blob/main/ARCHITECTURE.md)`,
              )}
          </InlineMarkdown>
        </div>
      </section>

      <DocBody markdown={document} tocLabel="Sections" />
    </article>
  );
}
