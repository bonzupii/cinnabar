import { expect, test } from "@playwright/test";
import { ROUTES } from "./routes";

/*
 * Deployment-shape checks.
 *
 * The site ships as a static export, so whatever the build wrote is exactly
 * what gets served. The social images in particular are route handlers under
 * /og/<name>.png rather than Next's `opengraph-image` convention, because that
 * convention emits an extension-less file, which a static host serves without
 * an image content-type and every social scraper then rejects.
 */

const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

for (const route of ROUTES) {
  test(`${route.name} social image is a real 1200x630 PNG`, async ({ request }) => {
    const response = await request.get(route.og);
    expect(response.status()).toBe(200);

    // The content type is asserted in tests/unit/netlify-config.test.ts, not
    // here: Next's file convention writes an extension-less file and the local
    // `serve` has no netlify.toml to read, so only the deployed host sets it.
    const body = await response.body();
    expect(body.subarray(0, 8)).toEqual(PNG_SIGNATURE);
    expect(body.readUInt32BE(16)).toBe(1200);
    expect(body.readUInt32BE(20)).toBe(630);
  });
}

test("every page declares a social image with its dimensions", async ({ page }) => {
  for (const route of ROUTES) {
    await page.goto(route.path);

    const ogImage = await page
      .locator('meta[property="og:image"]')
      .getAttribute("content");
    expect(ogImage, `${route.name} has no og:image`).toContain(`${route.og}?`);

    await expect(page.locator('meta[property="og:image:width"]')).toHaveAttribute(
      "content",
      "1200",
    );
    await expect(page.locator('meta[property="og:image:height"]')).toHaveAttribute(
      "content",
      "630",
    );
    await expect(page.locator('meta[name="twitter:card"]')).toHaveAttribute(
      "content",
      "summary_large_image",
    );
  }
});

test("every page has a title, description and canonical link", async ({ page }) => {
  for (const route of ROUTES) {
    await page.goto(route.path);
    await expect(page).toHaveTitle(/Cinnabar/);

    const description = await page
      .locator('meta[name="description"]')
      .getAttribute("content");
    expect(description?.length ?? 0).toBeGreaterThan(50);

    const canonical = await page.locator('link[rel="canonical"]').getAttribute("href");
    expect(canonical, `${route.name} canonical`).toContain(route.path);
  }
});

test("the favicon is an SVG served with the right type", async ({ page, request }) => {
  await page.goto("/");
  const href = await page.locator('link[rel="icon"]').first().getAttribute("href");
  expect(href).toMatch(/\.svg/);

  const response = await request.get(href!);
  expect(response.status()).toBe(200);
  expect(response.headers()["content-type"]).toContain("image/svg+xml");
});

test("the sitemap lists every route", async ({ request }) => {
  const response = await request.get("/sitemap.xml");
  expect(response.status()).toBe(200);
  const xml = await response.text();

  for (const route of ROUTES) {
    expect(xml, `sitemap is missing ${route.path}`).toContain(`${route.path}</loc>`);
  }
});

test("robots.txt points at the sitemap", async ({ request }) => {
  const response = await request.get("/robots.txt");
  expect(response.status()).toBe(200);
  const body = await response.text();
  expect(body).toContain("Allow: /");
  expect(body).toContain("sitemap.xml");
});

