import type { Metadata } from "next";
import ActivityFeed from "@/components/ActivityFeed";
import Disclosure from "@/components/Disclosure";
import DocBody from "@/components/DocBody";
import PageHeader, { Eyebrow } from "@/components/PageHeader";
import SectionHeading from "@/components/SectionHeading";
import Reveal from "@/components/Reveal";
import { ArrowLink, Callout, Panel, SourceNote, Stat } from "@/components/ui";
import { CheckIcon, GitHubIcon, LinearIcon, RunIcon } from "@/components/brand/icons";
import {
  HORIZON,
  IN_PROGRESS,
  MILESTONE_TALLY,
  SHIPPED,
  type Capability,
} from "@/content/roadmap";
import { CONTAINER, ICON } from "@/lib/constants";
import { COMMITS_URL, fetchCommits } from "@/lib/github";
import { isShallow, readGitLog } from "@/lib/git-log";
import { ogImageMetadata } from "@/lib/og-image";
import { readPageContent, type PageContent } from "@/lib/page-content";
import { linkRepoFile, readRepoDoc } from "@/lib/repo-docs";

/** Social image copy, rendered by ./og-image/route.tsx. */
export const og = {
  eyebrow: "Project status",
  title: "Six milestones complete, two partial.",
  description:
    "What the language and its toolchain do today, what is still being widened, and why self-hosting is a completeness test rather than a gate.",
  alt: "Cinnabar social card — the project status.",
};

export const metadata: Metadata = {
  title: "Roadmap",
  description:
    "What Cinnabar does today: linear types checked on every path, direct system calls, freestanding static binaries — and self-hosting after that.",
  alternates: { canonical: "/roadmap/" },
  ...ogImageMetadata("/roadmap/", og),
};

/**
 * One capability, as a cell in the hairline grid.
 *
 * Deliberately small. There are fourteen of these on the page and the section
 * they lead is answering one question — what can the language do — so a reader
 * has to be able to take the set in at a glance rather than scroll a column of
 * full-height cards. Hence the 20px icon rather than the 24px one the home
 * page's highlights use, the tighter padding, and body copy a step down from
 * the site's ordinary 14.5px.
 */
function CapabilityCard({
  capability,
  content,
  delay,
}: {
  capability: Capability;
  /** Supplies the title and the description, from the `@capabilities` block. */
  content: PageContent;
  delay: number;
}) {
  const Icon = capability.icon;
  // Throws at build time naming the slug if content.md has no section for it.
  const { title, body } = content.item("capabilities", capability.slug);
  return (
    <Reveal
      delay={delay}
      className="bg-panel hover:bg-panel-raised panel-hover flex flex-col gap-3 p-5 sm:p-6"
    >
      <Icon size={ICON.section} className="text-text" />
      <h3 className="text-[15px] leading-snug font-bold tracking-[-0.015em]">
        <a
          href={capability.anchor}
          className="text-text hover:text-cinnabar-text panel-hover"
        >
          {title}
        </a>
      </h3>
      <p className="text-secondary text-[13px] leading-[1.6] text-pretty">{body}</p>
    </Reveal>
  );
}

export default async function RoadmapPage() {
  const [document, content, commits] = await Promise.all([
    readRepoDoc("ROADMAP.md"),
    readPageContent("roadmap"),
    /*
     * The build's copy of the commit feed, prerendered into the HTML so the
     * section is correct without JavaScript and when GitHub cannot be reached.
     * `fetchCommits` never rejects, so a build behind a firewall or with the
     * API rate-limited produces the section's static fallback rather than
     * failing the deploy. The timeout is what stops a hung request hanging it.
     */
    fetchCommits({ timeoutMs: 5000, revalidateSeconds: 900 }),
  ]);

  /*
   * The history the section's windows are measured against, read from the
   * repository this site is built inside rather than from the API.
   *
   * The API path cannot supply it: `/repos/{repo}/commits` pages at a hundred,
   * so any real history is several requests, and this repository lands about
   * fifteen commits on a day it moves at all — a window of thirty commits is a
   * window of two days. git has the whole log locally and costs nothing.
   */
  const history = readGitLog();

  /*
   * A swallowed failure is invisible by construction, and this one already
   * hid a real mistake once: an earlier version asked for `cache: "no-store"`,
   * which Next rejects inside a statically exported page, so every build
   * silently shipped the fallback. The page still renders — the fallback is a
   * correct page, not a broken one — but the build says so.
   */
  if (commits.length === 0) {
    console.warn(
      "[roadmap] GitHub returned no commits at build time; the activity feed" +
        " will ship the repository's own log and fill in from the browser.",
    );
  }

  /*
   * Both of these are silent failures too, and both change what the section
   * claims rather than whether it renders: with no log the windows collapse to
   * whatever the API returned, and from a shallow clone "All" means "all of
   * the depth this checkout was given". Deploys here are run by hand from a
   * full clone, so either line means something changed about how the site is
   * built.
   */
  if (history.length === 0) {
    console.warn(
      "[roadmap] git returned no commits at build time; the activity feed's" +
        " windows will cover only the commits the API returned.",
    );
  } else if (isShallow()) {
    console.warn(
      "[roadmap] the checkout is shallow, so the activity feed's history" +
        " reaches only as far back as the clone depth.",
    );
  }

  return (
    <article className="pb-28">
      <PageHeader
        section="Roadmap"
        note="ROADMAP.md · read from the source"
        icon={CheckIcon}
        title="Six milestones complete, two partial."
        lede={content.block("lede")}
      />

      {/* The shape of the project in three numbers. */}
      <section className={`${CONTAINER} pt-14`}>
        <div className="rule-grid grid grid-cols-2 sm:grid-cols-4">
          <Stat value={SHIPPED.length} label="Capabilities shipped" />
          <Stat
            value={`${MILESTONE_TALLY.complete} / ${MILESTONE_TALLY.total}`}
            label="Milestones complete"
          />
          <Stat value={IN_PROGRESS.length} label="Marked partial" />
          {/* ROADMAP.md's own definition of done, rather than a fourth number. */}
          <Panel className="bg-ground justify-center gap-2 p-6">
            <span className="text-secondary text-[13px] leading-[1.55] text-pretty">
              A milestone is done only when the fixtures, the sanitizer gate and
              the spec all agree.
            </span>
          </Panel>
        </div>
      </section>

      {/*
        Everything the language does today, in one grid.

        Six of these used to lead and six sat behind a "the other six
        capabilities" fold, which is the wrong trade for a list whose whole job
        is to be complete: it made the language look half as capable as it is,
        and it asked the reader to click before they knew whether clicking was
        worth it. Four across at xl puts all twelve in three rows.
      */}
      <section className={`${CONTAINER} pt-20`}>
        <SectionHeading
          title="What Cinnabar does today"
          note={content.block("shipped-note")}
          icon={CheckIcon}
        />
        <div className="rule-grid mt-11 grid sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          {SHIPPED.map((capability, index) => (
            <CapabilityCard
              key={capability.slug}
              capability={capability}
              content={content}
              // Staggered by row rather than by card: twelve cards at 0.03s
              // each would still be arriving a third of a second after the
              // first one landed.
              delay={Math.min(index * 0.02, 0.16)}
            />
          ))}
        </div>
      </section>

      {/* The two partials, stated plainly rather than as open tickets. */}
      <section className={`${CONTAINER} pt-24`}>
        <SectionHeading title="Still widening" note="Marked partial" icon={RunIcon} />
        <Reveal className="mt-9">
          <p className="text-secondary max-w-[86ch] text-[16.5px] leading-[1.75] text-pretty">
            {content.block("progress")}
          </p>
        </Reveal>
        <div className="rule-grid mt-9 grid sm:grid-cols-2">
          {IN_PROGRESS.map((capability, index) => (
            <CapabilityCard
              key={capability.slug}
              capability={capability}
              content={content}
              delay={index * 0.04}
            />
          ))}
        </div>
      </section>

      {/* The horizon. */}
      <section className={`${CONTAINER} pt-24`}>
        <SectionHeading
          title="On the horizon"
          note={content.block("horizon-note")}
          icon={LinearIcon}
        />
        <Reveal className="mt-11">
          <Callout>
            <Eyebrow>Next</Eyebrow>
            <h3 className="text-text text-[28px] leading-tight font-bold tracking-tight sm:text-[36px]">
              {content.item("horizon", HORIZON.slug).title}
            </h3>
            <p className="text-bright max-w-[80ch] text-[17px] leading-[1.7] text-pretty">
              {content.item("horizon", HORIZON.slug).body}
            </p>
            <ArrowLink href={HORIZON.anchor}>Read the reasoning</ArrowLink>
          </Callout>
        </Reveal>
      </section>

      {/*
        Evidence, between the plan and the record: the plan above says what is
        coming, the document below says what was decided, and this says the
        work is happening. It sits on this page rather than on the home page
        because this is the page a reader opens to ask whether the project is
        alive, and one instance of it spends one of a reader's sixty
        unauthenticated GitHub requests rather than two.
      */}
      <section className={`${CONTAINER} pt-24`}>
        <SectionHeading
          title="Recent activity"
          note={content.block("activity-note")}
          icon={GitHubIcon}
        />
        <Reveal className="mt-9">
          <ActivityFeed
            history={history}
            initial={commits}
            fallback={content.block("activity-fallback")}
          />
          {/*
            Outside the feed on purpose. It is the one thing in this section
            that is true whatever GitHub answered, so it is the one thing that
            is never conditional.
          */}
          <div className="mt-6">
            <ArrowLink href={COMMITS_URL} external>
              The full commit log
            </ArrowLink>
          </div>
        </Reveal>
      </section>

      <section className={`${CONTAINER} pt-24 pb-12`}>
        <SectionHeading title="The full record" note="ROADMAP.md" />
        <SourceNote className="mt-10">
          {linkRepoFile(content.block("source"), "ROADMAP.md")}
        </SourceNote>
      </section>

      {/*
        The document itself, folded. The capability cards above link to anchors
        inside it; browsers expand a closed <details> when a fragment resolves
        into it, so those links still land.
      */}
      <div className={CONTAINER}>
        <Disclosure summary={content.block("document")}>
          <DocBody markdown={document} tocLabel="Milestones" />
        </Disclosure>
      </div>
    </article>
  );
}
