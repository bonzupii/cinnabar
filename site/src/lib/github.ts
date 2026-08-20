/*
 * The repository's recent commits, read from GitHub's public REST API.
 *
 * Why this shape, given that the site is a static export with no server:
 *
 * - The list is fetched **at build time** and prerendered into the page. That
 *   is what makes the section correct with JavaScript off, with the API
 *   unreachable, and for a crawler. It is accurate at deploy and goes stale
 *   afterwards, which for a manually deployed site is a matter of days.
 * - The same list is fetched **again in the browser** on mount, and replaces
 *   the prerendered one when it arrives. That is the "live" half. A failure
 *   here is not an error state: the prerendered list simply stays.
 *
 * There is no Netlify Function in the middle. The unauthenticated limit is 60
 * requests per hour *per originating IP* (docs.github.com, "Rate limits for
 * the REST API"), so it is spent by the reader, one request per session per
 * TTL, and a normal visit spends one of sixty. A function would raise that to
 * 5,000/hour but only by introducing a token to store and rotate, a runtime
 * dependency where there is currently none, and a second thing that can be
 * down. The prerendered layer already covers the case the function would exist
 * to prevent.
 *
 * Everything here is a pure function over a payload except `fetchCommits`, so
 * tests/unit/github.test.ts can exercise the parsing, the caching and the
 * failure paths without a network.
 */

export const REPO = "bonzupii/cinnabar";

/**
 * How many commits the feed reads, summarises and lists.
 *
 * Thirty rather than the five this started at, because the feed no longer just
 * proves the project is alive — it says what has been moving, and one screen
 * of five rows cannot. Thirty is one page of GitHub's default pagination, so
 * it costs the same single request; it is roughly a fortnight of this
 * repository, which is long enough for the per-day cadence to have a shape;
 * and it is small enough that the prerendered list stays a few kilobytes of
 * HTML. The list scrolls inside a fixed-height frame, so the number does not
 * decide how much of the page the section occupies.
 */
export const ACTIVITY_COUNT = 30;

/**
 * How many days the cadence chart draws, ending on the newest commit's date.
 *
 * Four weeks, so the columns line up week on week. Deliberately anchored to
 * the data rather than to `Date.now()`: a chart that ended today would be one
 * string on the server and another in the browser, which is the hydration
 * mismatch the absolute dates below exist to avoid.
 */
export const CADENCE_DAYS = 28;

/** Where the reader goes for the whole log. */
export const COMMITS_URL = `https://github.com/${REPO}/commits/main/`;

export const COMMITS_ENDPOINT =
  `https://api.github.com/repos/${REPO}/commits` +
  `?sha=main&per_page=${ACTIVITY_COUNT}`;

/** sessionStorage key. Versioned, so a shape change cannot read old entries. */
export const CACHE_KEY = "cinnabar:activity:v1";

/**
 * How long a cached list is served before another request is made.
 *
 * Ten minutes is longer than a reading session and short enough that the feed
 * is not obviously stale. GitHub's own `Cache-Control` on this endpoint is 60
 * seconds; the point of this TTL is not freshness but not spending a reader's
 * sixty requests on a five-page visit.
 */
export const CACHE_TTL_MS = 10 * 60 * 1000;

/** One commit, reduced to what the feed renders. */
export type Commit = {
  /** Full 40-character sha. */
  sha: string;
  /** The seven characters GitHub itself abbreviates to. */
  abbrev: string;
  /** First line of the message. The body, if any, is dropped. */
  subject: string;
  /**
   * The areas the subject names before its colon, lowercased — `["vscode"]`,
   * `["resolver", "codegen"]`, `[]` for a subject that names none.
   *
   * Derived, never read from a payload: see `describeSubject`.
   */
  areas: readonly string[];
  /**
   * The subject with that prefix removed, which is the part a reader is
   * actually scanning. Equal to `subject` when there is no prefix.
   */
  title: string;
  /**
   * Authored date as `YYYY-MM-DD`, in UTC.
   *
   * Deliberately absolute rather than "3 days ago". A relative date computed
   * on the server is wrong by the time anyone reads it, and one computed in
   * the browser disagrees with the prerendered HTML — a hydration mismatch,
   * and a visible reflow on every load. An absolute UTC date is the same
   * string in both places forever.
   */
  date: string;
  /** Permalink, constructed here rather than taken from the payload. */
  url: string;
};

const SHA = /^[0-9a-f]{7,40}$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;

/** Milliseconds in a day. Exact in UTC, which is the only zone used here. */
const DAY_MS = 86_400_000;

/**
 * A subject that opens with an area prefix: everything up to the first colon,
 * then a space, then the rest.
 *
 * The 32-character cap on the prefix is what keeps ordinary prose out. This
 * repository's log contains `amendment to commit message style rule: no AI
 * attribution.`, whose prefix is a sentence rather than an area, and the cap
 * is the cheapest rule that rejects it without a list of known areas — which
 * would go stale the first time a subsystem is added.
 */
const AREA_PREFIX = /^([^:]{1,32}): +(\S.*)$/;

/**
 * One area token, in either of the two conventions the log carries.
 *
 * `AGENTS.md` mandates `area: Subject` and most of the log follows it, but the
 * earlier commits are conventional-commits — `fix(codegen):`, `build(deps):` —
 * and dependabot still writes them. The capture group takes the parenthesised
 * scope when there is one, so `fix(codegen)` and `codegen` are the same area
 * and a reader filtering on `codegen` gets both.
 */
const AREA_TOKEN = /^[a-z][a-z0-9]*(?:[._/-][a-z0-9]+)*(?:\(([a-z][a-z0-9._/-]*)\))?$/;

/** At most this many areas before a prefix stops looking like a prefix. */
const MAX_AREAS = 3;

/**
 * Splits a subject into the areas it names and the rest of it.
 *
 * All or nothing by design: if any token in the prefix is not an area token,
 * the whole subject is left alone rather than half-parsed. A subject is a
 * sentence a human wrote, so the failure this guards against is not a
 * malformed area but a colon appearing in ordinary prose — and there, showing
 * the subject unchanged is exactly right.
 */
export function describeSubject(subject: string): {
  areas: string[];
  title: string;
} {
  const whole = { areas: [], title: subject };

  const match = AREA_PREFIX.exec(subject);
  if (!match) return whole;

  const [, prefix, rest] = match;
  const tokens = prefix.split(",").map((token) => token.trim().toLowerCase());
  if (tokens.length > MAX_AREAS) return whole;

  const areas: string[] = [];
  for (const token of tokens) {
    const parsed = AREA_TOKEN.exec(token);
    if (!parsed) return whole;
    areas.push(parsed[1] ?? token);
  }
  // `fix(lsp), lsp:` would otherwise list the same area twice.
  return { areas: [...new Set(areas)], title: rest };
}

/**
 * The permalink for a sha.
 *
 * Always constructed, never read from a payload — not from the API's own
 * `html_url` and not from the session cache. Both are data from outside this
 * code being turned into an href on our own page, and a validated sha is the
 * only part of either that needs to be trusted for the link to be right.
 */
function commitUrl(sha: string): string {
  return `https://github.com/${REPO}/commit/${sha}`;
}

/**
 * Builds a commit from the only three things either source is trusted for: a
 * validated sha, a subject and a date.
 *
 * Everything else on a `Commit` — the abbreviation, the permalink, the areas,
 * the title — is computed here rather than read. That is what lets the session
 * cache store whole commits and still be treated as untrusted input: a
 * poisoned `url` or `areas` in storage is not read, so it cannot be rendered.
 */
function makeCommit(sha: string, subject: string, date: string): Commit {
  const { areas, title } = describeSubject(subject);
  return {
    sha,
    abbrev: sha.slice(0, 7),
    subject,
    areas,
    title,
    date,
    url: commitUrl(sha),
  };
}

/** Storage-shaped enough for the cache, so a test can pass a plain object. */
export type CacheStorage = {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
};

function field(value: unknown, ...path: string[]): unknown {
  let current = value;
  for (const key of path) {
    if (typeof current !== "object" || current === null) return undefined;
    current = (current as Record<string, unknown>)[key];
  }
  return current;
}

/**
 * Turns whatever the endpoint returned into commits, dropping anything that
 * does not have the four fields the feed needs.
 *
 * Written to survive every non-array body the endpoint actually produces: a
 * rate-limit body is `{ "message": "API rate limit exceeded…" }`, a 404 is
 * `{ "message": "Not Found" }`, and an empty repository returns `{}` rather
 * than `[]`. All three parse to no commits rather than throwing.
 */
export function parseCommits(payload: unknown): Commit[] {
  if (!Array.isArray(payload)) return [];

  const commits: Commit[] = [];
  for (const entry of payload) {
    const sha = field(entry, "sha");
    const message = field(entry, "commit", "message");
    const date =
      field(entry, "commit", "author", "date") ??
      field(entry, "commit", "committer", "date");

    if (typeof sha !== "string" || !SHA.test(sha)) continue;
    if (typeof message !== "string" || typeof date !== "string") continue;

    const subject = message.split("\n")[0].trim();
    // `Date` rather than a slice, so a malformed timestamp is rejected rather
    // than rendered as its own first ten characters.
    const parsed = new Date(date);
    if (subject.length === 0 || Number.isNaN(parsed.getTime())) continue;

    commits.push(makeCommit(sha, subject, parsed.toISOString().slice(0, 10)));
  }
  return commits.slice(0, ACTIVITY_COUNT);
}

/**
 * Checks a list that claims to already be `Commit[]` — what comes back out of
 * sessionStorage.
 *
 * Separate from `parseCommits` because it reads a different shape: the API's
 * nested payload, versus the flat one this module stores. It is not skipped:
 * sessionStorage is writable by anything running on this origin, so an entry
 * is untrusted input like any other. Only the sha, the subject and the date
 * are read; `makeCommit` derives everything else, so no stored `url`,
 * `abbrev`, `areas` or `title` can be poisoned into the page.
 */
export function reviveCommits(value: unknown): Commit[] {
  if (!Array.isArray(value)) return [];

  const commits: Commit[] = [];
  for (const entry of value) {
    const sha = field(entry, "sha");
    const subject = field(entry, "subject");
    const date = field(entry, "date");

    if (typeof sha !== "string" || !SHA.test(sha)) continue;
    if (typeof subject !== "string" || subject.length === 0) continue;
    if (typeof date !== "string" || !DATE.test(date)) continue;

    commits.push(makeCommit(sha, subject, date));
  }
  return commits.slice(0, ACTIVITY_COUNT);
}

/* -------------------------------------------------------------- reading -- */

/*
 * What follows turns a list of commits into the things the feed states about
 * it. Every one is a pure function of the list alone — never of `Date.now()`,
 * never of anything ambient — which is what lets the same numbers be computed
 * on the server for the prerender and again in the browser after a refresh
 * without the two disagreeing.
 */

/** An area and how many of the commits in hand touched it. */
export type AreaCount = { area: string; count: number };

/**
 * The areas the given commits touch, busiest first, ties broken by name.
 *
 * A commit naming two areas counts once for each: the question the filter
 * answers is "show me what touched the resolver", and a commit that touched
 * the resolver and the backend is an answer to it.
 */
export function activityAreas(commits: readonly Commit[]): AreaCount[] {
  const counts = new Map<string, number>();
  for (const commit of commits) {
    for (const area of commit.areas) {
      counts.set(area, (counts.get(area) ?? 0) + 1);
    }
  }
  return [...counts]
    .map(([area, count]) => ({ area, count }))
    .sort((a, b) => b.count - a.count || a.area.localeCompare(b.area));
}

/** Distinct dates among the given commits — days the repository moved. */
export function activeDays(commits: readonly Commit[]): number {
  return new Set(commits.map((commit) => commit.date)).size;
}

/** The commits of one day, newest day first, in the order they arrived. */
export type CommitDay = { date: string; commits: Commit[] };

/**
 * Groups commits by date, preserving the order they came in.
 *
 * The endpoint returns newest first and this does not re-sort, so a day header
 * describes the rows under it whatever order the source chose. Grouping is by
 * the `YYYY-MM-DD` string rather than by a parsed date: the strings are
 * already normalised to UTC, and comparing them cannot drift into the reader's
 * own zone.
 */
export function groupByDay(commits: readonly Commit[]): CommitDay[] {
  const days: CommitDay[] = [];
  for (const commit of commits) {
    const last = days.at(-1);
    if (last?.date === commit.date) last.commits.push(commit);
    else days.push({ date: commit.date, commits: [commit] });
  }
  return days;
}

/** One column of the cadence chart. */
export type CadenceDay = { date: string; count: number };

/**
 * Commits per day over the `days` ending on the newest date in the list.
 *
 * Anchored to the data rather than to today for the reason given on
 * `CADENCE_DAYS`, and it means the chart is still a true picture of the window
 * if the repository goes quiet — the last column is the last day something
 * landed, and the dates under the chart say when that was.
 *
 * Days with nothing in them are present with a count of zero rather than
 * absent: the gaps are the point of the chart.
 */
export function cadence(
  commits: readonly Commit[],
  days: number = CADENCE_DAYS,
): CadenceDay[] {
  if (commits.length === 0 || days <= 0) return [];

  const counts = new Map<string, number>();
  let newest = commits[0].date;
  for (const commit of commits) {
    counts.set(commit.date, (counts.get(commit.date) ?? 0) + 1);
    // Lexicographic order is chronological order for `YYYY-MM-DD`.
    if (commit.date > newest) newest = commit.date;
  }

  const end = Date.parse(`${newest}T00:00:00Z`);
  const series: CadenceDay[] = [];
  for (let back = days - 1; back >= 0; back -= 1) {
    const date = new Date(end - back * DAY_MS).toISOString().slice(0, 10);
    series.push({ date, count: counts.get(date) ?? 0 });
  }
  return series;
}

export type FetchOptions = {
  signal?: AbortSignal;
  /** Aborts the request after this long. Used by the build, where a hung
   *  request would hang the deploy. Ignored when `signal` is given. */
  timeoutMs?: number;
  /** Injected by the unit tests. */
  fetchImpl?: typeof fetch;
  /**
   * Seconds Next may reuse the build's copy of this response for.
   *
   * Set only by the build. `cache: "no-store"` is the obvious way to ask for a
   * live response and is the wrong one here: an uncached fetch inside a page
   * makes that page dynamic, and under `output: "export"` Next refuses to
   * render it — `NEXT_STATIC_GEN_BAILOUT`, caught by the handler below and
   * therefore invisible except as a feed that is silently always empty. A
   * finite revalidate keeps the route static. Consecutive builds on one
   * machine within the window reuse `.next/cache`; a clean checkout, which is
   * what a deploy usually is, always fetches.
   */
  revalidateSeconds?: number;
};

/**
 * Fetches the recent commits. Never throws and never rejects.
 *
 * Every failure — offline, DNS, CORS, 403 rate limit, 404, malformed JSON —
 * returns an empty list, which every caller treats as "keep what you have".
 * There is deliberately no error channel: nothing on the page would do
 * anything with one except render it, and a commit feed is not worth an error
 * message.
 */
export async function fetchCommits(options: FetchOptions = {}): Promise<Commit[]> {
  const { signal, timeoutMs, fetchImpl = fetch, revalidateSeconds } = options;
  try {
    const response = await fetchImpl(COMMITS_ENDPOINT, {
      // Only CORS-safelisted headers, so the browser makes the request
      // directly rather than preflighting it.
      headers: { Accept: "application/vnd.github+json" },
      signal:
        signal ??
        (timeoutMs !== undefined ? AbortSignal.timeout(timeoutMs) : undefined),
      ...(revalidateSeconds !== undefined
        ? { next: { revalidate: revalidateSeconds } }
        : {}),
    });
    if (!response.ok) return [];
    return parseCommits(await response.json());
  } catch {
    return [];
  }
}

type CacheEntry = { at: number; commits: Commit[] };

/** Reads the cached list, or undefined when there is none or it has expired. */
export function readCachedCommits(
  storage: CacheStorage | undefined,
  now: number,
): Commit[] | undefined {
  if (!storage) return undefined;
  try {
    const raw = storage.getItem(CACHE_KEY);
    if (!raw) return undefined;
    const entry = JSON.parse(raw) as CacheEntry;
    if (typeof entry?.at !== "number" || now - entry.at > CACHE_TTL_MS) {
      return undefined;
    }
    const commits = reviveCommits(entry.commits);
    return commits.length > 0 ? commits : undefined;
  } catch {
    return undefined;
  }
}

/** Stores the list. A storage that refuses to write is not an error here. */
export function writeCachedCommits(
  storage: CacheStorage | undefined,
  commits: Commit[],
  now: number,
): void {
  if (!storage || commits.length === 0) return;
  try {
    storage.setItem(CACHE_KEY, JSON.stringify({ at: now, commits } as CacheEntry));
  } catch {
    // Private-mode Safari throws on write, and a full quota throws anywhere.
    // The feed works without a cache; it just costs a request per page.
  }
}

/** sessionStorage, or undefined where it is absent or blocked. */
export function sessionCache(): CacheStorage | undefined {
  try {
    return globalThis.sessionStorage ?? undefined;
  } catch {
    return undefined;
  }
}
