import Wordmark from "@/components/brand/Wordmark";
import CodeBlock from "@/components/CodeBlock";
import ShellBlock from "@/components/ShellBlock";
import DiagnosticTranscript, {
  DiagnosticLegend,
} from "@/components/DiagnosticTranscript";
import SampleExplorer from "@/components/SampleExplorer";
import SectionHeading from "@/components/SectionHeading";
import Markdown, { InlineMarkdown } from "@/components/Markdown";
import Reveal from "@/components/Reveal";
import { Eyebrow } from "@/components/PageHeader";
import { Action, ArrowLink } from "@/components/ui";
import {
  BuildIcon,
  CodegenIcon,
  DiagnosticIcon,
  DocIcon,
  GitHubIcon,
  LinearIcon,
} from "@/components/brand/icons";
import { HIGHLIGHTS } from "@/content/highlights";
import { STAGES } from "@/content/pipeline";
import { MANIFEST_SAMPLE } from "@/content/samples";
import { readPageContent } from "@/lib/page-content";
import { BADGES, QUIP, REPO_URL, STATUS_BADGE } from "@/lib/site";

/** Social image copy, consumed by ./opengraph-image.tsx. */
export const og = {
  eyebrow: "Systems language",
  title: "Consumed exactly once.",
  description:
    "A statically-typed systems language with Austral-style linear typing. No garbage collector, no lifetime annotations, no reachable panics.",
  alt: "Cinnabar — a statically-typed systems language with Austral-style linear typing.",
};

const INSTALL_SHELL = [
  "nix develop",
  "cargo build --release",
  "cinnabar init hello && cinnabar run hello",
];

export default async function Home() {
  const content = await readPageContent("(home)");

  return (
    <>
      {/* Hero — the cover of plate 00, and the README hero of plate 12. */}
      <section className="mx-auto max-w-[1400px] px-6 pt-16 pb-20 sm:px-10 sm:pt-24 sm:pb-28">
        {/*
          The wordmark is the page's heading — the home page's subject is the
          project itself. Wordmark carries the accessible name "Cinnabar", so
          the h1 announces that rather than the letterforms it draws.
        */}
        <h1>
          <Wordmark
            size="clamp(56px, 11.5vw, 184px)"
            step="display"
            letter="var(--text)"
            className="block"
          />
        </h1>

        <div className="mt-10 grid gap-8 lg:grid-cols-[minmax(0,1.55fr)_minmax(0,1fr)] lg:gap-16">
          <div className="text-text max-w-[46ch] text-[20px] leading-[1.4] tracking-[-0.015em] sm:text-[27px]">
            <InlineMarkdown>{content.block("tagline")}</InlineMarkdown>
          </div>
          <p className="text-secondary font-mono text-[13px] leading-[1.7] lg:self-end">
            {QUIP}
            <br />
            <a
              href={REPO_URL}
              target="_blank"
              rel="noopener noreferrer"
              className="text-label hover:text-cinnabar-text panel-hover"
            >
              github.com/bonzupii/cinnabar
            </a>
          </p>
        </div>

        {/* Plate 12's badge strip. */}
        <div className="rule-grid mt-11 flex w-fit flex-wrap">
          {BADGES.map((badge) => (
            <span
              key={badge}
              className="bg-ground text-secondary px-4 py-2.5 font-mono text-xs"
            >
              {badge}
            </span>
          ))}
          <span className="bg-cinnabar text-on-cinnabar px-4 py-2.5 font-mono text-xs font-medium">
            {STATUS_BADGE}
          </span>
        </div>

        <ShellBlock
          lines={INSTALL_SHELL}
          cwd="~/src"
          label="Getting started"
          className="mt-7 max-w-[760px]"
        />

        <div className="mt-9 flex flex-wrap items-center gap-4">
          <Action href="/manifesto/" variant="primary" icon={DocIcon}>
            Read the manifesto
          </Action>
          <Action href="/install/" icon={BuildIcon}>
            Install
          </Action>
          <Action href={REPO_URL} variant="ghost" icon={GitHubIcon} external>
            Source
          </Action>
        </div>
      </section>

      {/* The claim the whole language is organised around. */}
      <section className="border-hairline bg-panel border-y">
        <Reveal className="mx-auto grid max-w-[1400px] gap-8 px-6 py-20 sm:px-10 lg:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)] lg:gap-20">
          <h2 className="text-text max-w-[16ch] text-[32px] leading-[1.03] font-bold tracking-[-0.03em] sm:text-[46px]">
            {content.block("invariants-title")}
          </h2>
          <div className="[&_p]:max-w-none [&_p:first-child]:mt-0">
            <Markdown>{content.block("invariants")}</Markdown>
          </div>
        </Reveal>
      </section>

      {/* Highlights — README's own list, in the board's hairline grid. */}
      <section className="mx-auto max-w-[1400px] px-6 py-24 sm:px-10">
        <SectionHeading
          title="Language highlights"
          note={content.block("highlights-note")}
          icon={LinearIcon}
        />

        <div className="rule-grid mt-11 grid sm:grid-cols-2 lg:grid-cols-4">
          {HIGHLIGHTS.map(({ title, body, icon: Icon }, index) => (
            <Reveal
              key={title}
              delay={Math.min(index * 0.04, 0.2)}
              className="bg-panel hover:bg-panel-raised panel-hover flex flex-col gap-5 p-8"
            >
              <Icon size={24} className="text-text" />
              <h3 className="text-text text-[17px] leading-snug font-bold tracking-[-0.015em]">
                {title}
              </h3>
              <p className="text-secondary text-[14.5px] leading-[1.65] text-pretty">
                {body}
              </p>
            </Reveal>
          ))}
        </div>
      </section>

      {/* Samples, every one verbatim from the fixture corpus. */}
      <section className="border-hairline border-t">
        <div className="mx-auto max-w-[1400px] px-6 py-24 sm:px-10">
          <SectionHeading
            title="A taste of the language"
            note={content.block("samples-note")}
            icon={CodegenIcon}
          />
          <Reveal className="mt-11">
            <SampleExplorer />
          </Reveal>
        </div>
      </section>

      {/* Diagnostics — plate 10. */}
      <section className="border-hairline bg-panel border-t">
        <div className="mx-auto max-w-[1400px] px-6 py-24 sm:px-10">
          <SectionHeading
            title="Diagnostics"
            note={content.block("diagnostics-note")}
            icon={DiagnosticIcon}
          />

          <div className="mt-11 grid min-w-0 gap-10 lg:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]">
            <Reveal className="min-w-0">
              <DiagnosticTranscript />
            </Reveal>

            <Reveal delay={0.06} className="flex min-w-0 flex-col gap-8">
              <div className="flex flex-col gap-4">
                <Eyebrow>Rules</Eyebrow>
                <div className="[&_p]:max-w-none [&_p]:text-[15px] [&_p:first-child]:mt-0">
                  <Markdown>{content.block("diagnostics-rules")}</Markdown>
                </div>
              </div>
              <DiagnosticLegend />
            </Reveal>
          </div>
        </div>
      </section>

      {/* The pipeline, as a strip. */}
      <section className="border-hairline border-t">
        <div className="mx-auto max-w-[1400px] px-6 py-24 sm:px-10">
          <SectionHeading
            title="One fixed pipeline"
            note={content.block("pipeline-note")}
            icon={BuildIcon}
          />

          <div className="rule-grid mt-11 grid sm:grid-cols-2 lg:grid-cols-4">
            {STAGES.map((stage, index) => (
              <Reveal
                key={stage.name}
                delay={Math.min(index * 0.04, 0.2)}
                className="bg-panel flex flex-col gap-2.5 p-6"
              >
                <h3 className="text-text text-[15px] font-bold tracking-[-0.01em]">
                  {stage.name}
                </h3>
                <span className="text-label font-mono text-[11px] break-all">
                  {stage.file}
                </span>
              </Reveal>
            ))}
            <Reveal className="bg-ground flex flex-col justify-center gap-3 p-6">
              <ArrowLink href="/architecture/">Read the walkthrough</ArrowLink>
            </Reveal>
          </div>
        </div>
      </section>

      {/* The manifest is Cinnabar source, which is worth showing. */}
      <section className="border-hairline bg-panel border-t">
        <Reveal className="mx-auto grid max-w-[1400px] gap-12 px-6 py-24 sm:px-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-center">
          <div className="flex flex-col gap-6">
            <Eyebrow>The manifest</Eyebrow>
            <h2 className="text-text text-[28px] leading-tight font-bold tracking-[-0.025em] sm:text-[38px]">
              {content.block("manifest-title")}
            </h2>
            <div className="[&_p:first-child]:mt-0">
              <Markdown>{content.block("manifest")}</Markdown>
            </div>
            <ArrowLink href="/reference/#manifest">Manifest reference</ArrowLink>
          </div>
          <CodeBlock code={MANIFEST_SAMPLE} caption="build.cnb" />
        </Reveal>
      </section>

      {/* Close. */}
      <section className="border-hairline border-t">
        <Reveal className="mx-auto flex max-w-[1400px] flex-col gap-8 px-6 py-24 sm:px-10 lg:flex-row lg:items-center lg:justify-between">
          <div className="flex flex-col gap-4">
            <h2 className="text-text max-w-[20ch] text-[28px] leading-tight font-bold tracking-[-0.025em] sm:text-[38px]">
              {content.block("closing-title")}
            </h2>
            <div className="[&_p:first-child]:mt-0">
              <Markdown>{content.block("closing")}</Markdown>
            </div>
          </div>
          <div className="flex flex-wrap gap-4">
            <Action href="/roadmap/" variant="primary">
              Roadmap
            </Action>
            <Action href="/architecture/">Architecture</Action>
          </div>
        </Reveal>
      </section>
    </>
  );
}
