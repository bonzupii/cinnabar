import type { Metadata } from "next";
import DocBody from "@/components/DocBody";
import PageHeader from "@/components/PageHeader";
import { InlineMarkdown } from "@/components/Markdown";
import { DocIcon } from "@/components/brand/icons";
import { readPageContent } from "@/lib/page-content";
import { readRepoDoc, REPO_URL } from "@/lib/repo-docs";

export const metadata: Metadata = {
  title: "Manifesto",
  description:
    "The normative Cinnabar language specification: twelve core principles, the authoritative language surface, and the anti-principles.",
  alternates: { canonical: "/manifesto/" },
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
        lede={
          <div className="text-secondary text-[18px] leading-[1.55] tracking-[-0.01em] text-pretty sm:text-[21px]">
            <InlineMarkdown>{content.block("lede")}</InlineMarkdown>
          </div>
        }
      />

      <div className="mx-auto max-w-[1400px] px-6 sm:px-10">
        <div className="border-cinnabar text-bright mt-12 mb-14 border-l-2 pl-6 font-mono text-[13px] leading-[1.8] [&_a]:text-[color:var(--cinnabar-text)] [&_a]:underline [&_a]:underline-offset-[3px]">
          <InlineMarkdown>
            {content
              .block("source")
              .replace("`MANIFESTO.md`", `[MANIFESTO.md](${REPO_URL}/blob/main/MANIFESTO.md)`)}
          </InlineMarkdown>
        </div>
      </div>

      <DocBody markdown={document} tocLabel="Sections" />
    </article>
  );
}
