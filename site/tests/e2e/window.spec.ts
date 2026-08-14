import { expect, test } from "@playwright/test";
import { preparePage } from "./prepare";

/*
 * Every dark block on the site — source, shell session, diagnostic, usage
 * synopsis, and any fenced block in a repository document — uses the same
 * window chrome, so a reader never has to work out why two code blocks look
 * like different kinds of thing.
 */

/*
 * /manifesto/ is absent on purpose: MANIFESTO.md contains no fenced blocks, so
 * that page has no windows to check.
 */
const PAGES_WITH_WINDOWS = [
  "/",
  "/install/",
  "/reference/",
  "/architecture/",
  "/roadmap/",
];

test("every window has controls and a centred title", async ({ page }) => {
  for (const path of PAGES_WITH_WINDOWS) {
    await page.goto(path);
    await preparePage(page);

    const captions = page.locator("figure > figcaption");
    const count = await captions.count();
    expect(count, `${path} has no windows`).toBeGreaterThan(0);

    for (let index = 0; index < count; index += 1) {
      const caption = captions.nth(index);
      // Left: the mark and a path. Centre: what the block shows. Right: the
      // three controls. Every window carries all three.
      await expect(caption.locator("svg").first()).toBeAttached();

      /*
       * `textContent`, not `innerText`: some of these windows sit inside a
       * collapsed Disclosure, and `innerText` is the rendered text, which is
       * empty for anything the browser is not laying out. What is being
       * asserted is that the markup carries a path and a title — which is also
       * what a crawler sees — not that the block happens to be on screen.
       */
      const path = caption.locator("> span").first();
      expect(
        ((await path.textContent()) ?? "").trim().length,
        `${path} window ${index} has no path`,
      ).toBeGreaterThan(0);

      const title = caption.locator("span.text-center");
      await expect(title).toHaveCount(1);
      expect(
        ((await title.textContent()) ?? "").trim().length,
        `window ${index} has no centre title`,
      ).toBeGreaterThan(0);

      await expect(caption.locator("div[aria-hidden='true'] > span")).toHaveCount(3);
    }
  }
});

test("the bar reads path, then subject, then controls", async ({ page }) => {
  await page.goto("/");
  await preparePage(page);

  const order = await page.evaluate(() => {
    const caption = document.querySelector<HTMLElement>("figure > figcaption");
    if (!caption) throw new Error("no window caption found");
    const boxes = Array.from(caption.children).map((child) =>
      child.getBoundingClientRect(),
    );
    return boxes.map((box) => Math.round(box.left));
  });
  // Three slots, left to right.
  expect(order).toHaveLength(3);
  expect(order[0]).toBeLessThan(order[1]);
  expect(order[1]).toBeLessThan(order[2]);
});

test("the controls are decorative and hidden from assistive technology", async ({
  page,
}) => {
  await page.goto("/");
  await preparePage(page);
  // There is no window to close; a control that does nothing must not be
  // announced as one.
  const controls = page.locator("figure > figcaption div[aria-hidden='true']").first();
  await expect(controls).toBeAttached();
  await expect(controls.locator("span")).toHaveCount(3);
});

test("the usage synopsis is framed like every other block", async ({ page }) => {
  await page.goto("/reference/");
  await preparePage(page);
  const usage = page.locator("figure").filter({ hasText: "cinnabar --help" }).first();
  await expect(usage).toBeVisible();
  await expect(usage.locator("figcaption span.text-center")).toHaveText("Usage");
});

test("a fenced block in a repository document is framed too", async ({ page }) => {
  await page.goto("/architecture/");
  await preparePage(page);
  // ARCHITECTURE.md contains bash and plain fenced blocks.
  const windows = page.getByRole("main").locator("figure > figcaption");
  expect(await windows.count()).toBeGreaterThan(0);
});

test("window titles are centred against the bar, not against the leftover space", async ({
  page,
}) => {
  await page.goto("/");
  await preparePage(page);

  const offset = await page.evaluate(() => {
    const caption = document.querySelector<HTMLElement>("figure > figcaption");
    const title = caption?.querySelector<HTMLElement>("span.text-center");
    if (!caption || !title) throw new Error("no window caption found");
    const bar = caption.getBoundingClientRect();
    const label = title.getBoundingClientRect();
    return Math.abs(
      (label.left + label.right) / 2 - (bar.left + bar.right) / 2,
    );
  });
  // A couple of pixels of slack for sub-pixel layout.
  expect(offset).toBeLessThanOrEqual(2);
});
