import { expect, test } from "@playwright/test";

/*
 * The sample explorer follows the APG tabs pattern with automatic activation:
 * arrow keys move focus and selection together, and only the selected tab is
 * in the tab sequence.
 */

test("exposes one tablist with a single selected tab", async ({ page }) => {
  await page.goto("/");
  const tablist = page.getByRole("tablist", { name: "Code samples" });
  await expect(tablist.getByRole("tab")).toHaveCount(3);
  await expect(tablist.getByRole("tab", { selected: true })).toHaveCount(1);
  await expect(page.getByRole("tabpanel")).toHaveCount(1);
});

test("clicking a tab swaps the panel and its caption", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("tab", { name: "Slices and patterns" }).click();
  await expect(page.getByRole("tab", { name: "Slices and patterns" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(
    page.getByText("tests/fixtures/repro/slice_test.cnb", { exact: true }),
  ).toBeVisible();
});

test("arrow keys move focus and selection together, and wrap", async ({ page }) => {
  await page.goto("/");
  const tabs = page.getByRole("tablist", { name: "Code samples" }).getByRole("tab");

  await tabs.first().focus();
  await page.keyboard.press("ArrowRight");
  await expect(tabs.nth(1)).toBeFocused();
  await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "true");

  await page.keyboard.press("ArrowLeft");
  await expect(tabs.first()).toBeFocused();

  // Wrapping backwards from the first tab lands on the last.
  await page.keyboard.press("ArrowLeft");
  await expect(tabs.last()).toBeFocused();
  await expect(tabs.last()).toHaveAttribute("aria-selected", "true");
});

test("Home and End jump to the ends", async ({ page }) => {
  await page.goto("/");
  const tabs = page.getByRole("tablist", { name: "Code samples" }).getByRole("tab");

  await tabs.first().focus();
  await page.keyboard.press("End");
  await expect(tabs.last()).toBeFocused();
  await page.keyboard.press("Home");
  await expect(tabs.first()).toBeFocused();
});

test("only the selected tab is in the tab sequence", async ({ page }) => {
  await page.goto("/");
  const tabs = page.getByRole("tablist", { name: "Code samples" }).getByRole("tab");
  await expect(tabs.first()).toHaveAttribute("tabindex", "0");
  await expect(tabs.nth(1)).toHaveAttribute("tabindex", "-1");
  await expect(tabs.nth(2)).toHaveAttribute("tabindex", "-1");
});
