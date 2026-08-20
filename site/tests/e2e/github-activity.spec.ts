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
 * nothing below it moves when data lands — nor when the reader filters it.
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

/**
 * The height the section holds in every state — `SLOT` in ActivityFeed.tsx.
 *
 * Asserted rather than merely reserved because the whole design of the section
 * rests on it: the summary, the filter and the log are each a fixed height so
 * that nothing moves when fresh data replaces the prerendered list, and the
 * fallback pads to the same figure so nothing moves when a build that got
 * nothing is followed by a browser that got something.
 */
const SECTION_HEIGHT = 612;

const STUB = [
  {
    sha: "1111111111111111111111111111111111111111",
    commit: {
      message: "resolver: a stubbed commit subject\n\nand a body that must not show",
      author: { date: "2026-08-12T09:00:00Z" },
    },
  },
  {
    sha: "2222222222222222222222222222222222222222",
    commit: {
      message: "codegen: a second stubbed subject",
      author: { date: "2026-08-11T09:00:00Z" },
    },
  },
  {
    sha: "3333333333333333333333333333333333333333",
    commit: {
      message: "resolver, codegen: a third, touching two areas",
      author: { date: "2026-08-11T08:00:00Z" },
    },
  },
];

/** The feed in whichever state it is in — the assembled feed or the fallback. */
function feed(page: Page) {
  return page.locator("[data-activity]");
}

/** The commit rows, which are not the only list items in the log. */
function rows(page: Page) {
  return feed(page).locator("[data-commit]");
}

/** One area's filter chip. */
function chip(page: Page, name: string) {
  return feed(page).getByRole("button", { name: new RegExp(`^${name}\\b`) });
}

/** The section heading below the feed, for measuring whether anything moved. */
function below(page: Page) {
  return page.getByRole("heading", { name: "The full record" });
}

async function height(page: Page) {
  return feed(page).evaluate((element) => element.getBoundingClientRect().height);
}

/**
 * Where the heading below the feed sits in the document.
 *
 * Document coordinates rather than viewport ones: clicking a filter chip
 * scrolls it into view, which moves everything in the viewport without
 * anything having moved on the page.
 */
async function documentY(page: Page) {
  return below(page).evaluate(
    (element) => element.getBoundingClientRect().y + window.scrollY,
  );
}

async function assertSectionIsWhole(page: Page) {
  // The heading, the slot and the link are all present whatever happened.
  await expect(page.getByRole("heading", { name: "Recent activity" })).toBeVisible();
  await expect(feed(page)).toHaveCount(1);
  await expect(
    page.getByRole("link", { name: /the full commit log/i }),
  ).toHaveAttribute("href", "https://github.com/bonzupii/cinnabar/commits/main/");

  // The slot is the same reserved height in both states, so the section can
  // never collapse to a sliver while it waits for something.
  expect(await height(page)).toBeGreaterThanOrEqual(SECTION_HEIGHT);

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

  test("the summary and the log are prerendered, not assembled on mount", async ({
    page,
  }) => {
    // The figures and the day headers are derived from the commits, and a
    // derivation that only runs in the browser is a section that is blank for
    // a crawler and for a reader with scripts off.
    await page.goto("/roadmap/");
    await expect(feed(page).getByText("Commits", { exact: true })).toBeVisible();
    await expect(feed(page).getByText("Areas touched")).toBeVisible();
    await expect(rows(page).first()).toBeVisible();
  });
});

test.describe("when GitHub answers", () => {
  /** Serves the stub, and waits for it to have replaced the prerendered list. */
  async function withStub(page: Page, body: unknown = STUB) {
    await page.route(API, (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(body),
      }),
    );
    await page.goto("/roadmap/");
    await expect(feed(page)).toContainText("a stubbed commit subject");
  }

  test("the feed shows the commits it was given", async ({ page }) => {
    await withStub(page);

    await expect(rows(page)).toHaveCount(STUB.length);
    await expect(rows(page).first()).toContainText("a stubbed commit subject");
    // The message body is dropped; only the subject line is a row.
    await expect(feed(page)).not.toContainText("and a body that must not show");
    await expect(rows(page).first()).toContainText("1111111");
  });

  test("the area is lifted out of the subject and into its own column", async ({
    page,
  }) => {
    await withStub(page);

    const first = rows(page).first();
    await expect(first).toContainText("resolver");
    // The prefix is not repeated in the title beside it.
    await expect(first).not.toContainText("resolver:");
    // The whole subject survives as the row's tooltip.
    await expect(first.locator("[title]")).toHaveAttribute(
      "title",
      "resolver: a stubbed commit subject",
    );
  });

  test("the commits are grouped under the day they landed", async ({ page }) => {
    await withStub(page);
    await expect(feed(page)).toContainText("2026-08-12");
    await expect(feed(page)).toContainText("1 commit");
    await expect(feed(page)).toContainText("2026-08-11");
    await expect(feed(page)).toContainText("2 commits");
  });

  test("the summary counts the window it is showing", async ({ page }) => {
    await withStub(page);

    const summary = feed(page).locator(".rule-grid").first();
    await expect(summary).toContainText("3");
    await expect(summary).toContainText("Commits");
    await expect(summary).toContainText("Active days");
    // Two areas across three commits, one of which names both.
    await expect(summary).toContainText("Areas touched");
  });

  test("the permalink is built from the sha, not taken from the payload", async ({
    page,
  }) => {
    await withStub(page, [
      { ...STUB[0], html_url: "https://example.invalid/not-a-commit" },
    ]);
    await expect(rows(page).first()).toHaveAttribute(
      "href",
      `https://github.com/bonzupii/cinnabar/commit/${STUB[0].sha}`,
    );
  });

  test("nothing below the feed moves when the data arrives", async ({ page }) => {
    /*
     * The reason every part of the section is a fixed height. The response is
     * held back so the page is measured before the swap and again after it,
     * rather than racing it.
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

    await expect(feed(page)).toContainText("a stubbed commit subject");
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
    await expect(feed(page)).toContainText("a stubbed commit subject");
    expect(requests).toBe(1);

    // A reader who leaves and comes back inside the TTL spends nothing more of
    // their sixty-an-hour budget.
    await page.goto("/architecture/");
    await page.goto("/roadmap/");
    await expect(feed(page)).toContainText("a stubbed commit subject");
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
    await expect(feed(page)).toContainText("a stubbed commit subject");
    await expect(rows(page)).toHaveCount(1);
    await assertSectionIsWhole(page);
  });
});

test.describe("filtering by area", () => {
  test.beforeEach(async ({ page }) => {
    await page.route(API, (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(STUB),
      }),
    );
    await page.goto("/roadmap/");
    await expect(page.locator("[data-activity]")).toContainText(
      "a stubbed commit subject",
    );
  });

  test("a chip narrows the log to the commits touching that area", async ({
    page,
  }) => {
    // The third stub names both areas, so it is an answer to either question.
    await expect(chip(page, "resolver")).toHaveText(/resolver\s*2/);
    await chip(page, "resolver").click();

    await expect(chip(page, "resolver")).toHaveAttribute("aria-pressed", "true");
    await expect(rows(page)).toHaveCount(2);
    await expect(feed(page)).not.toContainText("a second stubbed subject");
  });

  test("the summary follows the filter rather than the window", async ({ page }) => {
    await chip(page, "codegen").click();
    const summary = feed(page).locator(".rule-grid").first();
    // Two codegen commits, both on the same day, one area between them.
    await expect(summary.locator("div").first()).toContainText("2");
  });

  test("filtering moves nothing on the page", async ({ page }) => {
    const before = await documentY(page);
    const tall = await height(page);

    await chip(page, "codegen").click();
    await expect(rows(page)).toHaveCount(2);

    expect(await height(page)).toBe(tall);
    expect(await documentY(page)).toBe(before);
  });

  test("pressing the same chip again clears the filter", async ({ page }) => {
    await chip(page, "codegen").click();
    await chip(page, "codegen").click();
    await expect(rows(page)).toHaveCount(STUB.length);
    await expect(
      feed(page).getByRole("button", { name: /^All/ }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  test("the log is reachable and scrollable from the keyboard", async ({ page }) => {
    // A region that scrolls has to be focusable, or a reader without a mouse
    // cannot reach the commits below the fold of the frame.
    const log = feed(page).getByRole("list", { name: /recent commits/i });
    await log.focus();
    await expect(log).toBeFocused();
  });
});
