import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 2 : undefined,
  reporter: process.env.CI ? [["html", { open: "never" }], ["github"]] : "html",
  expect: {
    toHaveScreenshot: {
      // A small per-pixel tolerance absorbs font antialiasing differences
      // between machines without hiding a real visual regression.
      maxDiffPixelRatio: 0.02,
      animations: "disabled",
    },
  },
  use: {
    baseURL: "http://localhost:4173",
    trace: "on-first-retry",
    /*
     * The brand's default presentation is dark, so that is what the suite
     * exercises unless a spec says otherwise. A browser always reports some
     * colour-scheme preference — Playwright's own default is light — so this
     * has to be stated rather than left implicit.
     */
    colorScheme: "dark",
    /*
     * Entrance reveals are driven by an intersection observer, so an element
     * below the fold sits at opacity 0 until it is scrolled to. That makes
     * both screenshots and contrast checks a race. Reduced motion renders
     * every reveal in its final state, which is also the behaviour a visitor
     * with the OS setting gets. tests/e2e/motion.spec.ts covers the animated
     * path explicitly, via page.emulateMedia.
     *
     * Set through contextOptions because `reducedMotion` is not declared on
     * the top-level test-options type in this Playwright version.
     */
    contextOptions: { reducedMotion: "reduce" },
  },
  webServer: {
    // No -s/--single: that rewrites every 404 to index.html and would mask a
    // genuinely broken route. `trailingSlash: true` means each route is its
    // own directory with an index.html, which `serve` resolves on its own.
    command: "npx serve out -l 4173",
    url: "http://localhost:4173",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 900 } },
    },
  ],
});
