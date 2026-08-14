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

/** How many commits the feed shows. Also the height the slot reserves. */
export const ACTIVITY_COUNT = 5;

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

    commits.push({
      sha,
      abbrev: sha.slice(0, 7),
      subject,
      date: parsed.toISOString().slice(0, 10),
      url: commitUrl(sha),
    });
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
 * is untrusted input like any other. `abbrev` and `url` are recomputed from
 * the sha rather than read, so neither can be poisoned by a stored value.
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

    commits.push({ sha, abbrev: sha.slice(0, 7), subject, date, url: commitUrl(sha) });
  }
  return commits.slice(0, ACTIVITY_COUNT);
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
