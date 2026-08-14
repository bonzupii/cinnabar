import { describe, expect, it } from "vitest";
import { linkRepoFile, rewriteRepoLinks, stripLeadingHeading } from "@/lib/repo-docs";

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

describe("linkRepoFile", () => {
  it("links the named file where it appears as inline code", () => {
    expect(linkRepoFile("Rendered from `MANIFESTO.md` at build time.", "MANIFESTO.md")).toBe(
      "Rendered from [MANIFESTO.md](https://github.com/bonzupii/cinnabar/blob/main/MANIFESTO.md) at build time.",
    );
  });

  it("leaves other inline code in the sentence alone", () => {
    expect(
      linkRepoFile("`ROADMAP.md` is rendered by `readRepoDoc`.", "ROADMAP.md"),
    ).toBe(
      "[ROADMAP.md](https://github.com/bonzupii/cinnabar/blob/main/ROADMAP.md) is rendered by `readRepoDoc`.",
    );
  });

  it("is a no-op when the file is not mentioned", () => {
    expect(linkRepoFile("No filename here.", "MANIFESTO.md")).toBe("No filename here.");
  });

  it("links only the first mention, which is where the reference belongs", () => {
    const linked = linkRepoFile("`A.md` and `A.md`", "A.md");
    expect(linked.match(/https:/g)).toHaveLength(1);
  });
});
