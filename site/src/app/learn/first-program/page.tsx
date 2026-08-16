import type { Metadata } from "next";
import GuidePage from "@/components/GuidePage";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent } from "@/lib/page-content";
export const og = { eyebrow: "Learn · 05", title: "Your first Cinnabar program.", description: "Enter the Nix toolchain, create a project, check it, and run it.", alt: "Cinnabar first program learning chapter." };
export const metadata: Metadata = { title: "First program", description: "Build the Cinnabar compiler in its supported Nix environment, scaffold a project, understand the generated files, and check or run the program.", alternates: { canonical: "/learn/first-program/" }, ...ogImageMetadata("/learn/first-program/", og) };
export default async function Page() { const content = await readPageContent("learn/first-program"); return <GuidePage section="Learn · 05" title="Your first Cinnabar program." lede={content.block("lede")} body={content.block("body")} source="Source: [README.md](https://github.com/bonzupii/cinnabar/blob/main/README.md) and the repository’s Nix flake define the current toolchain." nextHref="/playground/" nextLabel="Open the playground" />; }
