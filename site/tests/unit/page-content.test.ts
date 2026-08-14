import { describe, expect, it } from "vitest";
import { parseContentBlocks } from "@/lib/page-content";

describe("parseContentBlocks", () => {
  it("treats a document with no markers as a single body block", () => {
    expect(parseContentBlocks("Just some prose.\n\nAnd more.")).toEqual({
      body: "Just some prose.\n\nAnd more.",
    });
  });

  it("splits on named markers", () => {
    const parsed = parseContentBlocks(
      ["<!-- @lede -->", "The lede.", "", "<!-- @closing -->", "The end."].join("\n"),
    );
    expect(parsed).toEqual({ lede: "The lede.", closing: "The end." });
  });

  it("keeps text before the first marker as the body", () => {
    const parsed = parseContentBlocks(
      ["Intro text.", "", "<!-- @extra -->", "Extra text."].join("\n"),
    );
    expect(parsed).toEqual({ body: "Intro text.", extra: "Extra text." });
  });

  it("preserves markdown inside a block, including blank lines and fences", () => {
    const parsed = parseContentBlocks(
      [
        "<!-- @sample -->",
        "Some prose.",
        "",
        "```cinnabar",
        "pub fun main() I64",
        "  return 0",
        "end",
        "```",
      ].join("\n"),
    );
    expect(parsed.sample).toBe(
      "Some prose.\n\n```cinnabar\npub fun main() I64\n  return 0\nend\n```",
    );
  });

  it("ignores an ordinary HTML comment", () => {
    const parsed = parseContentBlocks("<!-- a note -->\nBody text.");
    expect(parsed).toEqual({ body: "<!-- a note -->\nBody text." });
  });

  it("tolerates indentation and spacing around a marker", () => {
    const parsed = parseContentBlocks("  <!--   @spaced   -->  \nText.");
    expect(parsed).toEqual({ spaced: "Text." });
  });

  it("drops an empty block rather than storing an empty string", () => {
    const parsed = parseContentBlocks("<!-- @empty -->\n\n<!-- @filled -->\nHere.");
    expect(parsed).toEqual({ filled: "Here." });
  });

  it("handles CRLF line endings", () => {
    const parsed = parseContentBlocks("<!-- @a -->\r\nOne.\r\n<!-- @b -->\r\nTwo.");
    expect(parsed).toEqual({ a: "One.", b: "Two." });
  });
});
