import type { Metadata } from "next";
import GuidePage from "@/components/GuidePage";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent } from "@/lib/page-content";
export const og = { eyebrow: "Learn · 03", title: "Borrowing without lifetime syntax.", description: "Shared and exclusive borrows with scopes inferred from control flow.", alt: "Cinnabar borrowing learning chapter." };
export const metadata: Metadata = { title: "Borrowing", description: "Learn Cinnabar’s shared and exclusive references, flow-sensitive borrow scopes, mutation rules, and rejection of ambiguous returned borrows.", alternates: { canonical: "/learn/borrowing/" }, ...ogImageMetadata("/learn/borrowing/", og) };
export default async function Page() { const content = await readPageContent("learn/borrowing"); return <GuidePage section="Learn · 03" title="Borrowing without lifetime syntax." lede={content.block("lede")} body={content.block("body")} source="Source: [MANIFESTO.md §§5–6](https://github.com/bonzupii/cinnabar/blob/main/MANIFESTO.md#5-references-and-borrowing) is normative." nextHref="/learn/error-handling/" nextLabel="Next: Error handling" />; }
