import Wordmark from "@/components/brand/Wordmark";
import PlaygroundEditor from "@/components/PlaygroundEditor";
import SampleExplorer from "@/components/SampleExplorer";
import SectionHeading from "@/components/SectionHeading";
import Markdown, { InlineMarkdown } from "@/components/Markdown";
import Reveal from "@/components/Reveal";
import { Eyebrow } from "@/components/PageHeader";
import { Action, ArrowLink } from "@/components/ui";
import { BuildIcon, CodegenIcon, DocIcon, GitHubIcon, LinearIcon } from "@/components/brand/icons";
import { HIGHLIGHT_ICONS } from "@/content/highlights";
import { SAMPLES } from "@/content/samples";
import { ICON } from "@/lib/constants";
import { readPageContent } from "@/lib/page-content";
import { readRepoFixture } from "@/lib/repo-docs";
import { BADGES, REPO_URL, STATUS_BADGE } from "@/lib/site";

export const og = {
  eyebrow: "Systems language",
  title: "Systems programming without safety escape hatches.",
  description: "Linear resource ownership and flow-sensitive borrow checking, without lifetime annotations or a garbage collector.",
  alt: "Cinnabar — systems programming without safety escape hatches.",
};

export default async function Home() {
  const [content, fixture] = await Promise.all([
    readPageContent("(home)"),
    readRepoFixture("explainLeak"),
  ]);

  return (
    <>
      <section className="mx-auto max-w-350 px-6 pt-16 pb-20 sm:px-10 sm:pt-24 sm:pb-28">
        <h1><Wordmark size="clamp(56px, 11.5vw, 184px)" step="display" letter="var(--text)" className="block" /></h1>
        <div className="mt-10 grid gap-12 lg:grid-cols-[minmax(0,0.88fr)_minmax(0,1.12fr)] lg:gap-14">
          <div className="flex flex-col gap-6">
            <h2 className="text-text text-[32px] leading-[1.04] font-bold tracking-[-0.03em] sm:text-[48px]">{content.block("tagline")}</h2>
            <div className="text-secondary text-[18px] leading-[1.65] text-pretty"><InlineMarkdown>{content.block("hero-why")}</InlineMarkdown></div>
            <div className="rule-grid mt-2 flex w-fit flex-wrap">
              {BADGES.map((badge) => <span key={badge} className="bg-ground text-secondary grow px-4 py-2.5 text-center font-mono text-xs">{badge}</span>)}
              <span className="bg-cinnabar text-on-cinnabar grow px-4 py-2.5 text-center font-mono text-xs font-medium">{STATUS_BADGE}</span>
            </div>
            <div className="mt-2 flex flex-wrap items-center gap-4">
              <Action href="/playground/" variant="primary" icon={CodegenIcon}>Try in Playground</Action>
              <Action href="/manifesto/" icon={DocIcon}>Manifesto</Action>
              <Action href="/install/" variant="ghost" icon={BuildIcon}>Install</Action>
              <Action href={REPO_URL} variant="ghost" icon={GitHubIcon} external>Source</Action>
            </div>
          </div>
          <div className="min-w-0">
            <div className="mb-4 flex items-center justify-between gap-4">
              <Eyebrow>Real compiler front end</Eyebrow>
              <span className="text-label font-mono text-[11px]">tests/fixtures/explain_leak.cnb</span>
            </div>
            <PlaygroundEditor mode="embedded" initialSource={fixture} />
            <p className="text-secondary mt-4 text-[13px] leading-[1.65]">{content.block("hero-proof")}</p>
          </div>
        </div>
      </section>

      <section className="border-hairline bg-panel border-y">
        <div className="mx-auto max-w-350 px-6 py-24 sm:px-10">
          <SectionHeading title="Three promises" note={content.block("promises-note")} icon={LinearIcon} />
          <div className="rule-grid mt-11 grid lg:grid-cols-2">
            {content.items("promises").map(({ slug, title, body }, index) => {
              const Icon = HIGHLIGHT_ICONS[slug];
              if (!Icon) throw new Error(`No icon bound for promise "${slug}"`);
              return <Reveal key={slug} delay={index * 0.05} className={`bg-ground hover:bg-panel-raised panel-hover flex flex-col gap-5 p-8 sm:p-10 ${index === 0 ? "lg:col-span-2 lg:grid lg:grid-cols-[auto_minmax(0,1fr)] lg:items-start lg:gap-x-8" : ""}`}>
                <Icon size={ICON.card} className="text-text" />
                <div><h3 className="text-text text-[22px] leading-snug font-bold tracking-[-0.02em]">{title}</h3><div className="mt-3 text-secondary text-[15px] leading-[1.7] text-pretty"><InlineMarkdown>{body}</InlineMarkdown></div></div>
              </Reveal>;
            })}
          </div>
        </div>
      </section>

      <section className="border-hairline border-b">
        <div className="mx-auto max-w-350 px-6 py-24 sm:px-10">
          <SectionHeading title="A taste of the language" note={content.block("samples-note")} icon={CodegenIcon} />
          <Reveal className="mt-11"><SampleExplorer summaries={Object.fromEntries(SAMPLES.map((sample) => [sample.id, content.block(`sample-${sample.id}`)]))} /></Reveal>
        </div>
      </section>

      <section className="mx-auto grid max-w-350 gap-12 px-6 py-24 sm:px-10 lg:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)]">
        <Reveal className="flex flex-col gap-5"><Eyebrow>Go deeper</Eyebrow><h2 className="text-text text-[30px] leading-tight font-bold tracking-tight sm:text-[40px]">{content.block("depth-title")}</h2><Markdown>{content.block("depth")}</Markdown></Reveal>
        <Reveal delay={0.06} className="rule-grid grid sm:grid-cols-2">
          <div className="bg-panel flex flex-col gap-3 p-7"><ArrowLink href="/learn/">Learn the language</ArrowLink><p className="text-secondary text-sm">Concepts, examples, and a first program.</p></div>
          <div className="bg-panel flex flex-col gap-3 p-7"><ArrowLink href="/install/">Install Cinnabar</ArrowLink><p className="text-secondary text-sm">Toolchain requirements and build steps.</p></div>
          <div className="bg-panel flex flex-col gap-3 p-7"><ArrowLink href="/reference/">CLI reference</ArrowLink><p className="text-secondary text-sm">Commands, flags, manifests, and tests.</p></div>
          <div className="bg-panel flex flex-col gap-3 p-7"><ArrowLink href="/architecture/">Compiler architecture</ArrowLink><p className="text-secondary text-sm">The fixed pipeline and canonical facts.</p></div>
        </Reveal>
      </section>

      <section className="border-hairline bg-panel border-t">
        <Reveal className="mx-auto flex max-w-350 flex-col gap-6 px-6 py-20 sm:px-10 lg:flex-row lg:items-center lg:justify-between">
          <div><Eyebrow>Project status</Eyebrow><h2 className="text-text mt-4 text-[28px] font-bold tracking-tight">{content.block("status-title")}</h2><p className="text-secondary mt-3 max-w-[70ch] leading-[1.7]">{content.block("status")}</p></div>
          <Action href="/roadmap/" variant="primary">Read the roadmap</Action>
        </Reveal>
      </section>
    </>
  );
}
