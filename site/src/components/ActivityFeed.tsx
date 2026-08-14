"use client";

import { useEffect, useState } from "react";
import {
  fetchCommits,
  readCachedCommits,
  sessionCache,
  writeCachedCommits,
  type Commit,
} from "@/lib/github";

/*
 * The recent-commit feed.
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
 * - change height. The slot reserves the full five rows, so the moment fresh
 *   data lands nothing below it moves.
 *
 * The last of those is why the rows are a fixed 44px with the subject clipped
 * rather than wrapped: a commit message is arbitrary text, and a feed whose
 * height depends on how verbose the last commit was cannot reserve its space.
 */

/**
 * One row: 44px, and the whole of whatever track it ends up in.
 *
 * `min-h-11` sets the height the slot is sized from. `h-full` matters when
 * there are fewer than five commits: a grid stretches its auto tracks to fill
 * a taller container, so the row grows and the link has to grow with it —
 * otherwise the bottom of a visibly clickable row is not clickable.
 */
const ROW = "h-full min-h-11";

/**
 * The height five rows occupy in a `.rule-grid`, reserved whether or not there
 * is anything to put in it: 5 × 44px, plus the four 1px gaps between them,
 * plus the grid's own 1px border top and bottom.
 */
const SLOT = "min-h-[226px]";

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

  return (
    <ol data-activity="commits" className={`rule-grid grid grid-cols-1 ${SLOT}`}>
      {commits.map((commit) => (
        <li key={commit.sha} className="bg-panel min-w-0">
          <a
            href={commit.url}
            target="_blank"
            rel="noopener noreferrer"
            // The whole row is the target, so the link is a row rather than a
            // seven-character sha nobody can hit on a phone.
            className={`hover:bg-panel-raised panel-hover flex items-center gap-4 px-4 sm:gap-5 sm:px-5 ${ROW}`}
          >
            <span className="text-cinnabar-text flex-none font-mono text-[11px] tracking-[0.04em]">
              {commit.abbrev}
            </span>
            {/*
              `truncate` needs a minimum width of zero to clip inside a flex
              row; without it the cell grows to fit the message and the row
              stops being 44px tall.
            */}
            <span
              className="text-secondary min-w-0 flex-1 truncate text-[13.5px]"
              title={commit.subject}
            >
              {commit.subject}
            </span>
            <span className="text-label hidden flex-none font-mono text-[11px] sm:block">
              {commit.date}
            </span>
          </a>
        </li>
      ))}
      {/*
        A repository with fewer than five commits leaves the reserved slot
        partly empty rather than short. Nothing is drawn into the gap: an
        empty row would read as a row that failed to load.
      */}
    </ol>
  );
}
