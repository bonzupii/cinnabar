import type { Metadata } from "next";
import CodeBlock from "@/components/CodeBlock";
import { UsageBlock } from "@/components/ShellBlock";
import PageHeader, { Eyebrow } from "@/components/PageHeader";
import SectionHeading from "@/components/SectionHeading";
import Markdown, { InlineMarkdown } from "@/components/Markdown";
import Reveal from "@/components/Reveal";
import {
  BuildIcon,
  DocIcon,
  FmtIcon,
  RunIcon,
  TestIcon,
} from "@/components/brand/icons";
import {
  CLI_SECTIONS,
  TEST_ENV,
  TEST_LAYOUT,
  TEST_PROFILES,
  USAGE,
  type Row,
} from "@/content/cli";
import { MANIFEST_SAMPLE } from "@/content/samples";
import { readPageContent } from "@/lib/page-content";
import { REPO_URL } from "@/lib/site";

export const metadata: Metadata = {
  title: "Reference",
  description:
    "The Cinnabar CLI: every flag for compiling a single file, every project subcommand, the build.cnb manifest, the test layout, and the local test profiles.",
  alternates: { canonical: "/reference/" },
};

const SECTION_ICONS = {
  "single-file": BuildIcon,
  project: RunIcon,
  inspect: FmtIcon,
} as const;

/** A two-column definition table in the board's hairline grid. */
function RowTable({ rows, nameHeading }: { rows: readonly Row[]; nameHeading: string }) {
  return (
    <div className="rule-grid mt-8 block overflow-x-auto">
      <table className="bg-ground w-full border-collapse text-left">
        <thead className="bg-panel">
          <tr>
            <th
              scope="col"
              className="border-hairline text-label border-b px-5 py-3 font-mono text-[10px] font-medium tracking-[0.16em] whitespace-nowrap uppercase"
            >
              {nameHeading}
            </th>
            <th
              scope="col"
              className="border-hairline text-label border-b px-5 py-3 font-mono text-[10px] font-medium tracking-[0.16em] uppercase"
            >
              What it does
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.name}>
              <th
                scope="row"
                className="border-hairline text-text border-b px-5 py-3.5 text-left align-top font-mono text-[13px] font-normal whitespace-nowrap"
              >
                {row.name}
              </th>
              <td className="border-hairline text-secondary border-b px-5 py-3.5 align-top text-[14.5px] leading-relaxed">
                {row.description}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Prose({ children }: { children: string }) {
  return (
    <div className="max-w-[86ch] [&_p:first-child]:mt-0">
      <Markdown>{children}</Markdown>
    </div>
  );
}

export default async function ReferencePage() {
  const content = await readPageContent("reference");

  return (
    <article className="pb-28">
      <PageHeader
        section="Reference"
        note="cinnabar <COMMAND> --help prints the full description"
        icon={DocIcon}
        title="Two ways to invoke it."
        lede={
          <div className="text-secondary text-[18px] leading-[1.55] tracking-[-0.01em] text-pretty sm:text-[21px] [&_code]:font-mono [&_code]:text-[0.9em]">
            <InlineMarkdown>{content.block("lede")}</InlineMarkdown>
          </div>
        }
      />

      <div className="mx-auto flex max-w-[1400px] flex-col gap-20 px-6 pt-16 sm:px-10 [&>*]:min-w-0">
        <section>
          <Eyebrow>Usage</Eyebrow>
          <UsageBlock lines={USAGE.split("\n")} className="mt-5" />
        </section>

        {CLI_SECTIONS.map((section) => (
          <section key={section.id} className="min-w-0">
            <SectionHeading
              id={section.id}
              title={section.title}
              note={section.note}
              icon={SECTION_ICONS[section.id as keyof typeof SECTION_ICONS]}
            />
            {section.intro ? (
              <Reveal className="mt-8">
                <p className="text-secondary max-w-[90ch] text-[16.5px] leading-[1.75] text-pretty">
                  {section.intro}
                </p>
              </Reveal>
            ) : null}
            <RowTable
              rows={section.rows}
              nameHeading={section.id === "single-file" ? "Flag" : "Command"}
            />
          </section>
        ))}

        <section className="min-w-0">
          <SectionHeading
            id="manifest"
            title="The manifest"
            note="build.cnb · Cinnabar source"
            icon={BuildIcon}
          />
          <div className="mt-9 grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-start [&>*]:min-w-0">
            <Reveal>
              <Prose>{content.block("manifest")}</Prose>
            </Reveal>
            <Reveal delay={0.06}>
              <CodeBlock code={MANIFEST_SAMPLE} caption="build.cnb" />
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
          <RowTable rows={TEST_LAYOUT} nameHeading="File" />
          <div className="mt-8">
            <Prose>{content.block("test-layout")}</Prose>
          </div>
        </section>

        <section className="min-w-0">
          <SectionHeading
            id="test-profiles"
            title="Local test profiles"
            note="The full profile ignores every override below"
            icon={TestIcon}
          />
          <div className="rule-grid mt-8 block overflow-x-auto">
            <table className="bg-ground w-full border-collapse text-left">
              <thead className="bg-panel">
                <tr>
                  {[
                    "Profile",
                    "Fuzz corpus",
                    "Native fuzz runs",
                    "Native fixture runs",
                    "Record-only runs",
                  ].map((heading) => (
                    <th
                      key={heading}
                      scope="col"
                      className="border-hairline text-label border-b px-5 py-3 font-mono text-[10px] font-medium tracking-[0.16em] whitespace-nowrap uppercase"
                    >
                      {heading}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {TEST_PROFILES.map((profile) => (
                  <tr key={profile.name}>
                    <th
                      scope="row"
                      className="border-hairline text-text border-b px-5 py-3.5 text-left font-mono text-[13px] font-normal whitespace-nowrap"
                    >
                      {profile.name}
                    </th>
                    {[
                      profile.corpus,
                      profile.nativeFuzz,
                      profile.nativeFixtures,
                      profile.recordOnly,
                    ].map((cell, index) => (
                      <td
                        key={index}
                        className="border-hairline text-secondary border-b px-5 py-3.5 text-[14.5px] whitespace-nowrap"
                      >
                        {cell}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div className="mt-8">
            <Prose>{content.block("profiles")}</Prose>
          </div>
          <RowTable rows={TEST_ENV} nameHeading="Environment variable" />
        </section>

        <Reveal className="border-hairline bg-panel flex flex-col gap-5 border p-8 sm:p-10">
          <Eyebrow>Every command documents itself</Eyebrow>
          <div className="max-w-[72ch] [&_p]:text-[17px] [&_p:first-child]:mt-0 [&_p]:text-[color:var(--bright)]">
            <Markdown>{content.block("self-documenting")}</Markdown>
          </div>
          <a
            href={`${REPO_URL}/blob/main/README.md#using-the-compiler`}
            target="_blank"
            rel="noopener noreferrer"
            className="text-cinnabar-text hover:text-text panel-hover mt-1 inline-block text-[13px] font-bold tracking-[0.1em] uppercase"
          >
            README · using the compiler →
          </a>
        </Reveal>
      </div>
    </article>
  );
}
