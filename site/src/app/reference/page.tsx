import type { Metadata } from "next";
import CodeBlock from "@/components/CodeBlock";
import { UsageBlock } from "@/components/ShellBlock";
import PageHeader, { Eyebrow } from "@/components/PageHeader";
import SectionHeading from "@/components/SectionHeading";
import DataTable from "@/components/DataTable";
import Reveal from "@/components/Reveal";
import { ArrowLink, Callout, Prose } from "@/components/ui";
import { BuildIcon, DocIcon, FmtIcon, RunIcon, TestIcon } from "@/components/brand/icons";
import { CLI_SECTIONS, TEST_PROFILES, USAGE, type Row } from "@/content/cli";
import { MANIFEST_SAMPLE } from "@/content/samples";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent, type PageContent } from "@/lib/page-content";
import { REPO_URL } from "@/lib/site";

/** Social image copy, rendered by ./og-image/route.tsx. */
export const og = {
  eyebrow: "CLI reference",
  title: "Two ways to invoke it.",
  description:
    "Every flag for compiling a file, every project subcommand, the build.cnb manifest, and the test layout.",
  alt: "Cinnabar social card — the CLI reference.",
};

export const metadata: Metadata = {
  title: "CLI Reference",
  description:
    "The Cinnabar CLI: every flag for compiling a single file, every project subcommand, the build.cnb manifest, the test layout, and the local test profiles.",
  alternates: { canonical: "/reference/" },
  ...ogImageMetadata("/reference/", og),
};


/**
 * A `-rows` block, as table rows.
 *
 * Each `###` section is one row: the heading is the flag, command, file name
 * or variable, and the paragraph under it is what it does. Writing them as one
 * markdown section apiece is what keeps a name and its description from
 * drifting — they are the same paragraph.
 */
function tableRows(content: PageContent, block: string): Row[] {
  return content
    .items(block)
    .map((item) => ({ name: item.title, description: item.body }));
}

const SECTION_ICONS = {
  "single-file": BuildIcon,
  project: RunIcon,
  inspect: FmtIcon,
} as const;

export default async function ReferencePage() {
  const content = await readPageContent("reference");

  return (
    <article className="pb-28">
      <PageHeader
        section="CLI Reference"
        note="cinnabar <COMMAND> --help prints the full description"
        icon={DocIcon}
        title="Two ways to invoke it."
        lede={content.block("lede")}
      />

      <div className="mx-auto flex max-w-350 flex-col gap-20 px-6 pt-16 sm:px-10 *:min-w-0">
        <section>
          <Eyebrow>Usage</Eyebrow>
          <UsageBlock lines={USAGE.split("\n")} className="mt-5" />
        </section>

        {CLI_SECTIONS.map((section) => {
          const intro = content.optional(`${section.id}-intro`);
          return (
            <section key={section.id} className="min-w-0">
              <SectionHeading
                id={section.id}
                title={content.block(`${section.id}-heading`)}
                note={content.block(`${section.id}-note`)}
                icon={SECTION_ICONS[section.id as keyof typeof SECTION_ICONS]}
              />
              {intro ? (
                <Reveal className="mt-8">
                  <p className="text-secondary max-w-[90ch] text-[16.5px] leading-[1.75] text-pretty">
                    {intro}
                  </p>
                </Reveal>
              ) : null}
              <DataTable
                rows={tableRows(content, `${section.id}-rows`)}
                nameHeading={section.nameHeading}
              />
            </section>
          );
        })}

        <section className="min-w-0">
          <SectionHeading
            id="manifest"
            title="The manifest"
            note="build.cnb · Cinnabar source"
            icon={BuildIcon}
          />
          <div className="mt-9 grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-start *:min-w-0">
            <Reveal>
              <Prose>{content.block("manifest")}</Prose>
            </Reveal>
            <Reveal delay={0.06}>
              <CodeBlock code={MANIFEST_SAMPLE} path="build.cnb" title="The manifest" />
            </Reveal>
          </div>
        </section>

        <section className="min-w-0">
          <SectionHeading
            id="test-layout"
            title="Test layout"
            note="cinnabar test decides from the file name"
            icon={TestIcon}
          />
          <DataTable rows={tableRows(content, "test-layout-rows")} nameHeading="File" />
          <Prose className="mt-8">{content.block("test-layout")}</Prose>
        </section>

        <section className="min-w-0">
          <SectionHeading
            id="test-profiles"
            title="Local test profiles"
            note="The full profile ignores every override below"
            icon={TestIcon}
          />
          <DataTable
            headings={[
              "Profile",
              "Fuzz corpus",
              "Native fuzz runs",
              "Native fixture runs",
              "Record-only runs",
            ]}
            data={TEST_PROFILES.map((profile) => [
              profile.name,
              profile.corpus,
              profile.nativeFuzz,
              profile.nativeFixtures,
              profile.recordOnly,
            ])}
          />
          <Prose className="mt-8">{content.block("profiles")}</Prose>
          <DataTable
            rows={tableRows(content, "test-env-rows")}
            nameHeading="Environment variable"
          />
        </section>

        <Reveal>
          <Callout>
            <Eyebrow>Every command documents itself</Eyebrow>
            <Prose className="[&_p]:text-[17px] [&_p]:text-bright">
              {content.block("self-documenting")}
            </Prose>
            <ArrowLink
              href={`${REPO_URL}/blob/main/README.md#using-the-compiler`}
              external
              className="mt-1"
            >
              README · using the compiler
            </ArrowLink>
          </Callout>
        </Reveal>
      </div>
    </article>
  );
}
