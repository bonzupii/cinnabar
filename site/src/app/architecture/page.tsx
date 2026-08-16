import type { Metadata } from "next";
import PageHeader, { Eyebrow } from "@/components/PageHeader";
import SectionHeading from "@/components/SectionHeading";
import Markdown, { InlineMarkdown } from "@/components/Markdown";
import Reveal from "@/components/Reveal";
import { Callout, MarkedList, Panel, SourceNote } from "@/components/ui";
import { BuildIcon, StaticLinkIcon } from "@/components/brand/icons";
import { ARENAS, STAGES } from "@/content/pipeline";
import { CONTAINER } from "@/lib/constants";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent } from "@/lib/page-content";
import { linkRepoFile } from "@/lib/repo-docs";

export const og = { eyebrow: "Compiler internals", title: "One fixed pipeline.", description: "Seven stages over flat arenas, with each semantic fact computed once and carried forward.", alt: "Cinnabar compiler architecture." };
export const metadata: Metadata = { title: "Architecture", description: "Explore Cinnabar’s fixed compiler pipeline, flat arena representation, and rule that semantic facts are computed once by their owning stage.", alternates: { canonical: "/architecture/" }, ...ogImageMetadata("/architecture/", og) };

export default async function ArchitecturePage() {
  const content = await readPageContent("architecture");
  return <article className="pb-28">
    <PageHeader section="Architecture" note="Anchored chapters from ARCHITECTURE.md" icon={BuildIcon} title="One fixed pipeline." lede={content.block("lede")} />
    <section id="pipeline" className={`${CONTAINER} scroll-mt-24 pt-16`}>
      <SectionHeading title="The pipeline" note={content.block("stages-note")} icon={BuildIcon} />
      <figure className="rule-grid mt-11">
        <figcaption className="text-label bg-panel px-7 py-4 font-mono text-[10px] tracking-[0.16em] uppercase">
          Compiler pipeline diagram
        </figcaption>
        <ol className="grid list-none">
          {STAGES.map((stage, index) => <Reveal key={stage.name} as="li" delay={index * 0.04} className="bg-panel hover:bg-panel-raised panel-hover grid gap-3 p-7 sm:grid-cols-[4rem_9rem_minmax(0,1fr)] sm:items-start sm:gap-6">
            <span className="text-cinnabar-text font-mono text-[10px] tracking-[0.16em]">{stage.index}</span><div><h3 className="text-text text-[17px] font-bold">{stage.name}</h3><span className="text-label mt-1 block font-mono text-[11px] break-all">{stage.file}</span></div><div className="text-secondary text-[14.5px] leading-[1.65]"><InlineMarkdown>{content.block(`stage-${stage.slug}`)}</InlineMarkdown></div>
          </Reveal>)}
          <Reveal as="li" className="bg-ground grid gap-3 p-7 sm:grid-cols-[4rem_9rem_minmax(0,1fr)] sm:gap-6"><span className="text-cinnabar-text font-mono text-[10px] tracking-[0.16em]">HALT</span><span className="text-text font-bold">On error</span><InlineMarkdown>{content.block("stages-halt")}</InlineMarkdown></Reveal>
        </ol>
      </figure>
    </section>
    <section id="representation" className={`${CONTAINER} scroll-mt-24 pt-24`}>
      <SectionHeading title="The core representation" note="Struct of arrays, not a tree" icon={StaticLinkIcon} />
      <div className="mt-11 grid gap-10 lg:grid-cols-2 lg:gap-16">
        <Reveal><h3 className="text-text text-[32px] font-bold tracking-tight">{content.block("arena-title")}</h3><div className="mt-6"><Markdown>{content.block("arena")}</Markdown></div><MarkedList items={content.list("arena-properties")} accent className="mt-7" /></Reveal>
        <Reveal delay={0.06} className="rule-grid flex flex-col self-start">{ARENAS.map((arena) => <Panel key={arena.name} className="gap-3 p-7"><div className="flex gap-3"><span className="text-text font-mono font-semibold">{arena.name}</span><span className="text-label font-mono text-[13px]">{arena.type}</span></div><p className="text-secondary text-[14px] leading-[1.65]">{content.block(`arena-${arena.name}`)}</p></Panel>)}</Reveal>
      </div>
    </section>
    <section id="single-fact-rule" className={`${CONTAINER} scroll-mt-24 pt-24`}><Reveal><Callout><Eyebrow>The Single-Fact Rule</Eyebrow><p className="text-bright text-[18px] leading-[1.7]">{content.block("single-fact-rule")}</p></Callout></Reveal></section>
    <section className={`${CONTAINER} pt-20`}><SourceNote>{linkRepoFile("Read `ARCHITECTURE.md` for the complete source-level reference.", "ARCHITECTURE.md")}</SourceNote></section>
  </article>;
}
