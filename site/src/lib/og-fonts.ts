import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import path from "node:path";

/*
 * Fonts for the social images.
 *
 * Satori needs real font bytes, which the browser-side `next/font` pipeline
 * does not expose. They come from @fontsource rather than from files committed
 * here: a vendored binary is one more thing to keep in step with the typefaces
 * the pages actually render, and Schibsted Grotesk ships upstream only as a
 * variable font, which Satori resolves to its default 400 instance — so a
 * vendored copy also meant cutting static instances by hand before it could be
 * used at all. @fontsource publishes the static weights directly.
 *
 * .woff, not .woff2: Satori reads ttf, otf and woff, and @fontsource ships
 * both — so this picks the one that works rather than the smaller one, which
 * would fail at build time.
 *
 * The files are found by walking up from the working directory rather than
 * with `require.resolve`, because the bundler owns that function: pointed at a
 * .woff it fails the build with "Unknown module type", and pointed at the
 * package's package.json it returns an internal module id rather than a path.
 * Walking node_modules is invisible to it, and handles a dependency hoisted to
 * a workspace root as well as a local one.
 */

/** Finds an installed package's directory, or throws saying where it looked. */
function packageDir(name: string): string {
  const searched: string[] = [];
  let directory = process.cwd();

  for (;;) {
    const candidate = path.join(directory, "node_modules", name);
    searched.push(candidate);
    if (existsSync(candidate)) return candidate;

    const parent = path.dirname(directory);
    if (parent === directory) break;
    directory = parent;
  }

  throw new Error(
    `cannot find ${name}; the social images need it for their fonts. Looked in:\n  ${searched.join("\n  ")}`,
  );
}

const FONTS = [
  {
    name: "Schibsted Grotesk",
    package: "@fontsource/schibsted-grotesk",
    file: "schibsted-grotesk-latin-800-normal.woff",
    weight: 800,
  },
  {
    name: "Schibsted Grotesk",
    package: "@fontsource/schibsted-grotesk",
    file: "schibsted-grotesk-latin-700-normal.woff",
    weight: 700,
  },
  {
    name: "Schibsted Grotesk",
    package: "@fontsource/schibsted-grotesk",
    file: "schibsted-grotesk-latin-400-normal.woff",
    weight: 400,
  },
  {
    name: "IBM Plex Mono",
    package: "@fontsource/ibm-plex-mono",
    file: "ibm-plex-mono-latin-500-normal.woff",
    weight: 500,
  },
] as const;

/** The subset of CSS weights these faces ship, matching Satori's own union. */
type FontWeight = 400 | 500 | 700 | 800;

export type OgFont = {
  name: string;
  data: Buffer;
  weight: FontWeight;
  style: "normal";
};

/** Loads the four faces the social images set, in Satori's expected shape. */
export async function loadOgFonts(): Promise<OgFont[]> {
  return Promise.all(
    FONTS.map(async (font) => ({
      name: font.name,
      data: await readFile(path.join(packageDir(font.package), "files", font.file)),
      weight: font.weight as FontWeight,
      style: "normal" as const,
    })),
  );
}
