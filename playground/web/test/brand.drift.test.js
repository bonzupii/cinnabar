// The playground's palette against the site's.
//
// `src/brand.js` restates colours the site already defines, because Monaco
// needs literal hex in JavaScript and cannot read a CSS custom property.
// That copy can drift, so it is checked here rather than trusted — the same
// discipline `packages/cinnabar-monaco` applies to the compiler's KEYWORDS
// table.
//
// This asserts in one direction on purpose: every value the playground uses
// must exist in the site's stylesheet with the same value. The site may
// define tokens the playground has no use for.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import { SURFACE, SYNTAX, TERMINAL } from "../src/brand.js";
import { PORTED_GEOMETRY, MARK_BLOCK } from "../src/brandGeometry.js";

const here = dirname(fileURLToPath(import.meta.url));
const stylesheet = join(here, "..", "..", "..", "site", "src", "app", "globals.css");

/**
 * Every `--name: #value` declaration in the site's stylesheet.
 *
 * The site defines the same token twice — once for dark, once inside the
 * light media query — so a name maps to the set of values it is given, and
 * a match against any of them counts. The playground only ever uses the
 * dark set, but pinning to "the first occurrence" would make this test
 * depend on declaration order rather than on the palette.
 */
function declaredColours(css) {
  const declarations = new Map();
  for (const match of css.matchAll(/--([a-z0-9-]+)\s*:\s*(#[0-9a-fA-F]{3,8})\s*;/g)) {
    const [, name, value] = match;
    const values = declarations.get(name) ?? new Set();
    values.add(value.toLowerCase());
    declarations.set(name, values);
  }
  return declarations;
}

const declared = declaredColours(readFileSync(stylesheet, "utf8"));

// Each entry: the playground's key, and the site token it copies.
const MAPPING = [
  [SURFACE, "ground", "ground"],
  [SURFACE, "panel", "panel"],
  [SURFACE, "panelRaised", "panel-raised"],
  [SURFACE, "hairline", "hairline"],
  [SURFACE, "hairlineStrong", "hairline-strong"],
  [SURFACE, "mute", "mute"],
  [SURFACE, "grey", "grey"],
  [SURFACE, "secondary", "secondary"],
  [SURFACE, "bright", "bright"],
  [SURFACE, "text", "text"],
  [SURFACE, "label", "label"],
  [SURFACE, "cinnabar", "cinnabar"],
  [SURFACE, "cinnabarDeep", "cinnabar-deep"],
  [SURFACE, "cinnabarText", "cinnabar-text"],
  [SURFACE, "onCinnabar", "on-cinnabar"],
  [SYNTAX, "ground", "code-ground"],
  [SYNTAX, "terminal", "code-terminal"],
  [SYNTAX, "keyword", "syn-keyword"],
  [SYNTAX, "type", "syn-type"],
  [SYNTAX, "identifier", "syn-identifier"],
  [SYNTAX, "literal", "syn-literal"],
  [SYNTAX, "punctuation", "syn-punctuation"],
  [SYNTAX, "comment", "syn-comment"],
  [TERMINAL, "prompt", "term-prompt"],
  [TERMINAL, "command", "term-command"],
  [TERMINAL, "flag", "term-flag"],
  [TERMINAL, "output", "term-output"],
  [TERMINAL, "error", "term-error"],
  [TERMINAL, "gutter", "term-gutter"],
];

test("the site's stylesheet was found and parsed", () => {
  assert.ok(declared.size > 20, `only ${declared.size} tokens parsed from ${stylesheet}`);
});

test("every playground colour is a site token with the same value", () => {
  for (const [group, key, token] of MAPPING) {
    const values = declared.get(token);
    assert.ok(values, `site defines no --${token}`);
    assert.ok(
      values.has(group[key].toLowerCase()),
      `--${token} is ${[...values].join(" / ")} on the site, but ${group[key]} here`,
    );
  }
});

/*
 * The mark and the icon set.
 *
 * `CinnabarMark`'s own source says the board's path data must not be
 * redrawn, so the port is held to that literally: every coordinate string
 * must still be findable in the site's components.
 */
const brandDir = join(here, "..", "..", "..", "site", "src", "components", "brand");
const brandSource =
  readFileSync(join(brandDir, "CinnabarMark.tsx"), "utf8") + readFileSync(join(brandDir, "icons.tsx"), "utf8");

test("every ported figure is the board's own geometry", () => {
  for (const geometry of PORTED_GEOMETRY) {
    assert.ok(
      brandSource.includes(geometry),
      `"${geometry}" is drawn here but does not appear in the site's brand components`,
    );
  }
});

test("the mark's block is the board's block", () => {
  for (const [key, value] of Object.entries(MARK_BLOCK)) {
    assert.match(
      brandSource,
      new RegExp(`${key}:\\s*${value}\\b`),
      `MARK_BLOCK.${key} = ${value} does not match the site`,
    );
  }
});

test("the palette adds no colour the site does not define", () => {
  const mapped = new Set(MAPPING.map(([group, key]) => group[key].toLowerCase()));
  for (const group of [SURFACE, SYNTAX, TERMINAL]) {
    for (const value of Object.values(group)) {
      assert.ok(
        mapped.has(value.toLowerCase()),
        `${value} is used here but is not mapped to a site token`,
      );
    }
  }
});
