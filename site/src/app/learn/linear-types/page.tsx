import type { Metadata } from "next";
import GuidePage from "@/components/GuidePage";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent } from "@/lib/page-content";
export const og = { eyebrow: "Learn · 02", title: "Linear types.", description: "Resource handles are consumed exactly once on every control-flow path.", alt: "Cinnabar linear types learning chapter." };
export const metadata: Metadata = { title: "Linear types", description: "Learn how Cinnabar’s declared linear types make resource ownership explicit and require every handle to be consumed exactly once on every path.", alternates: { canonical: "/learn/linear-types/" }, ...ogImageMetadata("/learn/linear-types/", og) };
export default async function Page() { const content = await readPageContent("learn/linear-types"); return <GuidePage section="Learn · 02" title="Linear types." lede={content.block("lede")} body={content.block("body")} source="Source: [MANIFESTO.md §7](https://github.com/bonzupii/cinnabar/blob/main/MANIFESTO.md#7-linear-types) and the repository’s linear fixtures." nextHref="/learn/borrowing/" nextLabel="Next: Borrowing" />; }
