import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { preparePage } from "./prepare";
import { ROUTES } from "./routes";

/*
 * Automated accessibility checks.
 *
 * The palette is fixed by the brand board, so a contrast regression here means
 * a token was used against the wrong surface — vermilion carrying small body
 * text, for instance, which plate 05 explicitly forbids ("above 18 px, or as a
 * mark, only").
 */
for (const route of ROUTES) {
  test(`${route.name} has no detectable accessibility violations`, async ({ page }) => {
    await page.goto(route.path);
    await preparePage(page);

    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
      .analyze();

    expect(
      results.violations.map((violation) => ({
        id: violation.id,
        impact: violation.impact,
        nodes: violation.nodes.map((node) => node.target.join(" ")),
      })),
    ).toEqual([]);
  });
}

test("the mobile navigation dialog is accessible while open", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await page.getByRole("button", { name: /open menu/i }).click();
  await expect(page.getByRole("dialog", { name: /site navigation/i })).toBeVisible();

  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
    .analyze();

  expect(results.violations.map((violation) => violation.id)).toEqual([]);
});

test("the wordmark announces the project name, not its letterforms", async ({ page }) => {
  await page.goto("/");

  // The wordmark draws a C as SVG and sets the remaining letters as text, so
  // its raw content reads "INNABAR". Every place it appears must expose
  // "Cinnabar" to assistive technology instead.
  await expect(
    page.getByRole("link", { name: /^Cinnabar/ }).first(),
  ).toBeVisible();
  await expect(page.getByRole("heading", { level: 1 })).toHaveAccessibleName("Cinnabar");

  // The letterform run itself must be hidden from the accessibility tree.
  const exposed = await page.evaluate(() =>
    Array.from(document.querySelectorAll("span"))
      .filter((element) => element.textContent?.trim() === "INNABAR")
      .filter((element) => !element.closest("[aria-hidden='true']")).length,
  );
  expect(exposed).toBe(0);
});

test("every page has exactly one h1", async ({ page }) => {
  for (const route of ROUTES) {
    await page.goto(route.path);
    await expect(page.locator("h1")).toHaveCount(1);
    // The home h1 is the wordmark, whose raw text reads "CinnabarINNABAR"
    // — the sr-only name plus the aria-hidden letterforms. What matters is
    // the name it exposes, not the characters it contains.
    await expect(page.locator("h1")).toHaveAccessibleName(route.heading);
  }
});

test("the skip link reaches the main landmark", async ({ page }) => {
  await page.goto("/");
  await page.keyboard.press("Tab");
  const skip = page.getByRole("link", { name: /skip to content/i });
  await expect(skip).toBeFocused();
  await skip.click();
  await expect(page.locator("#main-content")).toBeFocused();
});
