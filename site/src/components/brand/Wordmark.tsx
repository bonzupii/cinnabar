import { InlineMark, type MarkVariant } from "./CinnabarMark";

/*
 * The wordmark — plate 03.
 *
 * Schibsted Grotesk 800, all caps, one weight only. The C is always the drawn
 * letter, never the foundry's glyph ("substituting it removes the identity").
 * Tracking opens as the size falls, on the board's own four steps.
 */

/** The board's tracking ladder, keyed by the size band it applies to. */
export const WORDMARK_TRACKING = {
  /** Above 64 px. */
  display: "-0.035em",
  /** Down to 32 px. */
  lg: "-0.03em",
  /** Down to 16 px. */
  md: "-0.02em",
  /** Below 16 px. Plate 03 floors the wordmark at 12 px; below that, mark alone. */
  sm: "-0.005em",
} as const;

export type WordmarkStep = keyof typeof WORDMARK_TRACKING;

/** Picks the tracking step the board specifies for a given rendered size. */
export function trackingForSize(px: number): WordmarkStep {
  if (px > 64) return "display";
  if (px > 32) return "lg";
  if (px > 16) return "md";
  return "sm";
}

type WordmarkProps = {
  /**
   * Cap size. A number is treated as px and picks its own tracking step; a CSS
   * length (e.g. a `clamp()`) is used verbatim, and then `step` decides the
   * tracking since it cannot be derived.
   */
  size?: number | string;
  /** Tracking step. Required in effect when `size` is a CSS length. */
  step?: WordmarkStep;
  variant?: MarkVariant;
  letter?: string;
  block?: string;
  className?: string;
};

export default function Wordmark({
  size = 20,
  step,
  variant = "duotone",
  letter = "currentColor",
  block = "var(--cinnabar)",
  className,
}: WordmarkProps) {
  const tracking =
    step ?? (typeof size === "number" ? trackingForSize(size) : "display");

  return (
    <span className={className}>
      {/*
       * The visible run is the drawn C plus the letters INNABAR, which is what
       * a screen reader would otherwise announce. Hide the rendering and carry
       * the real name alongside it.
       */}
      <span className="sr-only">Cinnabar</span>
      <span
        aria-hidden="true"
        style={{
          fontSize: size,
          fontWeight: 800,
          lineHeight: 1,
          letterSpacing: WORDMARK_TRACKING[tracking],
          whiteSpace: "nowrap",
        }}
      >
        <InlineMark variant={variant} letter={letter} block={block} />
        INNABAR
      </span>
    </span>
  );
}
