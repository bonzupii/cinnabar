import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { parseContentBlocks, parseContentItems } from "@/lib/page-content";
import { HIGHLIGHT_ICONS } from "@/content/highlights";
import { CLI_SECTIONS } from "@/content/cli";
import { ARENAS, STAGES } from "@/content/pipeline";
import { HORIZON, IN_PROGRESS, SHIPPED } from "@/content/roadmap";
import { SAMPLES } from "@/content/samples";

/*
 * The copy lives in each route's content.md; the structure that pairs it with
 * an icon, an anchor or an order lives in src/content/*.ts. The two halves are
 * bound by name, and a name that does not resolve throws when the page is
 * rendered — which is a build failure, but only for whoever runs a build.
 *
 * These checks make the same mistake fail in `npm test`, in a second, naming
 * the block and the key rather than the component that happened to read it.
 */

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

function blocks(route: string): Record<string, string> {
  return parseContentBlocks(
    readFileSync(path.join(ROOT, "src", "app", route, "content.md"), "utf8"),
  );
}

function slugs(route: string, block: string): string[] {
  const source = blocks(route)[block];
  expect(source, `@${block} is missing from ${route}/content.md`).toBeDefined();
  return parseContentItems(source).map((item) => item.slug);
}

describe("the home route", () => {
  it("binds an icon to every promise, and no icon to a promise that is gone", () => {
    expect(slugs("(home)", "promises").sort()).toEqual(
      Object.keys(HIGHLIGHT_ICONS).sort(),
    );
  });

  it("has a summary block for every code sample", () => {
    const home = blocks("(home)");
    for (const sample of SAMPLES) {
      expect(home[`sample-${sample.id}`], `@sample-${sample.id}`).toBeTruthy();
    }
  });
});

describe("the architecture route", () => {
  const architecture = blocks("architecture");

  it("has a summary block for every pipeline stage", () => {
    for (const stage of STAGES) {
      expect(architecture[`stage-${stage.slug}`], `@stage-${stage.slug}`).toBeTruthy();
    }
  });

  it("has a description block for every arena", () => {
    for (const arena of ARENAS) {
      expect(architecture[`arena-${arena.name}`], `@arena-${arena.name}`).toBeTruthy();
    }
  });

  it("carries the prose the page reads directly", () => {
    for (const name of ["arena-properties", "single-fact-rule", "stages-halt"]) {
      expect(architecture[name], `@${name}`).toBeTruthy();
    }
  });
});

describe("the roadmap route", () => {
  it("has a section for every capability, and no orphan section", () => {
    const listed = [...SHIPPED, ...IN_PROGRESS].map((item) => item.slug);
    expect(slugs("roadmap", "capabilities").sort()).toEqual([...listed].sort());
  });

  it("has the horizon the page links to", () => {
    expect(slugs("roadmap", "horizon")).toEqual([HORIZON.slug]);
  });
});

describe("the reference route", () => {
  const reference = blocks("reference");

  it("has a heading, a note and rows for every CLI section", () => {
    for (const section of CLI_SECTIONS) {
      expect(reference[`${section.id}-heading`], `@${section.id}-heading`).toBeTruthy();
      expect(reference[`${section.id}-note`], `@${section.id}-note`).toBeTruthy();
      expect(
        parseContentItems(reference[`${section.id}-rows`] ?? "").length,
        `@${section.id}-rows`,
      ).toBeGreaterThan(0);
    }
  });

  it("gives every table row a name and a description", () => {
    const tables = [
      ...CLI_SECTIONS.map((section) => `${section.id}-rows`),
      "test-layout-rows",
      "test-env-rows",
    ];
    for (const table of tables) {
      for (const row of parseContentItems(reference[table] ?? "")) {
        expect(row.title.length, `${table}: empty name`).toBeGreaterThan(0);
        expect(row.body.length, `${table}: ${row.title} has no description`).
          toBeGreaterThan(0);
      }
    }
  });
});
