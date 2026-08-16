import type { Metadata } from "next";
import GuidePage from "@/components/GuidePage";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent } from "@/lib/page-content";
export const og = { eyebrow: "Learn · 04", title: "Failure stays explicit.", description: "Result, Option, match, and try keep failure in the function’s type and control flow.", alt: "Cinnabar error handling learning chapter." };
export const metadata: Metadata = { title: "Error handling", description: "Learn how Cinnabar represents failure with Result and Option, requires exhaustive handling, and uses try for explicit propagation without hidden exceptions.", alternates: { canonical: "/learn/error-handling/" }, ...ogImageMetadata("/learn/error-handling/", og) };
export default async function Page() { const content = await readPageContent("learn/error-handling"); return <GuidePage section="Learn · 04" title="Failure stays explicit." lede={content.block("lede")} body={content.block("body")} source="Source: [MANIFESTO.md](https://github.com/bonzupii/cinnabar/blob/main/MANIFESTO.md) defines the normative failure rules." nextHref="/learn/first-program/" nextLabel="Next: First program" />; }
