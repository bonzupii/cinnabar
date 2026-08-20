import { execFileSync } from "node:child_process";
import { describe, expect, it } from "vitest";
import { isShallow, readGitLog } from "@/lib/git-log";
import { HISTORY_LIMIT } from "@/lib/github";

/*
 * The build-time history, read against this repository itself.
 *
 * There is deliberately no fixture: the thing worth checking is that the
 * format string, the separators and the timestamp conversion agree with the
 * git that will actually run at build time, and a stubbed `git log` would
 * check none of that. What is asserted is therefore about shape and
 * invariants rather than about particular commits, so the file does not need
 * editing every time someone pushes.
 */

const log = readGitLog();

describe("reading the repository's log", () => {
  it("returns commits", () => {
    // If this fails, either git is absent or the format string stopped
    // parsing — and both make the section's windows collapse to the API's
    // thirty commits without anything else failing.
    expect(log.length).toBeGreaterThan(0);
  });

  it("agrees with git about how many commits there are", () => {
    const counted = Number(
      execFileSync("git", ["rev-list", "--count", "HEAD"], { encoding: "utf8" }).trim(),
    );
    expect(log).toHaveLength(Math.min(counted, HISTORY_LIMIT));
  });

  it("honours a lower limit", () => {
    expect(readGitLog(5)).toHaveLength(5);
  });

  it("gives every commit a full sha, a UTC date and a subject", () => {
    for (const commit of log) {
      expect(commit.sha).toMatch(/^[0-9a-f]{40}$/);
      expect(commit.date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
      expect(commit.subject.length).toBeGreaterThan(0);
      expect(commit.url).toBe(`https://github.com/bonzupii/cinnabar/commit/${commit.sha}`);
    }
  });

  it("dates a commit by its UTC instant, matching what the API reports", () => {
    // `--date=short` would give git's idea of the author's local day, and the
    // two disagree for anyone committing near midnight outside UTC.
    const [newest] = log;
    const seconds = Number(
      execFileSync("git", ["log", "-1", "--format=%at", newest.sha], {
        encoding: "utf8",
      }).trim(),
    );
    expect(newest.date).toBe(new Date(seconds * 1000).toISOString().slice(0, 10));
  });

  it("returns newest first", () => {
    const dates = log.map((commit) => commit.date);
    expect([...dates].sort().reverse()).toEqual(dates);
  });

  it("reads the areas out of the subjects, as the API path does", () => {
    // The whole point of the history is that it feeds the same filter.
    expect(log.some((commit) => commit.areas.length > 0)).toBe(true);
  });

  it("knows whether the checkout is shallow", () => {
    expect(typeof isShallow()).toBe("boolean");
  });
});
