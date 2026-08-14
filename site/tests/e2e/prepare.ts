import type { Page } from "@playwright/test";

/**
 * Puts a page into the state a reader actually sees before it is inspected.
 *
 * Entrance reveals start hidden and are entered by an intersection observer,
 * so a scan that runs before an element has been scrolled to would measure it
 * mid-transition — or as fully transparent. Scrolling the whole page and then
 * waiting for every transition to finish removes that race, and is also just
 * a truer reading of the page than a scan of the first viewport.
 */
export async function preparePage(page: Page) {
  await page.evaluate(() => document.fonts.ready);

  await page.evaluate(async () => {
    const step = window.innerHeight * 0.8;
    for (let y = 0; y < document.body.scrollHeight; y += step) {
      window.scrollTo(0, y);
      await new Promise((resolve) => requestAnimationFrame(resolve));
    }
    window.scrollTo(0, 0);
    await new Promise((resolve) => requestAnimationFrame(resolve));
  });

  await page.evaluate(() =>
    Promise.all(document.getAnimations().map((animation) => animation.finished)),
  );
}
