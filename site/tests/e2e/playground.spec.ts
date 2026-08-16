import { expect, test } from "@playwright/test";

test("the homepage embeds the real checker without nested source scrolling", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("live ownership check", { exact: true })).toBeVisible();
  await expect(page.getByTestId("playground-diagnostics")).toContainText(/linear|consum/i);
  const editor = page.locator(".cm-content").first();
  await expect(editor).toHaveAttribute("contenteditable", "true");
  await expect(editor).toContainText("fun use_block");

  const terminal = page.locator("figure").filter({ hasText: "playground.cnb" }).first();
  const terminalBox = await terminal.boundingBox();
  expect(terminalBox?.height ?? Number.POSITIVE_INFINITY).toBeLessThan(720);

  const scroller = terminal.locator(".cm-scroller");
  const overflow = await scroller.evaluate((element) => {
    return {
      clientHeight: element.clientHeight,
      clientWidth: element.clientWidth,
      scrollHeight: element.scrollHeight,
      scrollWidth: element.scrollWidth,
    };
  });
  expect(overflow.scrollHeight).toBeLessThanOrEqual(overflow.clientHeight + 1);
  expect(overflow.scrollWidth).toBeLessThanOrEqual(overflow.clientWidth + 1);

  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload();
  await expect(page.getByTestId("playground-diagnostics")).toContainText(/linear|consum/i);
  const mobileScroller = page.locator("figure").filter({ hasText: "playground.cnb" }).first().locator(".cm-scroller");
  const mobileOverflow = await mobileScroller.evaluate((element) => {
    return {
      clientHeight: element.clientHeight,
      clientWidth: element.clientWidth,
      scrollHeight: element.scrollHeight,
      scrollWidth: element.scrollWidth,
    };
  });
  expect(mobileOverflow.scrollHeight).toBeLessThanOrEqual(mobileOverflow.clientHeight + 1);
  expect(mobileOverflow.scrollWidth).toBeLessThanOrEqual(mobileOverflow.clientWidth + 1);
});
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

  const diagnostics = page.getByTestId("playground-diagnostics");
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

  const diagnostics = page.getByTestId("playground-diagnostics");
  const editor = page.locator(".cm-content");
  await expect(diagnostics).toContainText("No diagnostics.", { timeout: 15_000 });

  // Unlike content/samples.ts's homepage excerpts -- trimmed for readability
  // and never actually checked there -- every playground starter is a
  // complete, verified-clean program (content/playground-samples.ts), so
  // clicking through all of them should never surface a diagnostic.
  const labels = [
    "Linear handles",
    "Slices and patterns",
    "Result & errors",
    "Traits & impl",
    "Enums & matching",
    "Tail recursion",
  ];
  for (const label of labels) {
    await page.getByRole("tab", { name: label }).click();
    await expect(editor).not.toBeEmpty();
    await expect(diagnostics).toContainText("No diagnostics.", { timeout: 10_000 });
  }
});

test("hovering a line number highlights its line in the editor", async ({ page }) => {
  await page.goto("/playground/");
  await preparePage(page);

  const diagnostics = page.getByTestId("playground-diagnostics");
  await expect(diagnostics).toContainText("No diagnostics.", { timeout: 15_000 });

  // The hand-drawn gutter (cinnabar-codemirror.ts explains why it isn't
  // CodeMirror's own) has no ARIA role of its own, so it's found by its
  // rendered text -- the third row, i.e. line 3.
  const thirdLine = page.locator(".text-term-gutter > div").nth(2);
  await expect(page.locator(".cm-hovered-line")).toHaveCount(0);

  await thirdLine.hover();
  await expect(page.locator(".cm-hovered-line")).toHaveCount(1);

  await page.mouse.move(0, 0);
  await expect(page.locator(".cm-hovered-line")).toHaveCount(0);
});

test("hovering an identifier shows its resolved signature", async ({ page }) => {
  await page.goto("/playground/");
  await preparePage(page);

  const diagnostics = page.getByTestId("playground-diagnostics");
  await expect(diagnostics).toContainText("No diagnostics.", { timeout: 15_000 });

  // The recursive call, not the definition -- a genuine test that hover
  // follows the resolved symbol rather than just echoing text under the
  // cursor. cinnabarHighlighting wraps every token in its own span, so this
  // targets the exact one rather than an arbitrary point in .cm-content.
  await page.locator(".cm-content span", { hasText: /^hanoi_acc$/ }).last().hover();

  const tooltip = page.locator(".cm-tooltip-hover");
  await expect(tooltip).toBeVisible({ timeout: 5_000 });
  await expect(tooltip).toContainText("hanoi_acc");
  await expect(tooltip).toContainText("I64");
});

test("hovering a keyword highlights it even though it has no intellisense signature", async ({
  page,
}) => {
  await page.goto("/playground/");
  await preparePage(page);

  const diagnostics = page.getByTestId("playground-diagnostics");
  await expect(diagnostics).toContainText("No diagnostics.", { timeout: 15_000 });

  // `cinnabarTokenHover` highlights whatever token the pointer is over
  // independent of `analysis::hover`, which is the point -- a keyword never
  // resolves to a signature, so this is the one hover affordance covering it.
  await page.locator(".cm-content span", { hasText: /^fun$/ }).first().hover();

  await expect(page.locator(".cm-hovered-token")).toHaveText("fun");
  await expect(page.locator(".cm-tooltip-hover")).toHaveCount(0);
});

test("moving from a resolved symbol to a keyword clears the previous tooltip", async ({
  page,
}) => {
  await page.goto("/playground/");
  await preparePage(page);

  const diagnostics = page.getByTestId("playground-diagnostics");
  await expect(diagnostics).toContainText("No diagnostics.", { timeout: 15_000 });

  await page.locator(".cm-content span", { hasText: /^hanoi_acc$/ }).last().hover();
  await expect(page.locator(".cm-tooltip-hover")).toBeVisible({ timeout: 5_000 });

  // A keyword has no hover info of its own -- moving to one must dismiss the
  // stale tooltip rather than leave the last-resolved symbol's card on
  // screen pointing at the wrong token.
  await page.locator(".cm-content span", { hasText: /^fun$/ }).first().hover();
  await expect(page.locator(".cm-tooltip-hover")).toHaveCount(0, { timeout: 5_000 });
});
