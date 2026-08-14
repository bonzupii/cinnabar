import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isAsciiDiagram, parseFlow } from "@/lib/ascii-diagram";

/*
 * The renderer decides from the characters of a fenced block, never from its
 * text, so that an edit to ARCHITECTURE.md changes what the figure says rather
 * than whether it appears. These cover both directions of that: what counts as
 * a diagram, and what the fallbacks are when one cannot be redrawn.
 */

const REPO = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
  "..",
);

const FLOW = `module_loader::load
      │
      ▼
resolver::resolve
      │
      ▼
codegen::compile_and_link`;

describe("isAsciiDiagram", () => {
  it("recognises a block drawn with box characters", () => {
    expect(isAsciiDiagram(FLOW)).toBe(true);
  });

  it("recognises a boxed figure it cannot redraw", () => {
    expect(isAsciiDiagram("┌────┐\n│ hi │\n└────┘")).toBe(true);
  });

  it("leaves a shell transcript alone", () => {
    expect(isAsciiDiagram("$ cinnabar build\nSuccessfully compiled.")).toBe(false);
  });

  it("leaves source alone, including source full of punctuation", () => {
    const source = `pub const NAME: &[U8] = "cinnabar"
fun f(a: I64) I64
  return a - 1 | 0
end`;
    expect(isAsciiDiagram(source)).toBe(false);
  });
});

describe("parseFlow", () => {
  it("lifts the labels of a single-column flow, in order", () => {
    expect(parseFlow(FLOW)).toEqual([
      "module_loader::load",
      "resolver::resolve",
      "codegen::compile_and_link",
    ]);
  });

  it("reads an ASCII flow drawn without the Unicode set", () => {
    expect(parseFlow("lex\n |\n v\nparse")).toEqual(["lex", "parse"]);
  });

  it("declines a figure whose labels sit inside drawn boxes", () => {
    expect(parseFlow("┌──────┐\n│ lex  │\n└──────┘")).toBeNull();
  });

  it("declines two labels with no connector between them", () => {
    expect(parseFlow("lex\nparse")).toBeNull();
  });

  it("declines a figure with only one label", () => {
    expect(parseFlow("lex\n │\n ▼")).toBeNull();
  });
});

describe("ARCHITECTURE.md, as it stands today", () => {
  /*
   * A canary, not a specification. If the document's figure is rewritten into
   * something this cannot redraw, the renderer falls back to presenting the
   * art as written and the page is still correct — but the maintainer should
   * know the drawn form was lost, which is what this reports.
   */
  const document = readFileSync(path.join(REPO, "ARCHITECTURE.md"), "utf8");
  const blocks = [...document.matchAll(/```[\w-]*\n([\s\S]*?)```/g)].map(
    (match) => match[1],
  );

  it("still contains a block the renderer treats as a figure", () => {
    expect(blocks.some((block) => isAsciiDiagram(block))).toBe(true);
  });

  it("still contains a figure that can be redrawn as a flow", () => {
    const drawn = blocks.filter(isAsciiDiagram).map(parseFlow);
    expect(drawn.some((nodes) => nodes !== null && nodes.length >= 2)).toBe(true);
  });
});
