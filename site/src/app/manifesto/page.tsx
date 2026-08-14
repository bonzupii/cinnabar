import type { Metadata } from "next";
import DocBody from "@/components/DocBody";
import PageHeader from "@/components/PageHeader";
import { SourceNote } from "@/components/ui";
import { DocIcon } from "@/components/brand/icons";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent } from "@/lib/page-content";
import { linkRepoFile, readRepoDoc } from "@/lib/repo-docs";

/** Social image copy, rendered by ./og-image/route.tsx. */
export const og = {
  eyebrow: "Normative specification",
  title: "The Cinnabar Manifesto",
  description:
    "Twelve core principles, the authoritative language surface, and the anti-principles the language will never have.",
  alt: "Cinnabar social card — the normative language specification.",
};

export const metadata: Metadata = {
  title: "Manifesto",
  description:
    "The normative Cinnabar language specification: twelve core principles, the authoritative language surface, and the anti-principles.",
  alternates: { canonical: "/manifesto/" },
  ...ogImageMetadata("/manifesto/", og),
};


export default async function ManifestoPage() {
  const [document, content] = await Promise.all([
    readRepoDoc("MANIFESTO.md"),
    readPageContent("manifesto"),
  ]);

  return (
    <article className="pb-28">
      <PageHeader
        section="Manifesto"
        note="Normative · MANIFESTO.md"
        icon={DocIcon}
        title="The Cinnabar Manifesto"
        lede={content.block("lede")}
      />

      <div className="mx-auto max-w-[1400px] px-6 sm:px-10">
        <SourceNote className="mt-12 mb-14">
          {linkRepoFile(content.block("source"), "MANIFESTO.md")}
        </SourceNote>
      </div>

      <DocBody markdown={document} tocLabel="Sections" />
    </article>
  );
}
