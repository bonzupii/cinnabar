import { expect, test } from "@playwright/test";
import { preparePage } from "./prepare";

/*
 * ARCHITECTURE.md draws the compiler pipeline as an untagged fenced block of
 * box characters. It used to render through the shared window chrome, titled
 * "output" — which is what it is not: nothing printed it, it is a figure.
 *
 * These checks are about the shape of the block, never its wording, which is
 * also how the renderer decides. An edit upstream changes what the figure says
 * and none of this.
 */

test("the pipeline figure is a diagram, not a terminal window", async ({ page }) => {
  await page.goto("/architecture/");
  await preparePage(page);
  await page.evaluate(() => {
    document.querySelectorAll("details").forEach((node) => (node.open = true));
  });

  const diagram = page.getByRole("main").locator("figure:not([data-window])").first();
  await expect(diagram).toBeVisible();

  /*
   * No window chrome. The three things every window bar carries and this must
   * not: the mark, the three controls, and a path beside them. It is captioned
   * as what it is instead.
   */
  await expect(diagram.locator("svg")).toHaveCount(0);
  await expect(diagram.locator("div[aria-hidden='true'] > span")).toHaveCount(0);
  await expect(diagram.locator("figcaption")).toHaveText(/diagram/i);
});

test("the figure is drawn from the block's own labels, in order", async ({ page }) => {
  await page.goto("/architecture/");
  await preparePage(page);
  await page.evaluate(() => {
    document.querySelectorAll("details").forEach((node) => (node.open = true));
  });

  const nodes = page
    .getByRole("main")
    .locator("figure:not([data-window]) ol > li");
  const count = await nodes.count();
  expect(count).toBeGreaterThan(1);

  /*
   * The first and last labels are asserted as "something a reader can read",
   * not as exact strings: this is a figure whose text is owned by
   * ARCHITECTURE.md, and pinning the wording here would make editing that
   * document a test failure.
   */
  for (let index = 0; index < count; index += 1) {
    expect((await nodes.nth(index).innerText()).trim().length).toBeGreaterThan(0);
  }

  // The stages read top to bottom, which is the whole point of the figure.
  const tops = await nodes.evaluateAll((items) =>
    items.map((item) => item.getBoundingClientRect().top),
  );
  for (let index = 1; index < tops.length; index += 1) {
    expect(tops[index]).toBeGreaterThan(tops[index - 1]);
  }
});

test("no window on the page is titled after an unlabelled block", async ({ page }) => {
  await page.goto("/architecture/");
  await preparePage(page);
  // "Output" was the title the diagram used to be given.
  const titles = page.getByRole("main").locator("figure[data-window] span.text-center");
  for (const title of await titles.all()) {
    expect((await title.textContent())?.trim().toLowerCase()).not.toBe("output");
  }
});
