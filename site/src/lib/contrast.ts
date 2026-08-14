/*
 * WCAG relative luminance and contrast, used to keep the palette honest.
 *
 * The brand board states three ratios on plate 05 (text 15.8:1, secondary
 * 7.4:1, vermilion 4.9:1 against the ground). Those are the ones it checked;
 * the smaller greys it uses for mono labels were never given a figure, and
 * some of them do not clear AA for text. `tests/unit/palette.test.ts` pins
 * which token may carry text at which size.
 */

export type Rgb = { r: number; g: number; b: number };

export function parseHex(hex: string): Rgb {
  const value = hex.replace("#", "").trim();
  const full =
    value.length === 3
      ? value
          .split("")
          .map((char) => char + char)
          .join("")
      : value;
  if (!/^[0-9a-fA-F]{6}$/.test(full)) {
    throw new Error(`not a hex colour: ${hex}`);
  }
  return {
    r: Number.parseInt(full.slice(0, 2), 16),
    g: Number.parseInt(full.slice(2, 4), 16),
    b: Number.parseInt(full.slice(4, 6), 16),
  };
}

/** WCAG 2.x relative luminance. */
export function relativeLuminance(color: Rgb | string): number {
  const { r, g, b } = typeof color === "string" ? parseHex(color) : color;
  const channel = (value: number) => {
    const srgb = value / 255;
    return srgb <= 0.04045 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

/** WCAG contrast ratio between two colours, from 1 to 21. */
export function contrastRatio(a: Rgb | string, b: Rgb | string): number {
  const first = relativeLuminance(a);
  const second = relativeLuminance(b);
  const lighter = Math.max(first, second);
  const darker = Math.min(first, second);
  return (lighter + 0.05) / (darker + 0.05);
}

/** AA thresholds: 4.5:1 for normal text, 3:1 for large text and UI shapes. */
export const AA_NORMAL = 4.5;
export const AA_LARGE = 3;
