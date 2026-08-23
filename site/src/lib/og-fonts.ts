import { packageFontLoader, type LoadedFont } from "metaplate/fonts";

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

const FONTS = [
  {
    name: "Schibsted Grotesk",
    package: "@fontsource/schibsted-grotesk",
    file: "files/schibsted-grotesk-latin-800-normal.woff",
    weight: 800,
  },
  {
    name: "Schibsted Grotesk",
    package: "@fontsource/schibsted-grotesk",
    file: "files/schibsted-grotesk-latin-700-normal.woff",
    weight: 700,
  },
  {
    name: "Schibsted Grotesk",
    package: "@fontsource/schibsted-grotesk",
    file: "files/schibsted-grotesk-latin-400-normal.woff",
    weight: 400,
  },
  {
    name: "IBM Plex Mono",
    package: "@fontsource/ibm-plex-mono",
    file: "files/ibm-plex-mono-latin-500-normal.woff",
    weight: 500,
  },
] as const;

export type OgFont = LoadedFont;

/** Loads the four faces the social images set, in Satori's expected shape. */
export const loadOgFonts = packageFontLoader(FONTS);
