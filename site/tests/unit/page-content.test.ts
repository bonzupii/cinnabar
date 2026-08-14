import { describe, expect, it } from "vitest";
import {
  parseContentBlocks,
  parseContentItems,
  parseContentList,
} from "@/lib/page-content";

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

describe("parseContentItems", () => {
  it("splits a block into its `###` sections, slugging each heading", () => {
    const parsed = parseContentItems(
      [
        "### Linear resource management",
        "",
        "Consumed on every path.",
        "",
        "### O(1) call-stack recursion",
        "",
        "Strict tail position.",
      ].join("\n"),
    );
    expect(parsed).toEqual([
      {
        slug: "linear-resource-management",
        title: "Linear resource management",
        body: "Consumed on every path.",
      },
      {
        slug: "o1-call-stack-recursion",
        title: "O(1) call-stack recursion",
        body: "Strict tail position.",
      },
    ]);
  });

  it("drops a preamble before the first heading", () => {
    const parsed = parseContentItems("<!-- a note -->\n\n### One\n\nBody.");
    expect(parsed).toEqual([{ slug: "one", title: "One", body: "Body." }]);
  });

  it("keeps a heading that is a flag or a command verbatim", () => {
    // The reference tables are written this way: the heading is the CLI token.
    const parsed = parseContentItems("### -o, --output <PATH>\n\nOutput binary path");
    expect(parsed[0].title).toBe("-o, --output <PATH>");
    expect(parsed[0].body).toBe("Output binary path");
  });

  it("keeps multi-paragraph bodies", () => {
    const parsed = parseContentItems("### One\n\nFirst.\n\nSecond.");
    expect(parsed[0].body).toBe("First.\n\nSecond.");
  });

  it("returns nothing for a block with no headings", () => {
    expect(parseContentItems("Just prose.")).toEqual([]);
  });
});

describe("parseContentList", () => {
  it("returns the top-level bullets without their markers", () => {
    expect(parseContentList("- One\n- Two\n* Three")).toEqual([
      "One",
      "Two",
      "Three",
    ]);
  });

  it("ignores prose around the list", () => {
    expect(parseContentList("Intro.\n\n- One\n\nOutro.")).toEqual(["One"]);
  });
});
