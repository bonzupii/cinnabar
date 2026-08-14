import { expect, test, type Page } from "@playwright/test";
import { LOGO_CLICK_THRESHOLD, LOGO_CLICK_WINDOW_MS } from "@/lib/logo-easter-egg";
import { preparePage } from "./prepare";

/*
 * The Konami-code easter egg (MushroomEasterEgg), driven the way a visitor
 * would find it: real key presses on a real page, then the foraging puzzle
 * played through the three tiles with the live checker refereeing.
 *
 * The matcher works on `KeyboardEvent.code`, and Playwright's keyboard.press
 * produces genuine codes -- press("b") lands as code "KeyB" -- so the
 * sequence is typed rather than synthesised via page.evaluate.
 *
 * Every verdict asserted here is the real check()'s: the dialog re-runs the
 * assembled program through the wasm checker after every move, so these
 * tests are also live proof the puzzle's outcomes still hold against the
 * shipped build.
 */

const KONAMI_PRESSES = [
  "ArrowUp",
  "ArrowUp",
  "ArrowDown",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "ArrowLeft",
  "ArrowRight",
  "b",
  "a",
] as const;

async function pressKonami(page: Page) {
  for (const key of KONAMI_PRESSES) {
    await page.keyboard.press(key);
  }
}

async function openEgg(page: Page) {
  await page.goto("/");
  await preparePage(page);
  await pressKonami(page);
  await expect(page.getByRole("dialog")).toBeVisible();
}

/** Clicks tiles in order: forage on a growing tile, eat on a held one. */
async function clickTiles(page: Page, ids: readonly number[]) {
  for (const id of ids) {
    await page.getByTestId(`mushroom-tile-${id}`).click();
  }
}

test("the Konami code opens the mushroom dialog, and the empty program draws the SPECIES nudge", async ({
  page,
}) => {
  await page.goto("/");
  await preparePage(page);

  const dialog = page.getByRole("dialog");
  await expect(dialog).toHaveCount(0);

  await pressKonami(page);

  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Cantharellus cinnabarinus");
  // Zero moves is deliberately not clean: the component's own copy frames the
  // checker's real "unused constant" complaint as the opening nudge.
  await expect(dialog).toContainText("SPECIES is unused");
  // The dialog's first open is what loads the wasm checker, so the first
  // verdict takes as long as the playground's does.
  await expect(dialog).toContainText("unused constant", { timeout: 15_000 });
});

test("foraging all three before eating any is rejected by the real checker", async ({
  page,
}) => {
  await openEgg(page);
  const dialog = page.getByRole("dialog");

  // The obvious order: pick everything up, then eat. Each forage's
  // `Err => return 1` arm is a real early-return path, and the mushrooms
  // still held there leak on it.
  await clickTiles(page, [1, 2, 3, 1, 2, 3]);

  await expect(dialog).toContainText("must be consumed before returning", {
    timeout: 15_000,
  });
  await expect(page.getByTestId("mushroom-solved")).toHaveCount(0);
});

test("fully dispatching each mushroom in turn solves the puzzle", async ({ page }) => {
  await openEgg(page);

  // Forage 1, eat 1, forage 2, eat 2, forage 3, eat 3: no mushroom is ever
  // held across another forage's error path, so the program checks clean.
  await clickTiles(page, [1, 1, 2, 2, 3, 3]);

  await expect(page.getByTestId("mushroom-solved")).toBeVisible({ timeout: 15_000 });
});

test("eating an eaten mushroom un-solves it, and undo restores the win", async ({
  page,
}) => {
  await openEgg(page);
  const dialog = page.getByRole("dialog");
  const solved = page.getByTestId("mushroom-solved");

  await clickTiles(page, [1, 1, 2, 2, 3, 3]);
  await expect(solved).toBeVisible({ timeout: 15_000 });

  // An eaten tile stays clickable; clicking it appends a second eat, and the
  // checker answers with the same "use of moved value" the old two-scene egg
  // demonstrated -- reached here by actually making the mistake.
  await clickTiles(page, [1]);
  await expect(solved).toHaveCount(0);
  await expect(dialog).toContainText("use of moved value", { timeout: 15_000 });

  await page.getByTestId("mushroom-undo").click();
  await expect(solved).toBeVisible({ timeout: 15_000 });
});

test("reset returns every tile to growing and the move count to zero", async ({ page }) => {
  await openEgg(page);

  await clickTiles(page, [1, 1, 2]);
  await expect(page.getByTestId("mushroom-move-count")).toHaveText("Moves 3");

  await page.getByTestId("mushroom-reset").click();

  for (const id of [1, 2, 3]) {
    await expect(page.getByTestId(`mushroom-tile-${id}`)).toHaveAttribute(
      "data-state",
      "growing",
    );
  }
  await expect(page.getByTestId("mushroom-move-count")).toHaveText("Moves 0");
});

test("the sequence typed inside the playground editor does not open the dialog", async ({
  page,
}) => {
  await page.goto("/playground/");
  await preparePage(page);

  // Focus CodeMirror's contenteditable content: arrow keys here are real
  // cursor movement, and the egg firing mid-edit is the regression guarded.
  await page.locator(".cm-content").click();
  await pressKonami(page);

  // The dialog opens synchronously on the final keydown when it opens at
  // all, so its continued absence after the last press is conclusive.
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("spam-clicking the header logo opens the mushroom dialog", async ({ page }) => {
  await page.goto("/");
  await preparePage(page);

  const dialog = page.getByRole("dialog");
  await expect(dialog).toHaveCount(0);

  // clickCount performs the mousedown/mouseup pairs back to back with no
  // delay after a single actionability check, so every click event lands
  // well inside the burst window -- a loop of separate click() calls would
  // re-check actionability each time and could straddle it on a slow run.
  await page
    .locator('a[aria-label="Cinnabar — home"]')
    .click({ clickCount: LOGO_CLICK_THRESHOLD });

  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Cantharellus cinnabarinus");
});

test("slow, spread-out clicks on the logo never open the dialog", async ({ page }) => {
  await page.goto("/");
  await preparePage(page);

  // The regression that matters: the logo is first a navigation control, and
  // a visitor clicking it at a human pace must never summon the egg. More
  // clicks than the threshold, each spaced past half the window, so no
  // window-sized slice ever holds a triggering burst.
  const logo = page.locator('a[aria-label="Cinnabar — home"]');
  for (let click = 0; click < LOGO_CLICK_THRESHOLD + 1; click += 1) {
    await logo.click();
    await page.waitForTimeout(LOGO_CLICK_WINDOW_MS * 0.6);
  }

  // The dialog opens synchronously on the triggering click when it opens at
  // all, so its continued absence after the last click is conclusive.
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("Escape closes the dialog", async ({ page }) => {
  await page.goto("/");
  await preparePage(page);

  await pressKonami(page);
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});
