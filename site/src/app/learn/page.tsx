import type { Metadata } from "next";
import PageHeader from "@/components/PageHeader";
import Reveal from "@/components/Reveal";
import { ArrowLink } from "@/components/ui";
import { DocIcon } from "@/components/brand/icons";
import { LEARN_CHAPTERS } from "@/content/learn";
import { ogImageMetadata } from "@/lib/og-image";

export const og = { eyebrow: "Learn Cinnabar", title: "From design stance to first program.", description: "A guided path through linear ownership, borrowing, explicit failure, and the Cinnabar toolchain.", alt: "Cinnabar learning path." };
export const metadata: Metadata = { title: "Learn", description: "Learn why Cinnabar exists, how linear types and flow-sensitive borrowing work, how failures stay explicit, and how to run a first program.", alternates: { canonical: "/learn/" }, ...ogImageMetadata("/learn/", og) };

export default function LearnPage() {
  return <article className="pb-28">
    <PageHeader section="Learn" note="Five short chapters" icon={DocIcon} title="From design stance to first program." lede="Start with the constraints Cinnabar is designed to enforce, then follow them into ownership, borrowing, failure handling, and the toolchain." />
    <div className="mx-auto max-w-350 px-6 pt-16 sm:px-10">
      <ol className="rule-grid grid list-none md:grid-cols-2">
        {LEARN_CHAPTERS.map((chapter, index) => { const Icon = chapter.icon; return <Reveal as="li" key={chapter.href} delay={index * 0.04} className="bg-panel hover:bg-panel-raised panel-hover flex flex-col gap-5 p-8">
          <div className="flex items-center justify-between"><Icon size={22} className="text-cinnabar-text" /><span className="text-label font-mono text-xs">0{index + 1}</span></div>
          <h2 className="text-text text-[24px] font-bold tracking-tight">{chapter.title}</h2>
          <p className="text-secondary grow text-[15px] leading-[1.7]">{chapter.summary}</p>
          <ArrowLink href={chapter.href}>Read chapter</ArrowLink>
        </Reveal>; })}
      </ol>
      <div className="mt-12 flex flex-wrap gap-6"><ArrowLink href="/manifesto/">Normative manifesto</ArrowLink><ArrowLink href="/reference/">CLI reference</ArrowLink></div>
    </div>
  </article>;
}
