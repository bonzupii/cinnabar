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
import { HIGHLIGHT_ICONS } from "@/content/highlights";
import { STAGES } from "@/content/pipeline";
import { MANIFEST_SAMPLE, SAMPLES } from "@/content/samples";
import { ICON } from "@/lib/constants";
import { readPageContent } from "@/lib/page-content";
import { BADGES, REPO_URL, STATUS_BADGE } from "@/lib/site";

/** Social image copy, rendered by /og-image. The root layout points at it. */
export const og = {
  eyebrow: "Systems language",
  title: "For compilers, runtimes, kernels and firmware.",
  // No backticks: this string is also drawn into the social image by Satori,
  // which has no markdown and would print them.
  description:
    "Statically typed, compiled through LLVM 21 to static native binaries. Handles are linear, borrows are checked without lifetime annotations, and no flag turns a check off.",
  alt: "Cinnabar — a linear-typed systems language for compilers, runtimes, kernels and firmware.",
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
      <section className="mx-auto max-w-350 px-6 pt-16 pb-20 sm:px-10 sm:pt-24 sm:pb-28">
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

        {/*
          The quip and the repository URL used to sit in a second column here.
          Both are gone: the repository is one click away in the header and
          again in the footer, and the hero's job is to say what the language
          is, not to make a joke about another one.
        */}
        <div className="mt-10 flex flex-col gap-6">
          <div className="text-text max-w-[46ch] text-[20px] leading-[1.4] tracking-[-0.015em] sm:text-[27px]">
            <InlineMarkdown>{content.block("tagline")}</InlineMarkdown>
          </div>
          {/*
            What it is, then what it is for. A reader deciding whether to keep
            reading needs the domain and the implementation, and both are
            stated in README.md's opening two paragraphs.
          */}
          <div className="text-secondary max-w-[62ch] text-[16px] leading-[1.7]">
            <InlineMarkdown>{content.block("hero-why")}</InlineMarkdown>
          </div>
        </div>

        {/*
          Plate 12's badge strip.

          `grow` on every badge is the same class of fix as the arena stack on
          /architecture/ and the window body: a `.rule-grid` paints `--hairline`
          as its own background, so any part of it the children do not cover
          shows as a grey block rather than as a rule. `w-fit` is `fit-content`,
          which clamps to the space available — so on a phone the strip is as
          wide as the section while its badges have wrapped onto two lines, and
          the tail of the last line was left uncovered. Flex distributes free
          space per line, so growing the badges fills whichever line is short
          and changes nothing at a width where they all fit on one.
        */}
        <div className="rule-grid mt-11 flex w-fit flex-wrap">
          {BADGES.map((badge) => (
            <span
              key={badge}
              className="bg-ground text-secondary grow px-4 py-2.5 text-center font-mono text-xs"
            >
              {badge}
            </span>
          ))}
          <span className="bg-cinnabar text-on-cinnabar grow px-4 py-2.5 text-center font-mono text-xs font-medium">
            {STATUS_BADGE}
          </span>
        </div>

        {/*
          The three commands, beside what each one actually does. The shell
          block on its own told a reader how to start but not what starting
          would cost them — the flake, the build, the scaffold.
        */}
        <div className="mt-9 grid gap-8 lg:grid-cols-[minmax(0,1.55fr)_minmax(0,1fr)] lg:gap-16">
          <ShellBlock lines={INSTALL_SHELL} cwd="~/src" className="max-w-190" />
          <div className="flex flex-col gap-5 [&_li]:text-[15px] [&_ul]:mt-0">
            <Markdown>{content.block("hero-steps")}</Markdown>
            <ArrowLink href="/install/">Full build instructions</ArrowLink>
          </div>
        </div>

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

      {/* The stance the rest of the language follows from — MANIFESTO.md's opening. */}
      <section className="border-hairline bg-panel border-y">
        {/*
          One column, headline then body. It was a two-column split, which set
          the headline against a body column narrower than the section it sits
          in; the passage is the argument the whole language follows from and
          reads better given the full measure.
        */}
        <Reveal className="mx-auto flex max-w-350 flex-col gap-8 px-6 py-20 sm:px-10">
          <h2 className="text-text text-[32px] leading-[1.03] font-bold tracking-[-0.03em] sm:text-[46px]">
            {content.block("invariants-title")}
          </h2>
          <div className="[&_p]:max-w-none [&_p:first-child]:mt-0">
            <Markdown>{content.block("invariants")}</Markdown>
          </div>
        </Reveal>
      </section>

      {/* Highlights — README's own list, in the board's hairline grid. */}
      <section className="mx-auto max-w-350 px-6 py-24 sm:px-10">
        <SectionHeading
          title="Language highlights"
          note={content.block("highlights-note")}
          icon={LinearIcon}
        />

        <div className="rule-grid mt-11 grid sm:grid-cols-2 lg:grid-cols-4">
          {content.items("highlights").map(({ slug, title, body }, index) => {
            // Throws at build time if content.md gains a highlight that
            // highlights.ts has no icon for, or reworders one it does.
            const Icon = HIGHLIGHT_ICONS[slug];
            if (!Icon) {
              throw new Error(
                `No icon bound for highlight "${slug}" in src/content/highlights.ts`,
              );
            }
            return (
              <Reveal
                key={slug}
                delay={Math.min(index * 0.04, 0.2)}
                className="bg-panel hover:bg-panel-raised panel-hover flex flex-col gap-5 p-8"
              >
                <Icon size={ICON.card} className="text-text" />
                <h3 className="text-text text-[17px] leading-snug font-bold tracking-[-0.015em]">
                  {title}
                </h3>
                <p className="text-secondary text-[14.5px] leading-[1.65] text-pretty">
                  {body}
                </p>
              </Reveal>
            );
          })}
        </div>
      </section>

      {/* Samples, every one verbatim from the fixture corpus. */}
      <section className="border-hairline border-t">
        <div className="mx-auto max-w-350 px-6 py-24 sm:px-10">
          <SectionHeading
            title="A taste of the language"
            note={content.block("samples-note")}
            icon={CodegenIcon}
          />
          <Reveal className="mt-11">
            <SampleExplorer
              summaries={Object.fromEntries(
                SAMPLES.map((sample) => [
                  sample.id,
                  content.block(`sample-${sample.id}`),
                ]),
              )}
            />
          </Reveal>
        </div>
      </section>

      {/* Diagnostics — plate 10. */}
      <section className="border-hairline bg-panel border-t">
        <div className="mx-auto max-w-350 px-6 py-24 sm:px-10">
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
        <div className="mx-auto max-w-350 px-6 py-24 sm:px-10">
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
        <Reveal className="mx-auto grid max-w-350 gap-12 px-6 py-24 sm:px-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:items-center">
          <div className="flex flex-col gap-6">
            <Eyebrow>The manifest</Eyebrow>
            <h2 className="text-text text-[28px] leading-tight font-bold tracking-tight sm:text-[38px]">
              {content.block("manifest-title")}
            </h2>
            <div className="[&_p:first-child]:mt-0">
              <Markdown>{content.block("manifest")}</Markdown>
            </div>
            <ArrowLink href="/reference/#manifest">Manifest reference</ArrowLink>
          </div>
          <CodeBlock code={MANIFEST_SAMPLE} path="build.cnb" title="The manifest" />
        </Reveal>
      </section>

      {/* Close. */}
      <section className="border-hairline border-t">
        <Reveal className="mx-auto flex max-w-350 flex-col gap-8 px-6 py-24 sm:px-10 lg:flex-row lg:items-center lg:justify-between">
          <div className="flex flex-col gap-4">
            <h2 className="text-text max-w-[20ch] text-[28px] leading-tight font-bold tracking-tight sm:text-[38px]">
              {content.block("closing-title")}
            </h2>
            <div className="[&_p:first-child]:mt-0">
              <Markdown>{content.block("closing")}</Markdown>
            </div>
          </div>
          {/*
            `lg:flex-none` is what keeps these two on one line: as a flex item
            of the row above they were shrinkable, so the prose beside them
            squeezed the pair until the second wrapped under the first. Below
            `lg` the parent stacks and `flex-wrap` still applies, so a narrow
            phone can wrap them rather than overflow.
          */}
          <div className="flex flex-wrap gap-4 lg:flex-none lg:flex-nowrap">
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
