import { expect, test, type Page } from "@playwright/test";

/*
 * The recent-commit feed on the roadmap page.
 *
 * The feed is an enhancement over a list the build prerendered, so what is
 * tested here is almost entirely what happens when the enhancement does not
 * arrive. The happy path is one test; the rest are the reader whose request is
 * blocked, rate limited, answered with nonsense, or never made because they
 * have JavaScript off.
 *
 * The invariant every one of them shares: the section is a complete, correct
 * part of the page in every state. No spinner, no error, no empty box, and
 * nothing below it moves when data lands.
 */

const API = "**://api.github.com/**";

/*
 * Anything that would betray a half-finished widget.
 *
 * Deliberately not /error|failed|undefined/: those are ordinary words in a
 * compiler's commit subjects, and the feed renders commit subjects. What is
 * being looked for is a widget talking about itself.
 */
const BROKEN = /loading|please try again|something went wrong|couldn't load/i;

const STUB = [
  {
    sha: "1111111111111111111111111111111111111111",
    commit: {
      message: "feat: a stubbed commit subject\n\nand a body that must not show",
      author: { date: "2026-08-12T09:00:00Z" },
    },
  },
  {
    sha: "2222222222222222222222222222222222222222",
    commit: {
      message: "fix: a second stubbed subject",
      author: { date: "2026-08-11T09:00:00Z" },
    },
  },
];

/** The feed in whichever state it is in — the commit list or the fallback. */
function feed(page: Page) {
  return page.locator("[data-activity]");
}

/** The section heading below the feed, for measuring whether anything moved. */
function below(page: Page) {
  return page.getByRole("heading", { name: "The full record" });
}

async function assertSectionIsWhole(page: Page) {
  // The heading, the slot and the link are all present whatever happened.
  await expect(page.getByRole("heading", { name: "Recent activity" })).toBeVisible();
  await expect(feed(page)).toHaveCount(1);
  await expect(
    page.getByRole("link", { name: /the full commit log/i }),
  ).toHaveAttribute("href", "https://github.com/bonzupii/cinnabar/commits/main/");

  // The slot is the reserved height in both states, so the section can never
  // collapse to a sliver while it waits for something.
  const height = await feed(page).evaluate(
    (element) => element.getBoundingClientRect().height,
  );
  expect(height).toBeGreaterThanOrEqual(226);

  await expect(feed(page)).not.toHaveText(BROKEN);
}

test.describe("when GitHub cannot be reached", () => {
  test("the section is still whole with the request aborted", async ({ page }) => {
    await page.route(API, (route) => route.abort("failed"));
    await page.goto("/roadmap/");
    await assertSectionIsWhole(page);
  });

  test("the section is still whole when the reader is rate limited", async ({
    page,
  }) => {
    await page.route(API, (route) =>
      route.fulfill({
        status: 403,
        contentType: "application/json",
        headers: { "x-ratelimit-remaining": "0" },
        body: JSON.stringify({
          message: "API rate limit exceeded for 203.0.113.9.",
          documentation_url: "https://docs.github.com/rest/overview/rate-limits",
        }),
      }),
    );
    await page.goto("/roadmap/");
    await assertSectionIsWhole(page);
  });

  test("the section is still whole when the response is not JSON", async ({
    page,
  }) => {
    // A captive portal, a corporate proxy, a content blocker returning a page.
    await page.route(API, (route) =>
      route.fulfill({ status: 200, contentType: "text/html", body: "<html>no</html>" }),
    );
    await page.goto("/roadmap/");
    await assertSectionIsWhole(page);
  });

  test("the section is still whole when the repository has no commits", async ({
    page,
  }) => {
    // What a brand-new repository returns. It is also the shape a releases
    // panel would have had to handle, which is why there is no releases panel.
    await page.route(API, (route) =>
      route.fulfill({ status: 200, contentType: "application/json", body: "[]" }),
    );
    await page.goto("/roadmap/");
    await assertSectionIsWhole(page);
  });

  test("a blocked request leaves the prerendered list exactly as it was", async ({
    page,
  }) => {
    await page.route(API, (route) => route.abort("blockedbyclient"));
    await page.goto("/roadmap/");

    const rendered = await feed(page).innerText();
    // Long enough for the effect to have run and given up several times over.
    await page.waitForTimeout(500);
    expect(await feed(page).innerText()).toBe(rendered);
  });
});

test.describe("without JavaScript", () => {
  test.use({ javaScriptEnabled: false });

  test("the section is complete in the served HTML", async ({ page }) => {
    await page.goto("/roadmap/");
    await assertSectionIsWhole(page);
  });
});

test.describe("when GitHub answers", () => {
  test("the feed shows the commits it was given", async ({ page }) => {
    await page.route(API, (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(STUB),
      }),
    );
    await page.goto("/roadmap/");

    const rows = feed(page).getByRole("listitem");
    await expect(rows).toHaveCount(STUB.length);
    await expect(rows.first()).toContainText("feat: a stubbed commit subject");
    // The message body is dropped; only the subject line is a row.
    await expect(feed(page)).not.toContainText("and a body that must not show");
    await expect(rows.first()).toContainText("1111111");
    await expect(rows.first()).toContainText("2026-08-12");
  });

  test("the permalink is built from the sha, not taken from the payload", async ({
    page,
  }) => {
    await page.route(API, (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([
          { ...STUB[0], html_url: "https://example.invalid/not-a-commit" },
        ]),
      }),
    );
    await page.goto("/roadmap/");
    await expect(feed(page).getByRole("link").first()).toHaveAttribute(
      "href",
      `https://github.com/bonzupii/cinnabar/commit/${STUB[0].sha}`,
    );
  });

  test("nothing below the feed moves when the data arrives", async ({ page }) => {
    /*
     * The reason the rows are a fixed height and the slot has a reserved one.
     * The response is held back so the page is measured before the swap and
     * again after it, rather than racing it.
     */
    await page.route(API, async (route) => {
      await new Promise((resolve) => setTimeout(resolve, 700));
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(STUB),
      });
    });

    await page.goto("/roadmap/");
    const before = (await below(page).boundingBox())!.y;

    await expect(feed(page)).toContainText("feat: a stubbed commit subject");
    const after = (await below(page).boundingBox())!.y;

    expect(after).toBe(before);
  });

  test("a second page view is served from the session cache", async ({ page }) => {
    let requests = 0;
    await page.route(API, (route) => {
      requests += 1;
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(STUB),
      });
    });

    await page.goto("/roadmap/");
    await expect(feed(page)).toContainText("feat: a stubbed commit subject");
    expect(requests).toBe(1);

    // A reader who leaves and comes back inside the TTL spends nothing more of
    // their sixty-an-hour budget.
    await page.goto("/architecture/");
    await page.goto("/roadmap/");
    await expect(feed(page)).toContainText("feat: a stubbed commit subject");
    expect(requests).toBe(1);
  });

  test("a malformed entry is dropped rather than rendered", async ({ page }) => {
    await page.route(API, (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([{ sha: "../../etc/passwd", commit: {} }, STUB[0]]),
      }),
    );
    await page.goto("/roadmap/");
    await expect(feed(page).getByRole("listitem")).toHaveCount(1);
    await assertSectionIsWhole(page);
  });
});
