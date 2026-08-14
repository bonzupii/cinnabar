import { expect, test } from "@playwright/test";
import { ROUTES } from "./routes";

test("the header marks the current section", async ({ page }) => {
  await page.goto("/manifesto/");
  const nav = page.getByRole("navigation", { name: "Primary" });
  await expect(nav.getByRole("link", { name: "Manifesto" })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await expect(nav.getByRole("link", { name: "Roadmap" })).not.toHaveAttribute(
    "aria-current",
    "page",
  );
});

test("every header link reaches its page", async ({ page }) => {
  for (const route of ROUTES.slice(1)) {
    await page.goto("/");
    await page
      .getByRole("navigation", { name: "Primary" })
      .getByRole("link", { name: new RegExp(route.name, "i") })
      .click();
    await expect(page).toHaveURL(new RegExp(`${route.path}$`));
    await expect(page.locator("h1")).toHaveText(route.heading);
  }
});

test.describe("mobile menu", () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test("opens, traps focus, and closes on Escape restoring the trigger", async ({
    page,
  }) => {
    await page.goto("/");
    const trigger = page.getByRole("button", { name: /open menu/i });
    await trigger.click();

    const dialog = page.getByRole("dialog", { name: /site navigation/i });
    await expect(dialog).toBeVisible();
    // Focus moves into the panel on open.
    await expect(dialog.getByRole("link").first()).toBeFocused();

    // Shift+Tab from the first item wraps to the last, rather than escaping
    // into the page behind the dialog.
    await page.keyboard.press("Shift+Tab");
    await expect(dialog.getByRole("link").last()).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(dialog.getByRole("link").first()).toBeFocused();

    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(trigger).toBeFocused();
  });

  test("closes when the backdrop is clicked", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: /open menu/i }).click();
    const dialog = page.getByRole("dialog", { name: /site navigation/i });
    await expect(dialog).toBeVisible();

    // Click well below the panel: the panel sits at the top of the overlay and
    // would otherwise intercept the click, which tests the wrong thing.
    const backdrop = page.getByTestId("mobile-menu-backdrop");
    const panelBox = (await dialog.boundingBox())!;
    const backdropBox = (await backdrop.boundingBox())!;
    await backdrop.click({
      position: { x: 8, y: panelBox.height + (backdropBox.height - panelBox.height) / 2 },
    });
    await expect(dialog).toBeHidden();
  });

  test("closes after following a link, and restores page scrolling", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: /open menu/i }).click();
    await expect
      .poll(() => page.evaluate(() => document.body.style.overflow))
      .toBe("hidden");

    await page.getByRole("dialog").getByRole("link", { name: /Roadmap/ }).click();
    await expect(page).toHaveURL(/\/roadmap\/$/);
    await expect(page.getByRole("dialog")).toBeHidden();
    // A dialog that leaves `overflow: hidden` behind freezes the new page.
    await expect
      .poll(() => page.evaluate(() => document.body.style.overflow))
      .not.toBe("hidden");
  });
});

test("no page overflows horizontally on a phone", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  for (const route of ROUTES) {
    await page.goto(route.path);
    await page.evaluate(() => document.fonts.ready);
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    expect(overflow, `${route.name} overflows by ${overflow}px`).toBeLessThanOrEqual(1);
  }
});

test("the footer links to every documentation page", async ({ page }) => {
  await page.goto("/");
  const footer = page.getByRole("navigation", { name: "Footer" });
  for (const route of ROUTES.slice(1)) {
    await expect(footer.getByRole("link", { name: new RegExp(route.name, "i") })).toBeVisible();
  }
});
