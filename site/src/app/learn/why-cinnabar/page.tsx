import type { Metadata } from "next";
import GuidePage from "@/components/GuidePage";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent } from "@/lib/page-content";

export const og = { eyebrow: "Learn · 01", title: "Why Cinnabar?", description: "A systems language that makes safety, ownership, failure, and explicitness non-bypassable.", alt: "Why Cinnabar learning chapter." };
export const metadata: Metadata = { title: "Why Cinnabar", description: "Understand the failure modes Cinnabar is designed around and why its safety, ownership, failure-handling, and explicitness rules cannot be suppressed.", alternates: { canonical: "/learn/why-cinnabar/" }, ...ogImageMetadata("/learn/why-cinnabar/", og) };
export default async function Page() { const content = await readPageContent("learn/why-cinnabar"); return <GuidePage section="Learn · 01" title="Why Cinnabar?" lede={content.block("lede")} body={content.block("body")} source="Source: [MANIFESTO.md](https://github.com/bonzupii/cinnabar/blob/main/MANIFESTO.md) is normative." nextHref="/learn/linear-types/" nextLabel="Next: Linear types" />; }
