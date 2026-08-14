import { describe, expect, it } from "vitest";
import { extractHeadings } from "@/lib/markdown-toc";

describe("extractHeadings", () => {
  it("collects h2 and h3 by default, ignoring h1", () => {
    const entries = extractHeadings(
      ["# Title", "## First", "### Nested", "#### Too deep"].join("\n"),
    );
    expect(entries).toEqual([
      { depth: 2, text: "First", slug: "first" },
      { depth: 3, text: "Nested", slug: "nested" },
    ]);
  });

  it("slugs the way rehype-slug does, so the anchors resolve", () => {
    const entries = extractHeadings("## 7. Linear Types for Resource Management");
    expect(entries[0].slug).toBe("7-linear-types-for-resource-management");
  });

  it("keeps duplicate headings distinct with the same suffixes rehype-slug uses", () => {
    const entries = extractHeadings(["## Shipped", "## Shipped", "## Shipped"].join("\n"));
    expect(entries.map((entry) => entry.slug)).toEqual([
      "shipped",
      "shipped-1",
      "shipped-2",
    ]);
  });

  it("counts headings outside the range so the duplicate suffixes stay in step", () => {
    // The h1 is not returned, but it still consumes the "intro" slug — which
    // is what rehype-slug does when it walks the rendered document.
    const entries = extractHeadings(["# Intro", "## Intro"].join("\n"));
    expect(entries).toEqual([{ depth: 2, text: "Intro", slug: "intro-1" }]);
  });

  it("ignores a hash inside a fenced code block", () => {
    const markdown = [
      "## Real heading",
      "```cinnabar",
      "# linear handle consumed exactly once",
      "## not a heading either",
      "```",
      "## Second heading",
    ].join("\n");
    expect(extractHeadings(markdown).map((entry) => entry.text)).toEqual([
      "Real heading",
      "Second heading",
    ]);
  });

  it("handles tilde fences and does not close on a different fence character", () => {
    const markdown = ["~~~", "## hidden", "~~~", "## visible"].join("\n");
    expect(extractHeadings(markdown).map((entry) => entry.text)).toEqual(["visible"]);
  });

  it("strips inline code, emphasis and links from the label", () => {
    const entries = extractHeadings(
      "## Milestone 5 — `build.cnb` Project **Manifest** and [a link](x.md)",
    );
    expect(entries[0].text).toBe("Milestone 5 — build.cnb Project Manifest and a link");
  });

  it("honours an explicit depth range", () => {
    const entries = extractHeadings(["## Two", "### Three", "#### Four"].join("\n"), {
      minDepth: 3,
      maxDepth: 4,
    });
    expect(entries.map((entry) => entry.depth)).toEqual([3, 4]);
  });

  it("returns nothing for a document with no headings", () => {
    expect(extractHeadings("Just prose.\n\nMore prose.")).toEqual([]);
  });
});
