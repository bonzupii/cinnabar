import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { AA_NORMAL, contrastRatio } from "@/lib/contrast";

/*
 * Pins the palette against WCAG AA, in both themes.
 *
 * The brand board fixes the colours; this file fixes which of them may carry
 * text on which surface. It is the reason the lifted --label and
 * --cinnabar-text tokens exist, and the guard that stops the darker brand
 * greys drifting back onto text.
 */

/** Every `selector { … }` rule's custom properties, read from globals.css. */
function readRules(): { selector: string; props: Record<string, string> }[] {
  const css = readFileSync(
    path.join(process.cwd(), "src", "app", "globals.css"),
    "utf8",
  );
  // Comments can contain braces and hex values; strip them before parsing.
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");

  const rules: { selector: string; props: Record<string, string> }[] = [];
  for (const match of withoutComments.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selector = match[1].replace(/\s+/g, " ").trim();
    const props: Record<string, string> = {};
    for (const prop of match[2].matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{3,8});/g)) {
      props[prop[1]] = prop[2].toLowerCase();
    }
    if (Object.keys(props).length > 0) rules.push({ selector, props });
  }
  return rules;
}

const RULES = readRules();

/** Merges every rule whose selector satisfies `matches`, in file order. */
function tokensWhere(
  label: string,
  matches: (selector: string) => boolean,
): Record<string, string> {
  const merged: Record<string, string> = {};
  const found = RULES.filter((rule) => matches(rule.selector));
  if (found.length === 0) throw new Error(`no rule found for ${label}`);
  for (const rule of found) Object.assign(merged, rule.props);
  return merged;
}

/**
 * Code keeps its own ground in both themes, so its tokens live in one rule
 * listing all three root selectors. That rule is identified by content rather
 * than by selector text, which keeps this test working if the selector list is
 * reordered.
 */
const CODE_RULE = RULES.find((rule) => "code-ground" in rule.props);
if (!CODE_RULE) throw new Error("no rule declares --code-ground");
const SHARED = CODE_RULE.props;

/** Dark is the bare `:root` default plus the explicit dark override. */
const DARK = tokensWhere(
  "the dark theme",
  (selector) => selector === ":root" || selector === ':root[data-theme="dark"]',
);
const LIGHT = tokensWhere(
  "the light theme",
  (selector) =>
    selector === ':root[data-theme="light"]' ||
    // The media-query form carries the same values for a visitor who has made
    // no explicit choice, and must not be allowed to drift from it.
    selector === ':root:not([data-theme="dark"])',
);

const THEMES = { dark: DARK, light: LIGHT } as const;

describe.each(Object.entries(THEMES))("%s theme", (themeName, T) => {
  const SURFACES = {
    ground: T.ground,
    panel: T.panel,
    "panel-raised": T["panel-raised"],
  };

  it("declares every token the site paints with", () => {
    for (const name of [
      "ground",
      "panel",
      "panel-raised",
      "hairline",
      "hairline-strong",
      "text",
      "bright",
      "secondary",
      "label",
      "cinnabar",
      "cinnabar-deep",
      "cinnabar-text",
      "on-cinnabar",
    ]) {
      expect(T[name], `${themeName} is missing --${name}`).toMatch(/^#[0-9a-f]{6}$/);
    }
  });

  for (const token of ["text", "bright", "secondary", "label"]) {
    for (const [surfaceName, surface] of Object.entries(SURFACES)) {
      it(`--${token} clears AA on ${surfaceName}`, () => {
        const ratio = contrastRatio(T[token], surface);
        expect(
          ratio,
          `${themeName}: ${T[token]} on ${surface} is ${ratio.toFixed(2)}:1`,
        ).toBeGreaterThanOrEqual(AA_NORMAL);
      });
    }
  }

  it("--cinnabar-text clears AA on every surface, including raised panels", () => {
    for (const [surfaceName, surface] of Object.entries(SURFACES)) {
      const ratio = contrastRatio(T["cinnabar-text"], surface);
      expect(
        ratio,
        `${themeName}: cinnabar-text on ${surfaceName} is ${ratio.toFixed(2)}:1`,
      ).toBeGreaterThanOrEqual(AA_NORMAL);
    }
  });

  it("--on-cinnabar is legible on the accent fill", () => {
    // Buttons and the status badge set text directly on vermilion.
    expect(contrastRatio(T["on-cinnabar"], T.cinnabar)).toBeGreaterThanOrEqual(
      AA_NORMAL,
    );
  });

  it("keeps --label quieter than --secondary, preserving the grey hierarchy", () => {
    expect(contrastRatio(T.label, T.ground)).toBeLessThan(
      contrastRatio(T.secondary, T.ground),
    );
  });
});

describe("the brand's own values are unchanged", () => {
  it("dark keeps plate 05's palette", () => {
    expect(DARK.ground).toBe("#100e0d");
    expect(DARK.panel).toBe("#171514");
    expect(DARK.hairline).toBe("#302c2a");
    expect(DARK.secondary).toBe("#a29b96");
    expect(DARK.bright).toBe("#c9c2bd");
    expect(DARK.text).toBe("#ede9e6");
    expect(DARK.cinnabar).toBe("#e0442a");
    expect(DARK["cinnabar-deep"]).toBe("#a82d1b");
  });

  it("light is built from plate 05's own light-surface set", () => {
    // "For print and paper only: #F2EEEA, #E4DED8, #C4351D, #16130F."
    expect(LIGHT.ground).toBe("#f2eeea");
    expect(LIGHT["panel-raised"]).toBe("#e4ded8");
    expect(LIGHT.cinnabar).toBe("#c4351d");
    expect(LIGHT.text).toBe("#16130f");
  });

  it("reproduces the three contrast figures plate 05 states", () => {
    expect(contrastRatio(DARK.text, DARK.ground)).toBeCloseTo(15.8, 0);
    expect(contrastRatio(DARK.secondary, DARK.ground)).toBeCloseTo(7.4, 0);
    expect(contrastRatio(DARK.cinnabar, DARK.ground)).toBeCloseTo(4.9, 0);
  });

  it("documents why the lifted tokens exist", () => {
    // The board's two darkest greys are kept, and must not carry text.
    expect(contrastRatio(DARK.mute, DARK.ground)).toBeLessThan(AA_NORMAL);
    expect(contrastRatio(DARK.grey, DARK.ground)).toBeLessThan(AA_NORMAL);
  });
});

describe("the Cinnabar Dark syntax theme", () => {
  // Code keeps its own ground in both themes, so these ratios hold everywhere.
  const surfaces = [SHARED["code-ground"], SHARED["code-terminal"]];

  it("keeps the keyword accent at plate 09's exact value", () => {
    expect(SHARED["syn-keyword"]).toBe("#e0442a");
  });

  it("keeps the code ground at the board's dark ground", () => {
    expect(SHARED["code-ground"]).toBe("#100e0d");
  });

  for (const token of [
    "syn-keyword",
    "syn-type",
    "syn-identifier",
    "syn-literal",
    "syn-punctuation",
    "syn-comment",
    "term-prompt",
    "term-command",
    "term-flag",
    "term-output",
    "term-error",
    "term-gutter",
  ]) {
    it(`--${token} clears AA on both code surfaces`, () => {
      for (const surface of surfaces) {
        const ratio = contrastRatio(SHARED[token], surface);
        expect(
          ratio,
          `${token} ${SHARED[token]} on ${surface} is ${ratio.toFixed(2)}:1`,
        ).toBeGreaterThanOrEqual(AA_NORMAL);
      }
    });
  }

  it("keeps a comment quieter than punctuation, as the board sets it", () => {
    const ground = SHARED["code-ground"];
    expect(contrastRatio(SHARED["syn-comment"], ground)).toBeLessThan(
      contrastRatio(SHARED["syn-punctuation"], ground),
    );
  });

  it("keeps the theme's tonal ladder in order", () => {
    const ground = SHARED["code-ground"];
    const ladder = [
      "syn-comment",
      "syn-punctuation",
      "syn-literal",
      "syn-identifier",
      "syn-type",
    ];
    const ratios = ladder.map((token) => contrastRatio(SHARED[token], ground));
    for (let index = 1; index < ratios.length; index += 1) {
      expect(
        ratios[index],
        `${ladder[index]} must be brighter than ${ladder[index - 1]}`,
      ).toBeGreaterThan(ratios[index - 1]);
    }
  });
});
