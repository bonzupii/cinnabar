import { expect, test } from "@playwright/test";

/*
 * The animated path.
 *
 * The rest of the suite runs with reduced motion so screenshots and contrast
 * checks are deterministic; these tests turn it back on, because the reveal
 * has one failure mode that matters — content that never becomes visible.
 *
 * `page.emulateMedia` is used rather than `test.use({ reducedMotion })`: the
 * latter is not declared on the test-options type in this Playwright version,
 * and emulating it per-test is explicit about where it applies.
 */

/** Waits for every transition the reveal started, rather than sleeping. */
async function settleAnimations(page: import("@playwright/test").Page) {
  await page.evaluate(() =>
    Promise.all(document.getAnimations().map((animation) => animation.finished)),
  );
}

/** Scrolls to the bottom, so every reveal has been entered. */
async function scrollThroughPage(page: import("@playwright/test").Page) {
  await page.evaluate(async () => {
    const step = window.innerHeight * 0.8;
    for (let y = 0; y < document.body.scrollHeight; y += step) {
      window.scrollTo(0, y);
      await new Promise((resolve) => requestAnimationFrame(resolve));
    }
  });
}

test("revealed content ends fully visible", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/");

  await scrollThroughPage(page);
  await settleAnimations(page);

  // The reveal wraps the card, so opacity is asserted on the wrapper.
  const card = page.locator(".reveal").filter({ hasText: "Ownership without ceremony" }).first();
  await expect(card).toBeVisible();
  await expect(card).toHaveClass(/is-in/);
  await expect(card).toHaveCSS("opacity", "1");
});

test("content below the fold is opaque once scrolled to", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/");

  await scrollThroughPage(page);
  await settleAnimations(page);

  const closing = page.locator(".reveal").filter({ hasText: /early development, with the contracts written down/i }).first();
  await expect(closing).toBeVisible();
  await expect(closing).toHaveCSS("opacity", "1");
});

test("reduced motion renders the final state with no animation at all", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/");

  const closing = page.locator(".reveal").filter({ hasText: /early development, with the contracts written down/i }).first();
  // Not merely faster — present immediately, without being scrolled to.
  await expect(closing).toHaveCSS("opacity", "1");
});

test("the served HTML never hides content behind an entrance animation", async ({
  request,
}) => {
  /*
   * Motion renders its `initial` styles during SSR, so a reveal written the
   * obvious way ships `opacity:0` in the markup — and every revealed section
   * is then permanently invisible to a reader without JavaScript. Reveal
   * renders a plain element until hydration for exactly this reason; this
   * asserts the built HTML against a regression.
   */
  for (const path of ["/", "/roadmap/", "/architecture/", "/install/", "/reference/"]) {
    const response = await request.get(path);
    const html = await response.text();
    expect(html, `${path} ships hidden content`).not.toMatch(/opacity:\s*0[^.\d]/);
    // The hidden state is CSS gated on the `js` class, so the markup itself
    // carries only the class name, never the styles.
    expect(html).toContain("reveal");
  }
});
