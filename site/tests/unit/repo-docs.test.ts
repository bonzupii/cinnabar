import { describe, expect, it } from "vitest";
import { rewriteRepoLinks, stripLeadingHeading } from "@/lib/repo-docs";

describe("rewriteRepoLinks", () => {
  it("points documents that have their own page at that page", () => {
    expect(rewriteRepoLinks("see [`MANIFESTO.md`](MANIFESTO.md)")).toBe(
      "see [`MANIFESTO.md`](/manifesto/)",
    );
    expect(rewriteRepoLinks("[roadmap](ROADMAP.md)")).toBe("[roadmap](/roadmap/)");
    expect(rewriteRepoLinks("[arch](ARCHITECTURE.md)")).toBe("[arch](/architecture/)");
  });

  it("sends source files to the GitHub blob view", () => {
    expect(rewriteRepoLinks("[build.rs](build.rs)")).toBe(
      "[build.rs](https://github.com/bonzupii/cinnabar/blob/main/build.rs)",
    );
    expect(rewriteRepoLinks("[spec](tests/fixtures/spec.cnb)")).toBe(
      "[spec](https://github.com/bonzupii/cinnabar/blob/main/tests/fixtures/spec.cnb)",
    );
  });

  it("sends directories to the tree view instead", () => {
    expect(rewriteRepoLinks("[fixtures](tests/fixtures/)")).toBe(
      "[fixtures](https://github.com/bonzupii/cinnabar/tree/main/tests/fixtures/)",
    );
  });

  it("carries an anchor across the rewrite", () => {
    expect(rewriteRepoLinks("[cli](README.md#using-the-compiler)")).toBe(
      "[cli](https://github.com/bonzupii/cinnabar/blob/main/README.md#using-the-compiler)",
    );
    expect(rewriteRepoLinks("[p7](MANIFESTO.md#7-linear-types)")).toBe(
      "[p7](/manifesto/#7-linear-types)",
    );
  });

  it("leaves absolute links, anchors and mailto alone", () => {
    const untouched = [
      "[ariadne](https://github.com/zesterer/ariadne)",
      "[http](http://example.com)",
      "[section](#core-principles)",
      "[mail](mailto:someone@example.com)",
    ].join("\n");
    expect(rewriteRepoLinks(untouched)).toBe(untouched);
  });

  it("normalises a leading ./", () => {
    expect(rewriteRepoLinks("[gate](./pre_commit_check.sh)")).toBe(
      "[gate](https://github.com/bonzupii/cinnabar/blob/main/pre_commit_check.sh)",
    );
  });

  it("preserves a link title", () => {
    expect(rewriteRepoLinks('[flake](flake.nix "the dev shell")')).toBe(
      '[flake](https://github.com/bonzupii/cinnabar/blob/main/flake.nix "the dev shell")',
    );
  });

  it("does not mistake an image or inline code for a link", () => {
    expect(rewriteRepoLinks("`](MANIFESTO.md)` stays literal")).toBe(
      "`](/manifesto/)` stays literal",
    );
  });
});

describe("stripLeadingHeading", () => {
  it("removes the document title so the page owns its own h1", () => {
    expect(stripLeadingHeading("# Cinnabar — Roadmap\n\nThe language spec is...")).toBe(
      "The language spec is...",
    );
  });

  it("leaves a document that does not start with a heading untouched", () => {
    expect(stripLeadingHeading("Intro paragraph\n\n# Later heading\n")).toBe(
      "Intro paragraph\n\n# Later heading\n",
    );
  });

  it("keeps every later heading", () => {
    const stripped = stripLeadingHeading("# Title\n\n## Core Principles\n\nBody\n");
    expect(stripped).toBe("## Core Principles\n\nBody\n");
  });
});
