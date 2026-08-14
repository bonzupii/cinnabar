import { expect, test } from "@playwright/test";
import { preparePage } from "./prepare";
import { ROUTES } from "./routes";

/*
 * A full-page screenshot is taken by scrolling and stitching, and a sticky
 * element repaints at every step — so the page never settles and the capture
 * times out waiting for two identical frames. Pinning sticky elements in place
 * for the capture only affects how the screenshot is taken; the stickiness
 * itself is exercised by the contents-rail tests.
 */
const UNSTICK = `*, *::before, *::after { position: static !important; }
  [data-sticky-keep] { position: sticky !important; }`;

/*
 * Full-page visual baselines, in both themes.
 *
 * Plate 05 states "the screen system stays dark", and dark remains the
 * default, but the site now carries a light theme too — so each route needs a
 * baseline in each, or a regression in one would go unseen.
 */
for (const route of ROUTES) {
  test(`${route.name} — dark`, async ({ page }) => {
    await page.goto(route.path);
    await page.waitForLoadState("networkidle");
    // Web fonts shift metrics noticeably at these sizes; wait for them rather
    // than baking a half-loaded render into the baseline.
    await preparePage(page);
    await page.addStyleTag({ content: UNSTICK });
    await expect(page).toHaveScreenshot(`${route.name}-dark.png`, { fullPage: true });
  });

  test(`${route.name} — light`, async ({ page }) => {
    await page.goto(route.path);
    await page.evaluate(() => {
      document.documentElement.setAttribute("data-theme", "light");
    });
    await page.waitForLoadState("networkidle");
    await preparePage(page);
    await page.addStyleTag({ content: UNSTICK });
    await expect(page).toHaveScreenshot(`${route.name}-light.png`, { fullPage: true });
  });
}

test("404 page", async ({ page }) => {
  const response = await page.goto("/no-such-page/");
  expect(response?.status()).toBe(404);
  await page.evaluate(() => document.fonts.ready);
  await expect(page).toHaveScreenshot("not-found.png", { fullPage: true });
});

test("home renders at mobile width without horizontal overflow", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await page.evaluate(() => document.fonts.ready);
  await expect(page).toHaveScreenshot("home-mobile.png", { fullPage: true });
});
