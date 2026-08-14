import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/*
 * No arbitrary value may spell a registered theme token the long way.
 *
 * `globals.css` registers the palette in an `@theme inline` block, so
 * `--color-hairline-strong: var(--hairline-strong)` makes `bg-hairline-strong`
 * emit exactly what `bg-[color:var(--hairline-strong)]` emits. The long form
 * keeps coming back — it was reported, fixed, and reintroduced the next time
 * the window controls were rewritten — so it is checked here rather than left
 * to an editor hint nobody's build reads.
 *
 * This complements the ESLint rule rather than duplicating it.
 * `better-tailwindcss/enforce-canonical-classes` implements Tailwind's own
 * canonical suggestions, which catch an arbitrary value equal to a scale value
 * (`tracking-[-0.025em]` for `tracking-tight`) and a negative offset written
 * long-hand (`outline-offset-[-2px]` for `-outline-offset-2`). What it does
 * not do is resolve a token that aliases another custom property: it rewrites
 * `bg-[color:var(--hairline-strong)]` to `bg-(--hairline-strong)`, which is
 * shorter but still goes around the token. It also only sees classes in a
 * `className`, and this project keeps some class strings in named constants.
 * Both gaps are this file's subject.
 *
 * The token table is parsed out of `globals.css`, never listed here, so a
 * token added to the theme is covered without this test changing.
 */

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const GLOBALS = path.join(ROOT, "src", "app", "globals.css");

/** `--color-hairline-strong: var(--hairline-strong)` inside `@theme inline`. */
const THEME_INLINE = /@theme\s+inline\s*\{([\s\S]*?)\n\}/;
const ALIAS = /^\s*--([\w-]+)\s*:\s*var\((--[\w-]+)\)\s*;/gm;

/**
 * Every custom property a registered theme token aliases, mapped to the
 * utility name that token provides.
 *
 * Tailwind's namespaces are stripped so the suggestion reads as a class:
 * `--color-hairline-strong` provides `bg-hairline-strong`, `text-…`,
 * `border-…`; `--font-mono` provides `font-mono`.
 */
const NAMESPACES = [
  "color",
  "font",
  "text",
  "spacing",
  "breakpoint",
  "radius",
  "shadow",
  "tracking",
  "leading",
];

export function readThemeAliases(css: string): Map<string, string> {
  const block = THEME_INLINE.exec(css);
  if (!block) throw new Error("globals.css has no `@theme inline` block");

  const aliases = new Map<string, string>();
  for (const [, token, aliased] of block[1].matchAll(ALIAS)) {
    const namespace = NAMESPACES.find((name) => token.startsWith(`${name}-`));
    aliases.set(aliased, namespace ? token.slice(namespace.length + 1) : token);
  }
  return aliases;
}

/**
 * An arbitrary value referencing a custom property, in either spelling
 * Tailwind accepts: `text-[color:var(--bright)]` and `text-(--bright)`.
 *
 * The utility is whatever precedes the bracket after the last variant colon,
 * so `[&_code]:text-[color:var(--bright)]` reports `text`.
 */
const ARBITRARY =
  /([a-z][\w-]*)-(?:\[(?:[a-z-]+:)?var\((--[\w-]+)\)\]|\((--[\w-]+)\))/g;

export type Finding = { klass: string; suggestion: string };

/** Every place `source` spells a registered token as an arbitrary value. */
export function findTokenBypasses(
  source: string,
  aliases: Map<string, string>,
): Finding[] {
  const findings: Finding[] = [];
  for (const match of source.matchAll(ARBITRARY)) {
    const [klass, utility] = match;
    const variable = match[2] ?? match[3];
    const token = aliases.get(variable);
    if (token) findings.push({ klass, suggestion: `${utility}-${token}` });
  }
  return findings;
}

const aliases = readThemeAliases(readFileSync(GLOBALS, "utf8"));

describe("the theme token table", () => {
  it("is read from globals.css rather than listed in this test", () => {
    // A sample, not the whole table: the point is that parsing worked.
    expect(aliases.get("--hairline-strong")).toBe("hairline-strong");
    expect(aliases.get("--grey")).toBe("grey");
    expect(aliases.get("--bright")).toBe("bright");
    expect(aliases.size).toBeGreaterThan(20);
  });
});

describe("the check itself", () => {
  it("fails on the long-hand forms it exists to catch", () => {
    const source = `
      <span className="bg-[color:var(--hairline-strong)]" />
      <span className="bg-[color:var(--grey)]" />
      <span className="[&_code]:text-[color:var(--bright)]" />
      <span className="bg-(--hairline-strong)" />
    `;
    expect(findTokenBypasses(source, aliases).map((f) => f.suggestion)).toEqual([
      "bg-hairline-strong",
      "bg-grey",
      "text-bright",
      "bg-hairline-strong",
    ]);
  });

  it("leaves a variable that is not a registered token alone", () => {
    // --window-shadow is deliberately outside the token table.
    const source = `<div className="shadow-[var(--window-shadow)]" />`;
    expect(findTokenBypasses(source, aliases)).toEqual([]);
  });
});

/** Every .ts/.tsx file under src/, relative to the project root. */
function sourceFiles(dir: string): string[] {
  return readdirSync(path.join(ROOT, dir), { withFileTypes: true }).flatMap(
    (entry) => {
      const relative = `${dir}/${entry.name}`;
      if (entry.isDirectory()) return sourceFiles(relative);
      return /\.tsx?$/.test(entry.name) ? [relative] : [];
    },
  );
}

describe("the source tree", () => {
  const files = sourceFiles("src");

  it("has files to check", () => {
    expect(files.length).toBeGreaterThan(10);
  });

  it("uses theme tokens rather than arbitrary values", () => {
    const offences = files.flatMap((file) =>
      findTokenBypasses(readFileSync(path.join(ROOT, file), "utf8"), aliases).map(
        (finding) => `${file}: ${finding.klass} → ${finding.suggestion}`,
      ),
    );
    expect(offences).toEqual([]);
  });
});
