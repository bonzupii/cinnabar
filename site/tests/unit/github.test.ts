import { describe, expect, it, vi } from "vitest";
import {
  ACTIVITY_COUNT,
  CACHE_KEY,
  CACHE_TTL_MS,
  COMMITS_ENDPOINT,
  activeDays,
  activityAreas,
  availableWindows,
  cadence,
  describeSubject,
  mergeCommits,
  fetchCommits,
  groupByDay,
  parseCommits,
  readCachedCommits,
  withinWindow,
  writeCachedCommits,
  type CacheStorage,
  type Commit,
} from "@/lib/github";

/*
 * The commit feed's fetch, parse and cache logic, with no network.
 *
 * The feed is an enhancement over a prerendered list, so almost every case
 * here is a failure case: what matters is not that a good response renders but
 * that a bad one — rate limited, offline, truncated, hostile — comes back as
 * "no commits", which the component reads as "keep what the build gave you".
 */

/** A commit as the endpoint actually returns it, trimmed to the read fields. */
function payload(overrides: Record<string, unknown> = {}) {
  return {
    sha: "945928227fee394929097e547c0e66f2b7558e1e",
    html_url: "https://github.com/bonzupii/cinnabar/commit/9459282",
    commit: {
      message: "feat: unify native slice views under a signature-classified opcode",
      author: { name: "bonzupii", date: "2026-08-14T02:22:43Z" },
      committer: { name: "GitHub", date: "2026-08-14T02:22:43Z" },
    },
    ...overrides,
  };
}

/** An in-memory sessionStorage. */
function storage(initial?: string): CacheStorage & { value?: string } {
  return {
    value: initial,
    getItem(key) {
      return key === CACHE_KEY ? (this.value ?? null) : null;
    },
    setItem(key, value) {
      if (key === CACHE_KEY) this.value = value;
    },
  };
}

describe("the endpoint", () => {
  it("asks for exactly the branch and the number of commits the feed shows", () => {
    expect(COMMITS_ENDPOINT).toBe(
      "https://api.github.com/repos/bonzupii/cinnabar/commits" +
        `?sha=main&per_page=${ACTIVITY_COUNT}`,
    );
  });
});

describe("parsing a response", () => {
  it("reduces a commit to the fields the feed renders", () => {
    expect(parseCommits([payload()])).toEqual([
      {
        sha: "945928227fee394929097e547c0e66f2b7558e1e",
        abbrev: "9459282",
        subject: "feat: unify native slice views under a signature-classified opcode",
        areas: ["feat"],
        title: "unify native slice views under a signature-classified opcode",
        date: "2026-08-14",
        url:
          "https://github.com/bonzupii/cinnabar/commit/" +
          "945928227fee394929097e547c0e66f2b7558e1e",
      } satisfies Commit,
    ]);
  });

  it("keeps the subject and drops the message body", () => {
    const message = "fix: reject ambiguous returned borrows\n\nThe checker was\nlenient.";
    expect(parseCommits([payload({ commit: { ...payload().commit, message } })])[0]
      .subject).toBe("fix: reject ambiguous returned borrows");
  });

  it("builds the permalink from the sha rather than trusting html_url", () => {
    // The payload is third-party data being turned into an href on our page.
    const [commit] = parseCommits([
      payload({ html_url: "https://example.invalid/phish" }),
    ]);
    expect(commit.url).toBe(
      "https://github.com/bonzupii/cinnabar/commit/" +
        "945928227fee394929097e547c0e66f2b7558e1e",
    );
  });

  it("falls back to the committer date when there is no author date", () => {
    const commit = payload().commit;
    const [parsed] = parseCommits([
      payload({ commit: { ...commit, author: undefined } }),
    ]);
    expect(parsed.date).toBe("2026-08-14");
  });

  it("never returns more than the window, whatever the endpoint sends", () => {
    // `per_page` asks for the window; nothing obliges the answer to honour it.
    const many = Array.from({ length: ACTIVITY_COUNT * 2 }, (_, index) =>
      payload({ sha: index.toString(16).padStart(40, "0") }),
    );
    expect(parseCommits(many)).toHaveLength(ACTIVITY_COUNT);
  });

  it.each([
    ["a rate-limit body", { message: "API rate limit exceeded for 1.2.3.4." }],
    ["a 404 body", { message: "Not Found" }],
    ["an empty repository", {}],
    ["null", null],
    ["a string", "<html>blocked by your network</html>"],
    ["a number", 42],
  ])("reads %s as no commits rather than throwing", (_label, body) => {
    expect(parseCommits(body)).toEqual([]);
  });

  it.each([
    ["no sha", { sha: undefined }],
    ["a sha that is not a sha", { sha: "../../etc/passwd" }],
    ["a short sha", { sha: "abc" }],
    ["no commit object", { commit: undefined }],
    ["no message", { commit: { author: { date: "2026-08-14T02:22:43Z" } } }],
    ["an empty subject", { commit: { message: "\n\nbody", author: { date: "2026-08-14T02:22:43Z" } } }],
    ["no date", { commit: { message: "feat: x" } }],
    ["an unparseable date", { commit: { message: "feat: x", author: { date: "soon" } } }],
  ])("drops an entry with %s", (_label, overrides) => {
    expect(parseCommits([payload(overrides)])).toEqual([]);
  });

  it("keeps the good entries in a partly malformed list", () => {
    const commits = parseCommits([payload({ sha: "nope" }), payload()]);
    expect(commits.map((commit) => commit.abbrev)).toEqual(["9459282"]);
  });
});

describe("reading the area out of a subject", () => {
  /*
   * Two conventions live in this log. `AGENTS.md` mandates `area: Subject` and
   * that is what recent commits use; the earlier ones — and everything
   * dependabot still opens — are conventional-commits. Both have to land on
   * the same area, or the filter offers `codegen` and `fix(codegen)` as two
   * different subsystems.
   */
  it.each([
    ["vscode: Correct the launch default", ["vscode"]],
    ["resolver, codegen: Implement a sealed native registry", ["resolver", "codegen"]],
    ["fix(codegen): struct keys through HashMap", ["codegen"]],
    ["build(deps): bump inkwell to 0.10.0", ["deps"]],
    ["style(parser): re-wrap the token table", ["parser"]],
    ["tree-sitter: Regenerate the grammar", ["tree-sitter"]],
    // No parenthesised scope to prefer, so the type itself is the area. It is
    // what the commit said about where it landed, which is nothing.
    ["feat: make the backend cross-platform", ["feat"]],
    ["fix(lsp), lsp: Fold the duplicate", ["lsp"]],
  ])("reads %s", (subject, areas) => {
    expect(describeSubject(subject).areas).toEqual(areas);
  });

  it("strips the prefix from the title but keeps the whole subject", () => {
    const { title } = describeSubject("docs: Record WSL2 as the default");
    expect(title).toBe("Record WSL2 as the default");
    expect(parseCommits([payload({ commit: { ...payload().commit,
      message: "docs: Record WSL2 as the default" } })])[0].subject)
      .toBe("docs: Record WSL2 as the default");
  });

  it.each([
    // A merge commit, which has no prefix at all.
    "Merge pull request #9 from bonzupii/dependabot/cargo/inkwell-0.10.0",
    "Address OpenSSF Scorecard findings",
    // A real subject from this log: the colon is punctuation in a sentence,
    // and the 32-character cap is what tells the two apart.
    "amendment to commit message style rule: no AI attribution.",
    // Prose that happens to be short enough, but is not an area token.
    "See also: the note in ARCHITECTURE.md",
    'Revert "docs: record the collision"',
  ])("leaves %s whole", (subject) => {
    expect(describeSubject(subject)).toEqual({ areas: [], title: subject });
  });

  it("refuses a prefix listing more areas than a prefix plausibly lists", () => {
    const subject = "a, b, c, d: something";
    expect(describeSubject(subject).areas).toEqual([]);
  });
});

describe("summarising a window", () => {
  /** A commit, from just the parts these functions read. */
  function commit(sha: string, subject: string, date: string): Commit {
    const [parsed] = parseCommits([
      { sha: sha.padStart(40, "0"), commit: { message: subject, author: { date } } },
    ]);
    return parsed;
  }

  const window = [
    commit("1", "tools: Emit JSON documents", "2026-08-16T10:00:00Z"),
    commit("2", "docs: Record a gap", "2026-08-16T09:00:00Z"),
    commit("3", "resolver, codegen: Seal the registry", "2026-08-14T09:00:00Z"),
    commit("4", "Merge pull request #18", "2026-08-14T08:00:00Z"),
    commit("5", "docs: Record WSL2", "2026-08-10T08:00:00Z"),
  ];

  it("counts the areas, busiest first and ties broken by name", () => {
    expect(activityAreas(window)).toEqual([
      { area: "docs", count: 2 },
      { area: "codegen", count: 1 },
      { area: "resolver", count: 1 },
      { area: "tools", count: 1 },
    ]);
  });

  it("counts a commit once for each area it names", () => {
    // The filter answers "what touched the resolver", and a commit that
    // touched the resolver and the backend is an answer to it.
    const areas = activityAreas([window[2]]).map((entry) => entry.area);
    expect(areas).toEqual(["codegen", "resolver"]);
  });

  it("counts days the repository moved, not commits", () => {
    expect(activeDays(window)).toBe(3);
  });

  it("groups by day in the order the commits arrived", () => {
    expect(groupByDay(window).map((day) => [day.date, day.commits.length])).toEqual([
      ["2026-08-16", 2],
      ["2026-08-14", 2],
      ["2026-08-10", 1],
    ]);
  });

  describe("the cadence chart", () => {
    it("draws one column per day across the span the commits cover", () => {
      const series = cadence(window);
      // 2026-08-10 to 2026-08-16 inclusive.
      expect(series).toHaveLength(7);
      expect(series[0]).toMatchObject({ date: "2026-08-10", days: 1 });
      expect(series[6]).toMatchObject({ date: "2026-08-16", days: 1 });
    });

    it("keeps the quiet days, which are the point of the chart", () => {
      expect(cadence(window).map((day) => day.count)).toEqual([1, 0, 0, 0, 2, 0, 2]);
    });

    it("ends on the newest date even when the list is not sorted", () => {
      // The endpoint returns newest first and this does not re-sort, so the
      // anchor is found rather than assumed.
      const shuffled = [window[4], window[0], window[2]];
      expect(cadence(shuffled).at(-1)?.date).toBe("2026-08-16");
    });

    it("buckets by week once a column per day stops being legible", () => {
      const long = [
        commit("a", "docs: old", "2026-01-01T00:00:00Z"),
        commit("b", "docs: new", "2026-08-16T00:00:00Z"),
      ];
      const series = cadence(long);
      expect(series.every((column) => column.days === 7)).toBe(true);
      // The last column still ends on the newest commit, so the weeks are
      // counted back from the data rather than from an arbitrary Monday.
      expect(series.at(-1)?.date).toBe("2026-08-10");
      expect(series.at(-1)?.count).toBe(1);
      expect(series.reduce((total, column) => total + column.count, 0)).toBe(2);
    });

    it("is empty rather than anchored to today when there is nothing to draw", () => {
      // A chart anchored to `Date.now()` would be one string on the server and
      // another in the browser — the hydration mismatch the absolute dates
      // exist to avoid.
      expect(cadence([])).toEqual([]);
    });
  });

  describe("windows", () => {
    it("offers only the windows shorter than the history in hand", () => {
      // Seven days of commits: a 30-day window would select the same set the
      // "all" button already selects.
      expect(availableWindows(window)).toEqual([null]);
    });

    it("offers more as the history grows past each one", () => {
      const long = [
        commit("a", "docs: old", "2025-01-01T00:00:00Z"),
        commit("b", "docs: new", "2026-08-16T00:00:00Z"),
      ];
      expect(availableWindows(long)).toEqual([7, 30, 90, 365, null]);
    });

    it("measures the window back from the newest commit, not from today", () => {
      // Anchored to today, "the last 7 days" of a repository quiet for a month
      // would be empty rather than its most recent week of work.
      expect(withinWindow(window, 3).map((c) => c.date)).toEqual([
        "2026-08-16",
        "2026-08-16",
        "2026-08-14",
        "2026-08-14",
      ]);
    });

    it("takes everything for the null window", () => {
      expect(withinWindow(window, null)).toHaveLength(window.length);
    });

    it("has nothing to window when there are no commits", () => {
      expect(availableWindows([])).toEqual([null]);
      expect(withinWindow([], 7)).toEqual([]);
    });
  });

  describe("merging the build's history with the browser's refresh", () => {
    it("keeps one entry per sha across both sources", () => {
      const merged = mergeCommits(window, [window[0], window[1]]);
      expect(merged).toHaveLength(window.length);
    });

    it("carries commits the other source does not have, newest first", () => {
      // The history reaches back further; the refresh reaches forward to what
      // landed after the deploy.
      const older = commit("9", "docs: older", "2026-08-01T09:00:00Z");
      const newer = commit("8", "docs: newer", "2026-08-20T09:00:00Z");
      const merged = mergeCommits([...window, older], [newer, window[0]]);
      expect(merged[0].sha).toBe(newer.sha);
      expect(merged.at(-1)?.sha).toBe(older.sha);
      expect(merged).toHaveLength(window.length + 2);
    });
  });
});

describe("fetching", () => {
  function respond(body: unknown, status = 200) {
    return vi.fn(async () =>
      new Response(JSON.stringify(body), {
        status,
        headers: { "Content-Type": "application/json" },
      }),
    ) as unknown as typeof fetch;
  }

  it("returns the parsed commits on a good response", async () => {
    const commits = await fetchCommits({ fetchImpl: respond([payload()]) });
    expect(commits.map((commit) => commit.abbrev)).toEqual(["9459282"]);
  });

  it("sends only CORS-safelisted headers, so the browser does not preflight", async () => {
    const fetchImpl = respond([payload()]);
    await fetchCommits({ fetchImpl });
    const [, init] = (fetchImpl as unknown as { mock: { calls: [string, RequestInit][] } })
      .mock.calls[0];
    expect(init.headers).toEqual({ Accept: "application/vnd.github+json" });
  });

  it("returns nothing when the reader is rate limited", async () => {
    const rateLimited = respond({ message: "API rate limit exceeded" }, 403);
    expect(await fetchCommits({ fetchImpl: rateLimited })).toEqual([]);
  });

  it("returns nothing on a 5xx", async () => {
    expect(await fetchCommits({ fetchImpl: respond({}, 502) })).toEqual([]);
  });

  it("returns nothing rather than rejecting when the request fails outright", async () => {
    const offline = vi.fn(async () => {
      throw new TypeError("Failed to fetch");
    }) as unknown as typeof fetch;
    await expect(fetchCommits({ fetchImpl: offline })).resolves.toEqual([]);
  });

  it("returns nothing when the body is not JSON", async () => {
    const html = vi.fn(async () => new Response("<html>captive portal</html>")) as
      unknown as typeof fetch;
    expect(await fetchCommits({ fetchImpl: html })).toEqual([]);
  });

  it("returns nothing when the request is aborted", async () => {
    const controller = new AbortController();
    controller.abort();
    const aborting = vi.fn(async (_url: unknown, init?: RequestInit) => {
      init?.signal?.throwIfAborted();
      return new Response("[]");
    }) as unknown as typeof fetch;
    expect(
      await fetchCommits({ fetchImpl: aborting, signal: controller.signal }),
    ).toEqual([]);
  });

  it("asks Next to keep the build's copy only when the build asks it to", async () => {
    const fetchImpl = respond([payload()]);
    await fetchCommits({ fetchImpl });
    await fetchCommits({ fetchImpl, revalidateSeconds: 900 });
    const calls = (fetchImpl as unknown as { mock: { calls: [string, RequestInit][] } })
      .mock.calls;
    expect("next" in calls[0][1]).toBe(false);
    expect(calls[1][1]).toMatchObject({ next: { revalidate: 900 } });
  });
});

describe("the session cache", () => {
  const commits = parseCommits([payload()]);
  const now = 1_770_000_000_000;

  it("returns what was written, within the TTL", () => {
    const store = storage();
    writeCachedCommits(store, commits, now);
    expect(readCachedCommits(store, now + CACHE_TTL_MS - 1)).toEqual(commits);
  });

  it("expires", () => {
    const store = storage();
    writeCachedCommits(store, commits, now);
    expect(readCachedCommits(store, now + CACHE_TTL_MS + 1)).toBeUndefined();
  });

  it("is a miss when there is nothing stored", () => {
    expect(readCachedCommits(storage(), now)).toBeUndefined();
  });

  it("is a miss when the entry is not JSON", () => {
    expect(readCachedCommits(storage("not json"), now)).toBeUndefined();
  });

  it("is a miss when the entry has no timestamp", () => {
    const stored = JSON.stringify({ commits });
    expect(readCachedCommits(storage(stored), now)).toBeUndefined();
  });

  it("re-validates the stored commits rather than trusting them", () => {
    // sessionStorage is writable by anything running on this origin, so an
    // entry is untrusted input like any other.
    const stored = JSON.stringify({
      at: now,
      commits: [{ sha: "x", subject: "<script>", date: "2026-08-14" }],
    });
    expect(readCachedCommits(storage(stored), now)).toBeUndefined();
  });

  it("rebuilds the link from the stored sha rather than reading it", () => {
    const stored = JSON.stringify({
      at: now,
      commits: [{ ...commits[0], url: "javascript:alert(1)", abbrev: "hacked" }],
    });
    const [revived] = readCachedCommits(storage(stored), now)!;
    expect(revived.url).toBe(commits[0].url);
    expect(revived.abbrev).toBe("9459282");
  });

  it("does nothing at all without a storage", () => {
    expect(readCachedCommits(undefined, now)).toBeUndefined();
    expect(() => writeCachedCommits(undefined, commits, now)).not.toThrow();
  });

  it("does not store an empty list, which would cache a failure", () => {
    const store = storage();
    writeCachedCommits(store, [], now);
    expect(store.value).toBeUndefined();
  });

  it("survives a storage that refuses to write", () => {
    const readOnly: CacheStorage = {
      getItem: () => null,
      setItem: () => {
        // Safari in private mode, and any browser at quota.
        throw new DOMException("QuotaExceededError");
      },
    };
    expect(() => writeCachedCommits(readOnly, commits, now)).not.toThrow();
  });

  it("survives a storage that refuses to be read", () => {
    const blocked: CacheStorage = {
      getItem: () => {
        throw new DOMException("SecurityError");
      },
      setItem: () => {},
    };
    expect(readCachedCommits(blocked, now)).toBeUndefined();
  });
});
