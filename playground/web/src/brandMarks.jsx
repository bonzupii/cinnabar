// The mark and the icon set, as the brand board draws them.
//
// Both are ports of `site/src/components/brand/` — the mark from plates 01
// and 02, the icons from plate 07 — with the geometry copied rather than
// redrawn, which the mark's own source requires in as many words: "the
// geometry below is the board's own path data and must not be redrawn."
//
// The site is a TypeScript Next application and this is a Vite one, so the
// components cannot simply be imported across. The coordinates are therefore
// checked against the site's source by `test/brand.drift.test.js` in the
// same way `brand.js` checks the palette: a copy that could drift is
// verified against its original rather than trusted.

import { FIGURE, MARK_BLOCK, MARK_LETTER_POINTS, MARK_VIEWBOX_STANDALONE } from "./brandGeometry.js";

export { MARK_BLOCK, MARK_LETTER_POINTS, MARK_VIEWBOX_STANDALONE };

/**
 * The C with the block in its counter.
 *
 * The letter defaults to `currentColor` so the mark takes the colour of
 * whatever carries it. Plate 14's first misuse rule allows the block to be
 * vermilion or to match the letter, and nothing else, so there are exactly
 * two variants.
 */
export function CinnabarMark({ size = 20, variant = "duotone", letter = "currentColor", title }) {
  const block = variant === "mono" ? letter : "var(--cinnabar)";
  return (
    <svg
      viewBox={MARK_VIEWBOX_STANDALONE}
      width={size}
      height={size}
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      focusable="false"
      style={{ display: "block", flex: "none" }}
    >
      <polygon points={MARK_LETTER_POINTS} fill={letter} />
      <rect {...MARK_BLOCK} fill={block} />
    </svg>
  );
}

/* ---- the icon set (plate 07) --------------------------------------- */

// Vermilion marks the one part of each icon that carries the meaning. It is
// indirected through a custom property so an icon dropped onto the accent
// fill can rebind it: on a vermilion button the vermilion detail would be
// invisible, so `onAccent` points it at `currentColor` and the figure goes
// monochrome rather than losing half of itself.
const ACCENT = "var(--icon-accent, var(--cinnabar))";
const ON_ACCENT_STYLE = { "--icon-accent": "currentColor" };

function Icon({ size = 16, title, onAccent, children }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      // Plate 07: the stroke thickens at 16 px so the figure holds together.
      strokeWidth={size <= 16 ? 1.8 : 1.6}
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      focusable="false"
      style={onAccent ? { display: "block", flex: "none", ...ON_ACCENT_STYLE } : { display: "block", flex: "none" }}
    >
      {title ? <title>{title}</title> : null}
      {children}
    </svg>
  );
}

export function RunIcon(props) {
  return (
    <Icon {...props}>
      <polygon points={FIGURE.diamond} />
      <polygon points={FIGURE.play} fill={ACCENT} stroke="none" />
    </Icon>
  );
}

export function CheckIcon(props) {
  return (
    <Icon {...props}>
      <polygon points={FIGURE.diamond} />
      <polyline points={FIGURE.tick} stroke={ACCENT} />
    </Icon>
  );
}

export function DiagnosticIcon(props) {
  return (
    <Icon {...props}>
      <polygon points={FIGURE.diamond} />
      <line x1="12" y1="7.5" x2="12" y2="13.5" stroke={ACCENT} />
      <circle cx="12" cy="16.6" r="1.05" fill={ACCENT} stroke="none" />
    </Icon>
  );
}

export function BuildIcon(props) {
  return (
    <Icon {...props}>
      <polygon points={FIGURE.diamond} />
      <polyline points={FIGURE.chevron} stroke={ACCENT} />
    </Icon>
  );
}

export function LspIcon(props) {
  return (
    <Icon {...props}>
      <polyline points={FIGURE.bracketLeft} />
      <polyline points={FIGURE.bracketRight} />
      {/* The one curve the set allows. */}
      <circle cx="12" cy="12" r="1.9" fill={ACCENT} stroke="none" />
    </Icon>
  );
}

export function LinearIcon(props) {
  return (
    <Icon {...props}>
      <line x1="12" y1="1.5" x2="12" y2="7" />
      <rect x="5.5" y="7" width="13" height="10" />
      <line x1="12" y1="17" x2="12" y2="22.5" stroke={ACCENT} />
    </Icon>
  );
}

export function CodegenIcon(props) {
  return (
    <Icon {...props}>
      <polygon points={FIGURE.gem} />
      <line x1="5.5" y1="20.5" x2="11" y2="20.5" />
      <line x1="13" y1="20.5" x2="18.5" y2="20.5" stroke={ACCENT} />
    </Icon>
  );
}

export function DocIcon(props) {
  return (
    <Icon {...props}>
      <rect x="3.5" y="3" width="13" height="18" />
      <line x1="20" y1="6.5" x2="20" y2="21" stroke={ACCENT} />
    </Icon>
  );
}
