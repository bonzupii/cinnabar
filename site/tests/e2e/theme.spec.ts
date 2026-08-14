import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { preparePage } from "./prepare";
import { ROUTES } from "./routes";

/*
 * The theme is a three-state control: follow the system, or an explicit
 * choice in either direction. The brand board's own default is dark, so that
 * is what a visitor with no system preference gets.
 */

const toggle = (page: import("@playwright/test").Page) =>
  page.getByRole("button", { name: /switch between light and dark/i });

const resolvedTheme = (page: import("@playwright/test").Page) =>
  page.evaluate(() => {
    const chosen = document.documentElement.getAttribute("data-theme");
    if (chosen) return chosen;
    return window.matchMedia("(prefers-color-scheme: light)").matches
      ? "light"
      : "dark";
  });

test.describe("dark is the base, before any preference is applied", () => {
  /*
   * `prefers-color-scheme: no-preference` was dropped from the spec, and
   * Chromium resolves an absent OS preference to `light`. So "what happens
   * with no preference at all" is not observable in a browser, and asserting
   * it here would be asserting a fiction.
   *
   * What is real, and what the brand requires, is that the *base* rule is
   * dark and light only arrives via a media query or an explicit choice.
   * tests/unit/palette.test.ts asserts that directly against globals.css.
   * This checks the browser-visible half: an explicit choice is honoured, and
   * the stylesheet's own default carries the dark ground.
   */
  test.use({ colorScheme: "dark" });

  test("uses the dark palette", async ({ page }) => {
    await page.goto("/");
    await expect(toggle(page)).toHaveAttribute("aria-pressed", "true");
    await expect(page.locator("body")).toHaveCSS(
      "background-color",
      "rgb(16, 14, 13)",
    );
  });
});

test.describe("following the system preference", () => {
  test.use({ colorScheme: "light" });

  test("uses the light palette without an explicit choice", async ({ page }) => {
    await page.goto("/");
    // Plate 05's light surface: #F2EEEA.
    await expect(page.locator("body")).toHaveCSS(
      "background-color",
      "rgb(242, 238, 234)",
    );
    await expect(toggle(page)).toHaveAttribute("aria-pressed", "false");
  });

  test("an explicit dark choice overrides a light system preference", async ({
    page,
  }) => {
    await page.goto("/");
    await toggle(page).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(page.locator("body")).toHaveCSS(
      "background-color",
      "rgb(16, 14, 13)",
    );
  });
});

test.describe("dark system preference", () => {
  test.use({ colorScheme: "dark" });

  test("an explicit light choice overrides it", async ({ page }) => {
    await page.goto("/");
    await toggle(page).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await expect(page.locator("body")).toHaveCSS(
      "background-color",
      "rgb(242, 238, 234)",
    );
  });
});

test("the choice persists across navigation without flashing", async ({ page }) => {
  await page.goto("/");
  await toggle(page).click();
  const chosen = await resolvedTheme(page);

  await page.goto("/reference/");
  // The inline script in <head> runs before paint, so the attribute is already
  // set by the time the document is parsed rather than applied on hydration.
  await expect(page.locator("html")).toHaveAttribute("data-theme", chosen);
  expect(await resolvedTheme(page)).toBe(chosen);
});

test("code blocks keep the Cinnabar Dark ground in both themes", async ({ page }) => {
  for (const theme of ["dark", "light"] as const) {
    await page.goto("/");
    await page.evaluate((value) => {
      document.documentElement.setAttribute("data-theme", value);
    }, theme);

    // Plate 09's theme is specified against the dark ground; plate 14 forbids
    // adding colours to it, so there is no light variant to switch to.
    const block = page.getByTestId("sample-panel").locator("pre").first();
    await expect(block).toHaveCSS("background-color", "rgb(16, 14, 13)");
    const keyword = block.locator("span").filter({ hasText: /^return$/ }).first();
    await expect(keyword).toHaveCSS("color", "rgb(224, 68, 42)");
  }
});

test("the light theme has no accessibility violations", async ({ page }) => {
  for (const route of ROUTES) {
    await page.goto(route.path);
    await page.evaluate(() => {
      document.documentElement.setAttribute("data-theme", "light");
    });
    await preparePage(page);

    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
      .analyze();

    expect(
      results.violations.map((violation) => ({
        route: route.name,
        id: violation.id,
        nodes: violation.nodes.map((node) => node.target.join(" ")),
      })),
    ).toEqual([]);
  }
});
