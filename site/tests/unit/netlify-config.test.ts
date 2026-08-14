import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

/*
 * Guards the deploy configuration.
 *
 * Next's `opengraph-image` convention writes an extension-less file, so the
 * host has nothing to infer a content type from and would serve it as a
 * generic download — which every social scraper rejects. netlify.toml restores
 * the type.
 *
 * This cannot be checked end-to-end: the Playwright run is served by `serve`,
 * which knows nothing about netlify.toml. The e2e suite therefore asserts the
 * bytes are a valid PNG, and this asserts the header rules that make the
 * deployed site serve them as one.
 */

const CONFIG = readFileSync(path.join(process.cwd(), "netlify.toml"), "utf8");

/** Returns the values block for a `[[headers]]` rule matching `forPath`. */
function headerValues(forPath: string): string | undefined {
  const rules = CONFIG.split("[[headers]]").slice(1);
  const rule = rules.find((entry) =>
    new RegExp(`for\\s*=\\s*"${forPath.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"`).test(
      entry,
    ),
  );
  return rule;
}

describe("netlify.toml", () => {
  it("publishes the static export", () => {
    expect(CONFIG).toMatch(/publish\s*=\s*"out"/);
    expect(CONFIG).toMatch(/command\s*=\s*"npm run build"/);
  });

  it("pins a Node version, so the deploy matches local builds", () => {
    expect(CONFIG).toMatch(/NODE_VERSION\s*=\s*"22"/);
  });

  it("skips the Next runtime plugin, which a static export cannot use", () => {
    /*
     * Netlify auto-installs @netlify/plugin-nextjs on any project with Next in
     * its dependencies. That runtime exists to stand up a server — SSR
     * handlers, the ISR cache, the image CDN — and this site is
     * `output: "export"`, so there is no server build for its onPostBuild step
     * to publish. It failed a real deploy with "Failed publishing static
     * content" before this was set.
     *
     * This is not optional configuration: `npm run deploy` is
     * `netlify deploy --prod`, and Netlify CLI 27 always builds before
     * publishing, so every deploy goes through the path this flag disarms.
     */
    const environment = CONFIG.split("[build.environment]")[1]?.split("\n[")[0];
    expect(environment, "no [build.environment] section").toBeDefined();
    expect(environment).toMatch(/NETLIFY_NEXT_PLUGIN_SKIP\s*=\s*"true"/);
  });

  it("serves the root social image as a PNG", () => {
    const rule = headerValues("/og-image");
    expect(rule, "no [[headers]] rule for /og-image").toBeDefined();
    expect(rule).toMatch(/Content-Type\s*=\s*"image\/png"/);
  });

  it("serves every route's social image as a PNG", () => {
    const rule = headerValues("/*/og-image");
    expect(rule, "no [[headers]] rule for /*/og-image").toBeDefined();
    expect(rule).toMatch(/Content-Type\s*=\s*"image\/png"/);
  });

  it("sends the security headers a static site should", () => {
    const rule = headerValues("/*");
    expect(rule).toMatch(/X-Content-Type-Options\s*=\s*"nosniff"/);
    expect(rule).toMatch(/Referrer-Policy/);
    expect(rule).toMatch(/X-Frame-Options/);
  });

  it("caches fingerprinted build assets immutably", () => {
    const rule = headerValues("/_next/static/*");
    expect(rule).toMatch(/immutable/);
  });

  it("does not configure a build hook, since deploys are manual", () => {
    // A [[plugins]] or build hook here would reintroduce deploy-on-push.
    expect(CONFIG).not.toMatch(/\[\[plugins\]\]/);
  });
});
