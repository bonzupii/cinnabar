import type { Metadata } from "next";
import GuidePage from "@/components/GuidePage";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent } from "@/lib/page-content";

export const og = { eyebrow: "Getting started", title: "Build the compiler.", description: "The supported Nix toolchain, a first project, and the Cinnabar language server.", alt: "Cinnabar installation guide." };
export const metadata: Metadata = { title: "Install", description: "Build the Cinnabar compiler with the supported Nix environment, scaffold and run a first project, and connect an editor to cinnabar-lsp.", alternates: { canonical: "/install/" }, ...ogImageMetadata("/install/", og) };

export default async function InstallPage() {
  const content = await readPageContent("install");
  return <GuidePage section="Install" title="Build the compiler." lede={content.block("lede")} body={content.block("body")} source="Source: [README.md](https://github.com/bonzupii/cinnabar/blob/main/README.md), [flake.nix](https://github.com/bonzupii/cinnabar/blob/main/flake.nix), and build.rs." nextHref="/learn/first-program/" nextLabel="Walk through the first program" />;
}
