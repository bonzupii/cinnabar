import Link from "next/link";
import Wordmark from "@/components/brand/Wordmark";
import CodeBlock, { TerminalBlock } from "@/components/CodeBlock";
import ShellBlock from "@/components/ShellBlock";
import SampleExplorer from "@/components/SampleExplorer";
import SectionHeading from "@/components/SectionHeading";
import Markdown, { InlineMarkdown } from "@/components/Markdown";
import Reveal from "@/components/Reveal";
import { Eyebrow } from "@/components/PageHeader";
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

const SHELL = [
  "nix develop",
  "cargo build --release",
  "cinnabar init hello && cinnabar run hello",
];

/** The diagnostic legend from plate 10, shown as swatches beside their roles. */
const DIAGNOSTIC_ROLES = [
  ["error", "#E0442A · 600"],
  ["message", "#EDE9E6 · 600"],
  ["source", "#C9C2BD · 400"],
  ["secondary", "#A29B96 · 400"],
  ["gutter", "#7C7570 · 300"],
] as const;

export default async function Home() {
  const content = await readPageContent(".");

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

        <ShellBlock lines={SHELL} className="mt-7 max-w-[760px]" />

        <div className="mt-9 flex flex-wrap items-center gap-4">
          <Link
            href="/manifesto/"
            className="bg-cinnabar text-on-cinnabar hover:bg-cinnabar-deep panel-hover flex items-center gap-2.5 px-7 py-3.5 text-sm font-bold tracking-[0.1em] uppercase"
          >
            <DocIcon size={16} />
            Read the manifesto
          </Link>
          <Link
            href="/install/"
            className="border-hairline-strong text-text hover:border-text panel-hover flex items-center gap-2.5 border px-7 py-3.5 text-sm font-bold tracking-[0.1em] uppercase"
          >
            <BuildIcon size={16} />
            Install
          </Link>
          <a
            href={REPO_URL}
            target="_blank"
            rel="noopener noreferrer"
            className="text-secondary hover:text-text panel-hover flex items-center gap-2.5 px-2 py-3.5 text-sm font-bold tracking-[0.1em] uppercase"
          >
            <GitHubIcon size={15} />
            Source
          </a>
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
              <TerminalBlock>
                <span className="text-term-prompt">$ </span>
                <span className="text-term-command">cinnabar</span> src/main.cnb{" "}
                <span className="text-term-flag">--explain-borrow</span>
                {"\n\n"}
                {/* Plate 10 fixes the error accent at #E0442A, which measures
                    4.74:1 on the terminal ground — so it stays exact. */}
                <span className="text-term-error font-semibold">error</span>
                <span className="text-term-command font-semibold">
                  : linear value `vec` is not consumed on every path
                </span>
                {"\n"}
                <span className="text-term-gutter"> ╭─[</span>
                <span className="text-term-flag">src/main.cnb:14:5</span>
                <span className="text-term-gutter">]</span>
                {"\n"}
                <span className="text-term-gutter"> │</span>
                {"\n"}
                <span className="text-term-gutter"> 11 │</span>{" "}
                <span className="text-term-flag">val vec = vec_new[I64]()?</span>
                {"\n"}
                <span className="text-term-gutter"> │</span>{" "}
                <span className="text-term-output"> ─┬─</span>
                {"\n"}
                <span className="text-term-gutter"> │</span>{" "}
                <span className="text-term-output">
                  {" "}
                  ╰── bound here as `Collections.Vec(I64)`, linear
                </span>
                {"\n"}
                <span className="text-term-gutter"> 15 │</span>{" "}
                <span className="text-term-flag"> return 0</span>
                {"\n"}
                <span className="text-term-gutter"> │</span>{" "}
                <span className="text-term-error"> ────┬───</span>
                {"\n"}
                <span className="text-term-gutter"> │</span>{" "}
                <span className="text-term-error">
                  {" "}
                  ╰─── this path returns without consuming `vec`
                </span>
                {"\n"}
                <span className="text-term-gutter"> 18 │</span>{" "}
                <span className="text-term-flag"> vec_free(vec)</span>
                {"\n"}
                <span className="text-term-gutter"> │</span>{" "}
                <span className="text-term-output"> ──────┬──────</span>
                {"\n"}
                <span className="text-term-gutter"> │</span>{" "}
                <span className="text-term-output">
                  {" "}
                  ╰── consumed on the other path
                </span>
                {"\n"}
                <span className="text-term-gutter">───╯</span>
                {"\n"}
                <span className="text-term-command">help</span>
                <span className="text-term-output">
                  : consume `vec` before returning, or restructure so both
                </span>
                {"\n"}
                <span className="text-term-output"> paths leave through one exit.</span>
              </TerminalBlock>
            </Reveal>

            <Reveal delay={0.06} className="flex min-w-0 flex-col gap-8">
              <div className="flex flex-col gap-4">
                <Eyebrow>Rules</Eyebrow>
                <div className="[&_p]:max-w-none [&_p]:text-[15px] [&_p:first-child]:mt-0">
                  <Markdown>{content.block("diagnostics-rules")}</Markdown>
                </div>
              </div>
              {/*
                Plate 09's legend puts a filled swatch beside each label rather
                than tinting the label itself — which is also the only honest
                way to show these values, since setting "#7C7570 · 300" in
                #7C7570 would not be legible on this panel.
              */}
              <dl className="border-hairline flex flex-col border-t">
                {DIAGNOSTIC_ROLES.map(([role, value]) => (
                  <div
                    key={role}
                    className="border-hairline flex items-center gap-3.5 border-b py-3.5 font-mono text-xs"
                  >
                    <span
                      aria-hidden="true"
                      className="border-hairline h-3.5 w-3.5 flex-none border"
                      style={{ background: value.split(" · ")[0] }}
                    />
                    <dt className="text-label">{role}</dt>
                    <dd className="text-secondary ml-auto">{value}</dd>
                  </div>
                ))}
              </dl>
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
              <Link
                href="/architecture/"
                className="text-cinnabar-text hover:text-text panel-hover text-[13px] font-bold tracking-[0.1em] uppercase"
              >
                Read the walkthrough →
              </Link>
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
            <Link
              href="/reference/#manifest"
              className="text-cinnabar-text hover:text-text panel-hover w-fit text-[13px] font-bold tracking-[0.1em] uppercase"
            >
              Manifest reference →
            </Link>
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
            <Link
              href="/roadmap/"
              className="bg-cinnabar text-on-cinnabar hover:bg-cinnabar-deep panel-hover px-7 py-3.5 text-sm font-bold tracking-[0.1em] uppercase"
            >
              Roadmap
            </Link>
            <Link
              href="/architecture/"
              className="border-hairline-strong text-text hover:border-text panel-hover border px-7 py-3.5 text-sm font-bold tracking-[0.1em] uppercase"
            >
              Architecture
            </Link>
          </div>
        </Reveal>
      </section>
    </>
  );
}
