import type { Metadata } from "next";
import GuidePage from "@/components/GuidePage";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent } from "@/lib/page-content";
export const og = { eyebrow: "Contributing", title: "Develop across worktrees.", description: "Docker, reusable caches, attached editors, and the repository verification gate.", alt: "Cinnabar contributor development guide." };
export const metadata: Metadata = { title: "Contributor development", description: "Configure Cinnabar contributor worktrees with the repository’s Docker Compose helper, reusable caches, attached editor workflow, and verification gate.", alternates: { canonical: "/contributing/development/" }, ...ogImageMetadata("/contributing/development/", og) };
export default async function Page() { const content = await readPageContent("contributing/development"); return <GuidePage section="Contributing" title="Develop across worktrees." lede={content.block("lede")} body={content.block("body")} source="Source: [CONTAINER_DEVELOPMENT.md](https://github.com/bonzupii/cinnabar/blob/main/CONTAINER_DEVELOPMENT.md) remains the operational authority." nextHref="/architecture/" nextLabel="Read the compiler architecture" />; }
