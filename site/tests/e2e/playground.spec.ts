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

test("loading the linear-handles sample reports its own real diagnostics", async ({ page }) => {
  await page.goto("/playground/");
  await preparePage(page);

  const diagnostics = page.locator("figure").filter({ hasText: "Diagnostics" });
  await expect(diagnostics).toContainText("No diagnostics.", { timeout: 15_000 });

  await page.getByRole("tab", { name: "Linear handles" }).click();

  // This sample is an excerpt from tests/fixtures/repro/vec_test.cnb for
  // readability on the static pages, not a complete standalone program, so
  // the checker correctly reports its Collections references as
  // unresolved -- a real, accurate verdict on what was actually submitted.
  await expect(diagnostics).toContainText("cannot resolve import 'Collections.vec_new'", {
    timeout: 10_000,
  });
});
