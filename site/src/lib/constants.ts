/*
 * Values that were previously spelled out at each use site.
 *
 * Colours live in globals.css as CSS custom properties and are referenced
 * through Tailwind tokens everywhere they can be. The hexes below exist only
 * for the two places CSS cannot reach: Satori, which renders the social images
 * outside a browser and so has no custom properties, and the swatches in the
 * diagnostic legend, which have to state the literal value they name.
 *
 * Keeping those in one table means the brand palette has exactly two
 * definitions — this file and globals.css — and tests/unit/palette.test.ts
 * checks the CSS side against the board's own figures.
 */

/** Plate 05's palette, for contexts with no access to CSS variables. */
export const BRAND = {
  ground: "#100E0D",
  panel: "#171514",
  hairline: "#302C2A",
  mute: "#6E6763",
  grey: "#7C7570",
  secondary: "#A29B96",
  bright: "#C9C2BD",
  text: "#EDE9E6",
  cinnabar: "#E0442A",
  cinnabarDeep: "#A82D1B",
  terminal: "#0B0A09",
} as const;

/* ---------------------------------------------------------------- layout -- */

/** The page gutter and maximum content width, shared by every section. */
export const CONTAINER = "mx-auto max-w-[1400px] px-6 sm:px-10";

/** Vertical rhythm between major sections. */
export const SECTION_PADDING = "py-24";

/**
 * Line lengths.
 *
 * `measure` is the comfortable reading width for body copy; `wide` is for
 * text that sits beside a figure and can run longer without becoming hard to
 * track.
 */
export const MEASURE = {
  measure: "max-w-[86ch]",
  wide: "max-w-[90ch]",
  lede: "max-w-[80ch]",
} as const;

/**
 * Two-column splits, always with clamped tracks.
 *
 * A grid track defaults to `min-width: auto`, so a wide code block inside one
 * stretches the track instead of scrolling. Every split here clamps both.
 */
export const SPLIT = {
  even: "lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]",
  figureLeft: "lg:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]",
  proseLeft: "lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]",
} as const;

/* ------------------------------------------------------------------ type -- */

/** Icon sizes, matching plate 07's 24 / 20 / 16 steps. */
export const ICON = {
  section: 20,
  card: 24,
  inline: 16,
  small: 13,
} as const;

/** Plate 03's wordmark metrics, in em relative to the cap size. */
export const WORDMARK_METRICS = {
  capHeight: 0.705,
  width: 0.6698,
  sidebearing: 0.031,
} as const;

/* --------------------------------------------------------------- social -- */

/** Social images are the standard 1.91:1 card. */
export const OG_SIZE = { width: 1200, height: 630 } as const;
export const OG_CONTENT_TYPE = "image/png";
