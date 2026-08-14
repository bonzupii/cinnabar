import { execFileSync } from "node:child_process";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";

/*
 * Disclosure is a shared component with no page of its own, so there is
 * nothing on the built site that is guaranteed to contain one. Rather than
 * leave its behaviour untested until a page adopts it, these tests render the
 * real component and load its markup into the browser, so what is exercised is
 * the component's own output in a real engine.
 *
 * The render happens in a subprocess (fixtures/render-disclosure.tsx, through
 * `tsx`) because Playwright compiles JSX — in a spec and in everything a spec
 * imports — with its own component-testing factory, which react-dom/server
 * cannot render. Importing the component here would fail at the first tag.
 *
 * Styling is not asserted: the visual specs cover appearance, this covers
 * behaviour.
 */

const BODY = "The grammar is in GRAMMAR.md.";
const FIXTURE = path.join(__dirname, "fixtures", "render-disclosure.tsx");

/** Renders the component once per variant; the subprocess is not free. */
const rendered = new Map<boolean, string>();

function markup(defaultOpen: boolean): string {
  const cached = rendered.get(defaultOpen);
  if (cached) return cached;

  const html = execFileSync(
    "npx",
    ["tsx", FIXTURE, ...(defaultOpen ? ["--open"] : [])],
    { encoding: "utf8", cwd: path.join(__dirname, "..", ".."), shell: true },
  );
  rendered.set(defaultOpen, html);
  return html;
}

async function mount(page: Page, { defaultOpen = false } = {}) {
  await page.setContent(
    `<!doctype html><html><body>${markup(defaultOpen)}</body></html>`,
  );
}

test("starts closed, and opens and closes on click", async ({ page }) => {
  await mount(page);

  const details = page.locator("details");
  const summary = page.locator("summary");
  const body = page.getByText(BODY);

  await expect(details).not.toHaveAttribute("open", /.*/);
  await expect(body).toBeHidden();

  await summary.click();
  await expect(details).toHaveAttribute("open", /.*/);
  await expect(body).toBeVisible();

  await summary.click();
  await expect(details).not.toHaveAttribute("open", /.*/);
  await expect(body).toBeHidden();
});

test("defaultOpen renders the section already open", async ({ page }) => {
  await mount(page, { defaultOpen: true });
  await expect(page.locator("details")).toHaveAttribute("open", /.*/);
  await expect(page.getByText(BODY)).toBeVisible();
});

test("is reachable and operable from the keyboard", async ({ page }) => {
  await mount(page);
  const details = page.locator("details");

  // Tab reaches the summary: it is a real control, not a div with a handler,
  // so focus, Enter/Space and the expanded/collapsed announcement all come
  // from the browser rather than being reimplemented.
  await page.keyboard.press("Tab");
  await expect(page.locator("summary")).toBeFocused();

  await page.keyboard.press("Enter");
  await expect(details).toHaveAttribute("open", /.*/);

  await page.keyboard.press("Space");
  await expect(details).not.toHaveAttribute("open", /.*/);
});

test("keeps its content in the DOM while closed, so it stays crawlable", async ({
  page,
}) => {
  await mount(page);
  /*
   * The reason this is <details> and not a JS toggle: one that unmounts its
   * children hides them from a crawler and from a reader without JavaScript.
   * The markup carries the text either way.
   */
  await expect(page.locator("details")).not.toHaveAttribute("open", /.*/);
  await expect(page.locator("details")).toContainText("GRAMMAR.md");
});

test("labels the section with the summary it was given", async ({ page }) => {
  await mount(page);
  await expect(page.locator("summary")).toHaveText("Full grammar");
});
