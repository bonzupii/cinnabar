import { expect, test } from "@playwright/test";
import { preparePage } from "./prepare";
import { ROUTES } from "./routes";

/*
 * The hairline grid must always be covered by its own children.
 *
 * `.rule-grid` paints `--hairline` as its background and lays its children out
 * with a 1px gap, so what a reader sees as a rule between two cells is the
 * container showing through. The device only works while the children cover
 * it: any part they do not reach is not a 1px rule but a block of flat grey.
 *
 * That has now happened three times, from three different causes — a window
 * body that did not grow when the frame was stretched to a taller column, a
 * card stack stretched the same way, and a `fit-content` badge strip whose
 * badges wrapped onto a second line and left its tail bare. So this is checked
 * for every `.rule-grid` on every page rather than for the shape that was last
 * reported.
 *
 * What is measured is the gap between the container's inner edge and the outer
 * edge of the children, on all four sides. That is a bounding box rather than
 * true coverage — a hole in the middle of a grid would not be caught — but
 * every occurrence so far has been an uncovered edge, and a bounding box costs
 * one measurement per element and has no way to be flaky.
 *
 * A negative gap is a child wider than its container, which is not this bug:
 * the tables scroll horizontally inside `overflow-x-auto` on a phone, by
 * design.
 */

/** Sub-pixel layout slack. The bands reported were 26px, 38px and 51px. */
const SLACK = 1.5;

/** Desktop, and the narrowest phone the site is designed for. */
const WIDTHS = [1280, 390] as const;

type Uncovered = {
  side: string;
  gap: number;
  classes: string;
  text: string;
};

test("every hairline grid is covered by its children", async ({ page }) => {
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 900 });

    for (const route of ROUTES) {
      await page.goto(route.path);
      /*
       * The rendered repository documents sit inside a closed <details>, and a
       * closed one is not laid out at all — every rect would be zero and the
       * check would pass without having looked. Opening them puts their tables
       * on screen, which is where a reader who follows a capability anchor
       * sees them.
       */
      await page.evaluate(() => {
        for (const details of document.querySelectorAll("details")) {
          details.open = true;
        }
      });
      await preparePage(page);

      const uncovered = await page.evaluate((slack): Uncovered[] => {
        const found: Uncovered[] = [];

        for (const grid of document.querySelectorAll<HTMLElement>(".rule-grid")) {
          const children = Array.from(grid.children).map((child) =>
            child.getBoundingClientRect(),
          );
          if (children.length === 0) continue;

          const box = grid.getBoundingClientRect();
          const style = getComputedStyle(grid);
          const border = (side: string) =>
            parseFloat(style.getPropertyValue(`border-${side}-width`));

          const gaps = {
            top: Math.min(...children.map((c) => c.top)) - box.top - border("top"),
            bottom:
              box.bottom - border("bottom") - Math.max(...children.map((c) => c.bottom)),
            left: Math.min(...children.map((c) => c.left)) - box.left - border("left"),
            right:
              box.right - border("right") - Math.max(...children.map((c) => c.right)),
          };

          for (const [side, gap] of Object.entries(gaps)) {
            if (gap <= slack) continue;
            found.push({
              side,
              gap: Math.round(gap),
              classes: grid.className,
              text: (grid.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 50),
            });
          }
        }

        return found;
      }, SLACK);

      expect(
        uncovered,
        `${route.path} at ${width}px: hairline showing as a band, not a rule`,
      ).toEqual([]);
    }
  }
});
