import { expect, test } from "@playwright/test";
import { preparePage } from "./prepare";

/*
 * The borrow diagnostic's rails.
 *
 * These broke three times before they were drawn rather than typed, and each
 * fix addressed a cause that turned out not to be the cause. The actual one:
 * IBM Plex Mono has no box-drawing glyphs, so `│ ─ ┬` and `╭ ╰ ╯` are served
 * by two different fallback faces at two different advances, neither of them
 * the mono cell. The rail could not join and the columns could not line up, at
 * any line height.
 *
 * So the component lays each box character out as an empty one-`ch` cell and
 * paints the stroke with a border. These tests pin the two properties that
 * makes true, either of which a regression would break.
 */

/** The whole Box Drawing block. */
const BOX_DRAWING = /[─-╿]/;

/** Every drawn stroke, with its box and its line's box in page coordinates. */
async function strokes(page: import("@playwright/test").Page) {
  return page.evaluate(() => {
    const lines = [...document.querySelectorAll("[data-diagnostic-line]")];
    return lines.flatMap((line) => {
      const lineBox = line.getBoundingClientRect();
      return [...line.querySelectorAll<HTMLElement>("span[aria-hidden='true']")].map(
        (stroke) => {
          const box = stroke.getBoundingClientRect();
          return {
            x: box.left,
            top: box.top,
            bottom: box.bottom,
            vertical: box.height > box.width,
            lineBottom: lineBox.bottom,
            lineTop: lineBox.top,
          };
        },
      );
    });
  });
}

test("the rails are drawn, not typed", async ({ page }) => {
  await page.goto("/");
  await preparePage(page);

  const block = page.locator("figure").filter({ hasText: "Borrow diagnostic" }).first();
  await expect(block).toBeVisible();

  /*
   * No box-drawing character survives into the text. If one does, it is being
   * set in a fallback face at the wrong advance and every column after it on
   * that line is wrong.
   */
  const text = (await block.locator("code").textContent()) ?? "";
  expect(text.length).toBeGreaterThan(0);
  expect(
    BOX_DRAWING.test(text),
    "a box-drawing character is being rendered as text",
  ).toBe(false);

  // The text itself is still text: the error message reads normally.
  expect(text).toContain("linear value 'vec' is consumed on some paths");

  const drawn = await strokes(page);
  expect(drawn.length, "no rails were drawn").toBeGreaterThan(20);
});

test("the rail is continuous from the header to the closer", async ({ page }) => {
  await page.goto("/");
  await preparePage(page);
  const drawn = await strokes(page);

  /*
   * The general property, which covers the outer rail, the drop from each `┬`
   * and the closing elbow alike: a vertical stroke that reaches the bottom of
   * its line is continued at the top of the next line, in the same column.
   * That is what "continuous" means here, and it holds at any line height
   * because each stroke spans its own line box rather than a glyph.
   */
  const verticals = drawn.filter((stroke) => stroke.vertical);
  expect(verticals.length).toBeGreaterThan(10);

  const reachingBottom = verticals.filter(
    (stroke) => Math.abs(stroke.bottom - stroke.lineBottom) < 1,
  );
  expect(reachingBottom.length, "nothing carries down to the next line").toBeGreaterThan(
    10,
  );

  for (const stroke of reachingBottom) {
    const continued = verticals.some(
      (other) =>
        other !== stroke &&
        Math.abs(other.x - stroke.x) < 0.75 &&
        Math.abs(other.top - stroke.bottom) < 0.75,
    );
    expect(
      continued,
      `a rail at x=${stroke.x.toFixed(1)} stops at y=${stroke.bottom.toFixed(1)} with nothing below it`,
    ).toBe(true);
  }
});

test("runs of the underline meet edge to edge", async ({ page }) => {
  await page.goto("/");
  await preparePage(page);
  const drawn = await strokes(page);

  // Horizontals on the same row must tile: each full cell is one `ch` wide and
  // starts where the last one ended, so a span underline reads as one line.
  const horizontals = drawn.filter((stroke) => !stroke.vertical);
  const rows = new Map<number, number[]>();
  for (const stroke of horizontals) {
    const row = Math.round(stroke.top);
    rows.set(row, [...(rows.get(row) ?? []), stroke.x]);
  }

  const runs = [...rows.values()].filter((xs) => xs.length > 3);
  expect(runs.length, "no underline runs found").toBeGreaterThan(0);

  for (const xs of runs) {
    const sorted = [...xs].sort((a, b) => a - b);
    const cell = sorted[1] - sorted[0];
    for (let index = 1; index < sorted.length; index += 1) {
      expect(
        sorted[index] - sorted[index - 1],
        "an underline has a gap in it",
      ).toBeCloseTo(cell, 0);
    }
  }
});

test("holds up on a phone", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await preparePage(page);

  // The block scrolls inside its own frame rather than widening the page.
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(1);

  const drawn = await strokes(page);
  expect(drawn.length, "the rails vanished at 390px").toBeGreaterThan(20);
});
