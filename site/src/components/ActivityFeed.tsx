"use client";

import { useEffect, useMemo, useState } from "react";
import {
  CADENCE_DAYS,
  activeDays,
  activityAreas,
  cadence,
  fetchCommits,
  groupByDay,
  readCachedCommits,
  sessionCache,
  writeCachedCommits,
  type CadenceDay,
  type Commit,
} from "@/lib/github";

/*
 * The recent-commit feed: a summary of the window, a filter across the areas
 * it touched, and the log itself.
 *
 * The list this renders first is the one the build baked into the HTML, so the
 * section is complete before any script runs and stays complete if none ever
 * does. On mount it looks for a fresh list — from sessionStorage first, then
 * from GitHub — and swaps it in silently.
 *
 * What this component never does, in any state:
 *
 * - show a spinner. There is already something correct on screen; a spinner
 *   over it would be a worse view of the same page.
 * - show an error. A failed fetch leaves the prerendered list exactly where it
 *   was, which is the right outcome and needs no announcement.
 * - change height. Every part of it is a fixed height that does not depend on
 *   how many commits, areas or days are in hand, so the moment fresh data
 *   lands nothing below it moves — and nothing inside it moves either.
 *
 * That last constraint is what decides most of the layout below. It is why the
 * log scrolls inside a frame rather than growing to fit thirty commits, why
 * the filter chips scroll sideways on one line rather than wrapping onto a
 * second, and why the rows clip their subject rather than wrapping it. A
 * commit message is arbitrary text and the number of areas a fortnight touches
 * is arbitrary too; a section whose height is a function of either cannot
 * reserve its space.
 *
 * Nothing here is computed from the current time. Every number and every date
 * on screen is a function of the commit list alone, so the server's render and
 * the browser's agree and there is no hydration mismatch to reconcile.
 */

/* ---------------------------------------------------------------- metrics -- */

/** One commit row. The height the log's frame is measured in. */
const ROW = "min-h-11";

/** One day header. */
const DAY_HEADER = "h-8";

/**
 * The log's viewport: one day header and six rows — 32 + 6 × 44.
 *
 * A whole number of rows, so what is cut off at the bottom edge is a row
 * mid-way through rather than a row that looks like the last one. That partial
 * row is the affordance: it is what says the frame scrolls.
 */
const LOG_VIEWPORT = "h-[296px]";

/**
 * The height the whole section holds, in either state.
 *
 * The summary, the filter and the log assemble to 612px — 228 + 20 + 48 + 20 +
 * 296, and within a pixel of that at every breakpoint, because every one of
 * those parts is a fixed height. The fallback pads to the same figure.
 *
 * The case that needs it is narrow but real: a build that could not reach
 * GitHub ships the fallback, and a reader whose own request succeeds gets the
 * feed swapped in underneath the rest of the page. Pinned here, that swap
 * moves nothing either.
 *
 * tests/e2e/github-activity.spec.ts asserts this height in every state,
 * including the ones where nothing was fetched at all.
 */
const SLOT = "min-h-[612px]";

/* --------------------------------------------------------------- summary -- */

/** A figure with its label — the roadmap's `Stat`, without the ui.tsx import.
 *
 * Duplicated rather than imported on purpose: `ui.tsx` pulls in the markdown
 * renderer, and importing anything from it here would drag react-markdown, the
 * syntax highlighter and the diagram component into this client bundle to set
 * three numbers.
 */
function Figure({ value, label }: { value: number; label: string }) {
  return (
    <div className="bg-panel flex flex-col gap-2 p-5 sm:p-6">
      <span className="text-text text-[32px] leading-none font-bold tracking-[-0.03em]">
        {value}
      </span>
      <span className="text-label font-mono text-[10px] tracking-[0.16em] uppercase">
        {label}
      </span>
    </div>
  );
}

/**
 * Commits per day, as a column per day.
 *
 * Drawn with `preserveAspectRatio="none"` over a 100-unit-tall viewBox so the
 * chart stretches to whatever width the cell is without the columns changing
 * proportion. A day with nothing in it keeps a two-unit stub in the hairline
 * colour rather than disappearing, so the row of columns reads as a calendar
 * with gaps rather than as a chart with fewer bars than days.
 *
 * `role="img"` with a written label: the shape is the point, and a screen
 * reader is better served by the sentence than by twenty-eight rectangles.
 */
function Cadence({ series }: { series: readonly CadenceDay[] }) {
  const peak = Math.max(1, ...series.map((day) => day.count));
  const landed = series.filter((day) => day.count > 0).length;
  // The caller only ever draws this with a full series; the fallbacks are so
  // that an empty one degrades to an empty frame rather than to a broken
  // viewBox and a thrown read.
  const first = series[0]?.date ?? "";
  const last = series[series.length - 1]?.date ?? "";
  const width = Math.max(1, series.length * 4 - 1);

  return (
    <div className="bg-panel col-span-3 flex flex-col gap-3 p-5 sm:p-6">
      <svg
        viewBox={`0 0 ${width} 100`}
        preserveAspectRatio="none"
        className="h-12 w-full"
        role="img"
        aria-label={
          `Commits per day over the ${series.length} days ending ${last}: ` +
          `something landed on ${landed} of them, at most ${peak} in a day.`
        }
      >
        {series.map((day, index) => {
          // Six units is the shortest a real column can be and still read as
          // taller than an empty day's stub.
          const height = day.count === 0 ? 2 : Math.max(6, (day.count / peak) * 100);
          return (
            <rect
              key={day.date}
              x={index * 4}
              y={100 - height}
              width={3}
              height={height}
              className={day.count === 0 ? "fill-hairline-strong" : "fill-cinnabar"}
            />
          );
        })}
      </svg>
      {/*
        The axis. Every grey here is `--label` rather than `--mute`: the board
        sets its 10px mono labels in the two darkest greys, and globals.css
        records that both fall under AA on this ground, so `--label` is the
        quietest token that is allowed to carry text.
      */}
      <div className="text-label flex items-baseline justify-between gap-4 font-mono text-[10px] tracking-[0.12em] uppercase">
        <span className="text-secondary">{first}</span>
        <span className="hidden sm:block">Commits per day</span>
        <span className="text-secondary">{last}</span>
      </div>
    </div>
  );
}

/* ---------------------------------------------------------------- filter -- */

function Chip({
  label,
  count,
  pressed,
  onClick,
}: {
  label: string;
  count: number;
  pressed: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      onClick={onClick}
      className={`panel-hover pressable flex h-9 flex-none items-center gap-2 border px-3.5 font-mono text-[11px] tracking-[0.08em] ${
        pressed
          ? "border-cinnabar bg-panel-raised text-text"
          : "border-hairline text-secondary hover:border-hairline-strong hover:bg-panel"
      }`}
    >
      {label}
      <span className={pressed ? "text-cinnabar-text" : "text-label"}>{count}</span>
    </button>
  );
}

/* ------------------------------------------------------------------- log -- */

function Row({ commit, emphasis }: { commit: Commit; emphasis: string | null }) {
  /*
   * One area named, not all of them.
   *
   * `resolver · codegen` does not fit the column and truncating it produces
   * `RESOLVER · COD…`, which is worse than saying there is another. Which one
   * is named is the filtered-on area when there is one, so a reader who asked
   * for `codegen` is not shown a column of rows labelled `resolver`.
   */
  const named =
    emphasis && commit.areas.includes(emphasis) ? emphasis : commit.areas[0];
  const others = commit.areas.length - 1;

  return (
    <li>
      <a
        href={commit.url}
        target="_blank"
        rel="noopener noreferrer"
        // The whole row is the target, so the link is a row rather than a
        // seven-character sha nobody can hit on a phone.
        className={`hover:bg-panel-raised panel-hover flex items-center gap-4 px-4 sm:gap-5 sm:px-5 ${ROW}`}
        data-commit={commit.sha}
      >
        <span className="text-cinnabar-text flex-none font-mono text-[11px] tracking-[0.04em]">
          {commit.abbrev}
        </span>
        {/*
          A fixed column rather than a shrink-to-fit badge, so the subjects
          start on one line down the log instead of stepping in and out with
          the length of each area name. Reserved even for a commit that names
          no area — a merge, or one of the older conventional-commits subjects
          — so those rows stay in the same column as the rest. Hidden on a
          phone, where 112px of it would be a third of the row.
        */}
        <span className="text-label hidden w-28 flex-none truncate font-mono text-[10.5px] tracking-widest uppercase sm:block">
          {named}
          {others > 0 ? <span className="normal-case"> +{others}</span> : null}
        </span>
        {/*
          `truncate` needs a minimum width of zero to clip inside a flex row;
          without it the cell grows to fit the message and the row stops being
          44px tall.
        */}
        <span
          className="text-secondary min-w-0 flex-1 truncate text-[13.5px]"
          title={commit.subject}
        >
          {commit.title}
        </span>
      </a>
    </li>
  );
}

function Day({
  date,
  commits,
  emphasis,
}: {
  date: string;
  commits: readonly Commit[];
  emphasis: string | null;
}) {
  const label = `commits-${date}`;
  return (
    // `h-full` matters when the log is not full: the grid stretches its tracks
    // to fill the frame, and a cell that does not grow with them leaves the
    // hairline showing as a band rather than as a rule.
    <li className="bg-panel flex h-full min-w-0 flex-col">
      <div
        id={label}
        className={`border-hairline text-label flex flex-none items-center justify-between gap-4 border-b px-4 font-mono text-[10px] tracking-[0.16em] uppercase sm:px-5 ${DAY_HEADER}`}
      >
        <span className="text-secondary">{date}</span>
        <span>{commits.length === 1 ? "1 commit" : `${commits.length} commits`}</span>
      </div>
      <ol aria-labelledby={label}>
        {commits.map((commit) => (
          <Row key={commit.sha} commit={commit} emphasis={emphasis} />
        ))}
      </ol>
    </li>
  );
}

/* ----------------------------------------------------------------- feed -- */

export type ActivityFeedProps = {
  /** Fetched at build time. Rendered as-is until something better arrives. */
  initial: readonly Commit[];
  /**
   * Shown when there is nothing to list. Plain prose from the route's
   * content.md — not markdown, because rendering markdown here would pull the
   * whole document renderer, the syntax highlighter and the diagram component
   * into the client bundle to set one sentence.
   */
  fallback: string;
};

export default function ActivityFeed({ initial, fallback }: ActivityFeedProps) {
  // `initial` is what the server rendered, so the first client render matches
  // it exactly and there is no hydration mismatch to reconcile.
  const [commits, setCommits] = useState<readonly Commit[]>(initial);
  const [selected, setSelected] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    const storage = sessionCache();
    // A reader moving between pages within the TTL costs no request at all,
    // which is the point: the unauthenticated budget is sixty an hour and it
    // is theirs, not the site's.
    const cached = readCachedCommits(storage, Date.now());

    /*
     * Both paths resolve through a promise so the state update is always in a
     * callback. Setting state synchronously in an effect body is a cascading
     * render, and the React Compiler's lint rules reject it outright.
     */
    const pending = cached
      ? Promise.resolve(cached)
      : fetchCommits({ signal: controller.signal });

    void pending.then((fresh) => {
      // An empty result is every failure mode at once — offline, rate
      // limited, blocked by an extension, garbage JSON — and the response to
      // all of them is to leave the prerendered list alone.
      if (fresh.length === 0 || controller.signal.aborted) return;
      setCommits(fresh);
      if (!cached) writeCachedCommits(storage, fresh, Date.now());
    });

    return () => controller.abort();
  }, []);

  const areas = useMemo(() => activityAreas(commits), [commits]);

  /*
   * The selection, reconciled against the areas actually in hand.
   *
   * Derived rather than corrected in an effect: the fresh list can touch
   * different areas from the one the build prerendered, and a filter left
   * pointing at an area that is no longer there would empty the log. Reading
   * it this way means the stale selection simply stops applying, in the same
   * render that replaced the data.
   */
  const active = selected && areas.some((a) => a.area === selected) ? selected : null;

  const shown = useMemo(
    () => (active ? commits.filter((commit) => commit.areas.includes(active)) : commits),
    [commits, active],
  );

  if (commits.length === 0) {
    return (
      <div
        data-activity="fallback"
        className={`border-hairline bg-panel flex flex-col justify-center gap-3 border p-6 sm:p-8 ${SLOT}`}
      >
        <p className="text-secondary max-w-[62ch] text-[14.5px] leading-[1.65] text-pretty">
          {fallback}
        </p>
      </div>
    );
  }

  const series = cadence(shown, CADENCE_DAYS);

  return (
    <div data-activity="commits" className={`flex flex-col gap-5 ${SLOT}`}>
      {/*
        The window in three numbers and a shape. All four read the filtered
        list, so a reader who narrows to one area is told about that area
        rather than about the window they just narrowed away from.
      */}
      <div className="rule-grid grid grid-cols-3">
        <Figure value={shown.length} label="Commits" />
        <Figure value={activeDays(shown)} label="Active days" />
        <Figure value={activityAreas(shown).length} label="Areas touched" />
        <Cadence series={series} />
      </div>

      {/*
        One line, always. `overflow-x-auto` rather than `flex-wrap` because a
        second row of chips would be a second row of height, and the height of
        this section cannot depend on how many subsystems a fortnight happened
        to touch.

        The row is 48px for 36px chips so that the scrollbar, where the
        platform draws a real one, has its own 12px to sit in. Sized instead to
        the chips, the row would grow by the height of a scrollbar the moment
        one more area appeared than fits — which is the same shift, arriving by
        a subtler route.
      */}
      <div
        role="group"
        aria-label="Filter the log by area"
        className="flex h-12 flex-none items-start gap-2 overflow-x-auto scrollbar-thin"
      >
        <Chip
          label="All"
          count={commits.length}
          pressed={active === null}
          onClick={() => setSelected(null)}
        />
        {areas.map((entry) => (
          <Chip
            key={entry.area}
            label={entry.area}
            count={entry.count}
            pressed={active === entry.area}
            onClick={() => setSelected(active === entry.area ? null : entry.area)}
          />
        ))}
      </div>

      {/*
        The log. `tabIndex` is not decoration: a region that scrolls has to be
        reachable and scrollable from the keyboard, and the focus ring the site
        already draws is the affordance that says so.
      */}
      <ol
        tabIndex={0}
        aria-label={active ? `Commits touching ${active}` : "Recent commits"}
        className={`rule-grid grid grid-cols-1 overflow-y-auto scrollbar-thin ${LOG_VIEWPORT}`}
      >
        {groupByDay(shown).map((day) => (
          <Day
            key={day.date}
            date={day.date}
            commits={day.commits}
            emphasis={active}
          />
        ))}
      </ol>
    </div>
  );
}
