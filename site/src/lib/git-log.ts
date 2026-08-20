import { execFileSync } from "node:child_process";
import { HISTORY_LIMIT, makeCommit, type Commit } from "@/lib/github";

/*
 * The repository's own log, read at build time.
 *
 * The activity section used to be the last thirty commits and nothing else,
 * which for this repository was about two days: it lands roughly fifteen
 * commits on a day it moves at all, so a window measured in commits is a
 * window measured in hours. A window measured in days has to come from the
 * whole log, and the whole log is already here — the site is built from inside
 * the repository it describes, the same way the roadmap and manifesto pages
 * are rendered from ROADMAP.md and MANIFESTO.md.
 *
 * Reading it here rather than from the API is not only cheaper, it is the only
 * way to get it at all: `/repos/{repo}/commits` pages at a hundred, so a
 * history of any length is several requests, and the reader's unauthenticated
 * budget is sixty an hour and belongs to them. The API refresh in lib/github.ts
 * still runs in the browser, but only to cover the commits that landed since
 * the deploy — which is one page however busy the week was.
 *
 * This module is imported by a server component only. `node:child_process` in
 * anything reachable from a client component would fail the build.
 */

/** Separates fields within a record, and records from each other. */
const FIELD = "\x1f";
const RECORD = "\x1e";

/**
 * `%at` is the author timestamp as a Unix epoch, which is an instant rather
 * than a wall clock, so converting it here gives the same UTC date the API's
 * ISO-8601 `commit.author.date` parses to. `--date=short` would give git's
 * idea of the author's local day instead, and the two disagree for anyone
 * committing near midnight in a non-UTC zone.
 */
const FORMAT = `%H${FIELD}%at${FIELD}%s${RECORD}`;

/** Room for `HISTORY_LIMIT` records. Subjects are one line, so this is ample. */
const MAX_BUFFER = 8 * 1024 * 1024;

function git(...args: string[]): string {
  return execFileSync("git", args, {
    encoding: "utf8",
    maxBuffer: MAX_BUFFER,
    // The build runs in site/; git resolves the work tree from any subdirectory.
    windowsHide: true,
  });
}

/**
 * The commits reachable from HEAD, newest first, capped at `HISTORY_LIMIT`.
 *
 * HEAD rather than `main` because HEAD is what was built, and a preview built
 * from a branch should describe that branch. They are the same commit for the
 * deploys that matter.
 *
 * Never throws. A tree with no git — an unpacked tarball, a sandbox without
 * the binary — returns no commits, and the page falls back to the API list it
 * fetched alongside this, which is exactly the behaviour it had before.
 */
export function readGitLog(limit: number = HISTORY_LIMIT): Commit[] {
  let output: string;
  try {
    output = git("log", `--max-count=${limit}`, `--format=${FORMAT}`);
  } catch {
    return [];
  }

  const commits: Commit[] = [];
  for (const record of output.split(RECORD)) {
    const line = record.trim();
    if (line.length === 0) continue;

    const [sha, at, subject] = line.split(FIELD);
    const seconds = Number(at);
    if (!sha || !Number.isFinite(seconds) || !subject) continue;

    commits.push(
      makeCommit(sha, subject, new Date(seconds * 1000).toISOString().slice(0, 10)),
    );
  }
  return commits;
}

/**
 * Whether the checkout is shallow, and so whether the history above reaches
 * the first commit.
 *
 * Worth knowing at build time because a shallow clone is silent: the log is
 * simply shorter, and a section that says "All" would be describing whatever
 * depth the CI runner happened to fetch. The deploys here are run by hand from
 * a full clone; this is what would say so if that ever changed.
 */
export function isShallow(): boolean {
  try {
    return git("rev-parse", "--is-shallow-repository").trim() === "true";
  } catch {
    return false;
  }
}
