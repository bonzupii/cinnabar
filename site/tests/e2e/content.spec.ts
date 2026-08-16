import { expect, test } from "@playwright/test";

/*
 * The manifesto, roadmap and architecture pages are rendered from the
 * repository's own markdown at build time. These checks fail if that wiring
 * breaks — an empty page, an unrewritten link, or an unhighlighted sample.
 */

test("the manifesto renders the repository's own document", async ({ page }) => {
  await page.goto("/manifesto/");
  await expect(page.getByRole("heading", { name: "Core Principles" })).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "7. Linear Types for Resource Management" }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Anti-Principles (Things Cinnabar Will Never Have)" }),
  ).toBeVisible();
});

test("markdown headings carry ids so in-document anchors resolve", async ({ page }) => {
  await page.goto("/manifesto/");
  const heading = page.getByRole("heading", { name: "8. Casing Is Syntax" });
  await expect(heading).toHaveAttribute("id", "8-casing-is-syntax");
});

test("the architecture summary links to its complete source", async ({ page }) => {
  await page.goto("/architecture/");
  const sourceLink = page.getByRole("link", { name: "ARCHITECTURE.md" });
  await expect(sourceLink).toHaveAttribute(
    "href",
    "https://github.com/bonzupii/cinnabar/blob/main/ARCHITECTURE.md",
  );
});

test("no rendered link points at a bare .md path", async ({ page }) => {
  for (const path of ["/manifesto/", "/roadmap/", "/architecture/"]) {
    await page.goto(path);
    const stragglers = await page
      .getByRole("main")
      .locator('a[href$=".md"]:not([href^="http"])')
      .count();
    expect(stragglers, `${path} has unrewritten markdown links`).toBe(0);
  }
});

test("the roadmap capability cards link into the rendered document", async ({ page }) => {
  await page.goto("/roadmap/");
  /*
   * Any card will do, and none is named here on purpose: what is being
   * asserted is that a capability on the summary carries the reader down to
   * the milestone that delivered it, not which capabilities the roadmap lists
   * this week.
   */
  const card = page.getByRole("main").locator('a[href^="#milestone-"]').first();
  await expect(card).toBeVisible();

  const target = (await card.getAttribute("href"))!.slice(1);
  await card.click();
  await expect(page.locator(`#${target}`)).toBeInViewport();
});

test("the roadmap leads with what the language does, not with milestone numbers", async ({
  page,
}) => {
  await page.goto("/roadmap/");
  await expect(
    page.getByRole("heading", { name: "What Cinnabar does today" }),
  ).toBeVisible();
  await expect(page.getByRole("heading", { name: "On the horizon" })).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Self-hosting", exact: true }),
  ).toBeVisible();
});

test("code samples are highlighted in the Cinnabar Dark theme", async ({ page }) => {
  await page.goto("/");
  const panel = page.getByTestId("sample-panel");

  // Plate 09: "Only keywords take the accent, so the eye reads control flow
  // first." Vermilion is #E0442A.
  const keyword = panel.locator("code span").filter({ hasText: /^return$/ }).first();
  await expect(keyword).toHaveCSS("color", "rgb(224, 68, 42)");

  // A type is the bright text colour, not the accent.
  const type = panel.locator("code span").filter({ hasText: /^I64$/ }).first();
  await expect(type).toHaveCSS("color", "rgb(237, 233, 230)");

  // A constant declaration is not coloured as a keyword.
  const constant = panel.locator("code span").filter({ hasText: /^BAD_NEW$/ }).first();
  await expect(constant).toHaveCSS("color", "rgb(237, 233, 230)");
});

test("comments are muted and italic", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("tab", { name: "Linear handles" }).click();

  const comment = page
    .getByTestId("sample-panel")
    .locator("code span")
    .filter({ hasText: "# linear handle consumed exactly once" })
    .first();
  await expect(comment).toHaveCSS("color", "rgb(138, 131, 126)");
  await expect(comment).toHaveCSS("font-style", "italic");
});

test("linear handles are marked structurally, not chromatically", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("tab", { name: "Linear handles" }).click();

  const panel = page.getByTestId("sample-panel");
  const handle = panel.locator("span.linear-handle").first();
  await expect(handle).toHaveText("vec");
  // Plate 09: a dotted rule, and the identifier keeps the ordinary colour.
  await expect(handle).toHaveCSS("border-bottom-style", "dotted");
  await expect(handle).toHaveCSS("color", "rgb(201, 194, 189)");
});

test("the reference lists every project subcommand", async ({ page }) => {
  await page.goto("/reference/");
  for (const command of [
    "cinnabar init [PATH]",
    "cinnabar build [PATH] [--target host]",
    "cinnabar check [PATH]",
    "cinnabar test [PATH] [--update-snapshots]",
    "cinnabar doc [PATH] [-o DIR]",
    "cinnabar burn [PATH] [--address ADDR]",
  ]) {
    await expect(page.getByRole("rowheader", { name: command })).toBeVisible();
  }
});

test("the manifest anchor on the reference page resolves", async ({ page }) => {
  await page.goto("/reference/#manifest");
  await expect(page.locator("#manifest")).toBeInViewport();
});
