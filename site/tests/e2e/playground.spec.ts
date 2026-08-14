import { expect, test } from "@playwright/test";
import { preparePage } from "./prepare";

/*
 * The in-browser playground, exercised the way a visitor actually would:
 * typing, and switching between the sample starters.
 *
 * Two real bugs turned up writing this by hand in a browser before this spec
 * existed -- AnimatePresence leaving a stale "clean" state on screen forever
 * once a real error had already reached React state, and sample tabs
 * updating state without the visible editor content following. Both are
 * regressions this spec would have caught outright.
 */

test("the default sample checks clean, typing invalid source reports it, and reloading a sample resets both", async ({
  page,
}) => {
  await page.goto("/playground/");
  await preparePage(page);

  const diagnostics = page.locator("figure").filter({ hasText: "Diagnostics" });
  const editor = page.locator(".cm-content");

  // The wasm module has to load and run once before the first verdict lands.
  await expect(diagnostics).toContainText("No diagnostics.", { timeout: 15_000 });

  await editor.click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.type("this is not @@@ valid cinnabar");

  await expect(diagnostics).toContainText("Error", { timeout: 10_000 });
  await expect(diagnostics).not.toContainText("No diagnostics.");

  await page.getByRole("tab", { name: "Tail recursion" }).click();

  await expect(editor).toContainText("pub const DISKS");
  await expect(diagnostics).toContainText("No diagnostics.", { timeout: 10_000 });
});

test("every starter sample checks clean on its own", async ({ page }) => {
  await page.goto("/playground/");
  await preparePage(page);

  const diagnostics = page.locator("figure").filter({ hasText: "Diagnostics" });
  const editor = page.locator(".cm-content");
  await expect(diagnostics).toContainText("No diagnostics.", { timeout: 15_000 });

  // Unlike content/samples.ts's homepage excerpts -- trimmed for readability
  // and never actually checked there -- every playground starter is a
  // complete, verified-clean program (content/playground-samples.ts), so
  // clicking through all three should never surface a diagnostic.
  for (const label of ["Linear handles", "Slices and patterns", "Tail recursion"]) {
    await page.getByRole("tab", { name: label }).click();
    await expect(editor).not.toBeEmpty();
    await expect(diagnostics).toContainText("No diagnostics.", { timeout: 10_000 });
  }
});
