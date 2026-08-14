import { describe, expect, it, vi } from "vitest";
import {
  ACTIVITY_COUNT,
  CACHE_KEY,
  CACHE_TTL_MS,
  COMMITS_ENDPOINT,
  fetchCommits,
  parseCommits,
  readCachedCommits,
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
  it("reduces a commit to the four fields the feed renders", () => {
    expect(parseCommits([payload()])).toEqual([
      {
        sha: "945928227fee394929097e547c0e66f2b7558e1e",
        abbrev: "9459282",
        subject: "feat: unify native slice views under a signature-classified opcode",
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

  it("never returns more than the feed has room for", () => {
    const many = Array.from({ length: 30 }, (_, index) =>
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
