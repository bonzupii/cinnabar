import { describe, expect, it } from "vitest";
import { isClean, locateSpan, type PlaygroundReport } from "@/lib/cinnabar-diagnostics";

// "line one\nline two\nline three"
//  0........8 9.......17 18......28
const SOURCE = "line one\nline two\nline three";

describe("locateSpan", () => {
  it("locates a span on the first line", () => {
    expect(locateSpan(SOURCE, 0, 4)).toEqual({
      line: 1,
      column: 1,
      lineText: "line one",
      columnOffset: 0,
      length: 4,
    });
  });

  it("locates a span partway through a later line", () => {
    // "two" within "line two", which starts at byte 9.
    expect(locateSpan(SOURCE, 14, 17)).toEqual({
      line: 2,
      column: 6,
      lineText: "line two",
      columnOffset: 5,
      length: 3,
    });
  });

  it("clips a span that crosses a line break at the line's end", () => {
    // Starts at "two"'s 't' (byte 14) and asks for 6 bytes, which would run
    // into "line three" -- the span must stop at line 2's own end.
    expect(locateSpan(SOURCE, 14, 20)).toEqual({
      line: 2,
      column: 6,
      lineText: "line two",
      columnOffset: 5,
      length: 3,
    });
  });

  it("handles a zero-width span", () => {
    expect(locateSpan(SOURCE, 9, 9)).toMatchObject({
      line: 2,
      column: 1,
      length: 0,
    });
  });
});

describe("isClean", () => {
  const base: PlaygroundReport = { format: "cinnabar.playground-diagnostics.v1", diagnostics: [] };

  it("is clean with no diagnostics and no serialization error", () => {
    expect(isClean(base)).toBe(true);
  });

  it("is not clean once a diagnostic is present", () => {
    expect(
      isClean({
        ...base,
        diagnostics: [{ severity: "error", message: "boom", source: null, explanations: [] }],
      }),
    ).toBe(false);
  });

  it("is not clean on a serialization error even with an empty diagnostics list", () => {
    expect(isClean({ ...base, serialization_error: "malformed" })).toBe(false);
  });
});
