import { describe, expect, it } from "vitest";
import {
  hasTriggeringClickBurst,
  LOGO_CLICK_THRESHOLD,
  LOGO_CLICK_WINDOW_MS,
} from "@/lib/logo-easter-egg";

// Timestamps are built from the constants so these tests follow the tuned
// threshold and window rather than restating them -- the konami tests keep
// the same discipline with the sequence itself.

/** `count` clicks ending at `now`, evenly spread across `spreadMs`. */
function burst(count: number, now: number, spreadMs: number): number[] {
  return Array.from({ length: count }, (_, index) =>
    Math.round(now - spreadMs + (spreadMs * index) / (count - 1)),
  );
}

describe("hasTriggeringClickBurst", () => {
  it("rejects an empty run", () => {
    expect(hasTriggeringClickBurst([], 1_000)).toBe(false);
  });

  it("accepts a threshold-sized burst well inside the window", () => {
    const now = 10_000;
    const clicks = burst(LOGO_CLICK_THRESHOLD, now, 600);
    expect(hasTriggeringClickBurst(clicks, now)).toBe(true);
  });

  it("rejects a burst one click short of the threshold", () => {
    const now = 10_000;
    const clicks = burst(LOGO_CLICK_THRESHOLD - 1, now, 600);
    expect(hasTriggeringClickBurst(clicks, now)).toBe(false);
  });

  it("rejects enough clicks when they are spread beyond the window", () => {
    // A visitor clicking the logo to navigate, once every couple of seconds:
    // the run has plenty of clicks, but never enough of them recently.
    const now = 60_000;
    const clicks = burst(LOGO_CLICK_THRESHOLD, now, LOGO_CLICK_WINDOW_MS * 5);
    expect(hasTriggeringClickBurst(clicks, now)).toBe(false);
  });

  it("counts a click sitting exactly on the window boundary -- inclusive by design", () => {
    // The rule pinned here: `now - t <= LOGO_CLICK_WINDOW_MS` counts, so a
    // burst whose oldest click is exactly LOGO_CLICK_WINDOW_MS old fires.
    const now = 10_000;
    const onBoundary = burst(LOGO_CLICK_THRESHOLD, now, LOGO_CLICK_WINDOW_MS);
    expect(onBoundary[0]).toBe(now - LOGO_CLICK_WINDOW_MS);
    expect(hasTriggeringClickBurst(onBoundary, now)).toBe(true);
  });

  it("rejects a burst whose oldest click is one millisecond past the window", () => {
    const now = 10_000;
    const clicks = burst(LOGO_CLICK_THRESHOLD, now, LOGO_CLICK_WINDOW_MS + 1);
    expect(clicks[0]).toBe(now - LOGO_CLICK_WINDOW_MS - 1);
    expect(hasTriggeringClickBurst(clicks, now)).toBe(false);
  });

  it("ignores stale clicks before a fresh triggering burst", () => {
    // Old navigation clicks linger in the run; only the recent burst decides.
    const now = 120_000;
    const stale = [1_000, 30_000, 55_000];
    const fresh = burst(LOGO_CLICK_THRESHOLD, now, 500);
    expect(hasTriggeringClickBurst([...stale, ...fresh], now)).toBe(true);
  });
});
