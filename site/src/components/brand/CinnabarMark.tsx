/*
 * The mark — plates 01 and 02 of the brand board.
 *
 * Schibsted Grotesk's C redrawn with straight strokes: cap height 0.705 em,
 * stem 0.248 cap, arms 0.213 cap (the I's crossbar), width 0.95 cap, two 34°
 * chamfers parallel inside and out, terminals cut square, no overshoot. The
 * block is a 0.34-cap square centred on the counter.
 *
 * The geometry below is the board's own path data and must not be redrawn.
 */

/** The C outline, in the board's 0–100 cap-unit space. */
export const MARK_LETTER_POINTS =
  "95,0 44,0 0,30 0,70 44,100 95,100 95,78.72 52.7,78.72 24.8,59.69 24.8,40.31 52.7,21.28 95,21.28";

/** The block held in the counter. */
export const MARK_BLOCK = { x: 52, y: 33, width: 34, height: 34 } as const;

/**
 * Standalone framing. The -2.5 left inset is the board's — it balances the
 * open right side of the C so the mark optically centres in a square.
 */
export const MARK_VIEWBOX_STANDALONE = "-2.5 0 100 100";

/** Framing used when the C stands in for the letter inside the wordmark. */
export const MARK_VIEWBOX_INLINE = "0 0 95 100";

/**
 * Plate 14, misuse 1: "Do not recolour the block. It is vermilion, or it
 * matches the letter — nothing else." Those are the only two variants.
 */
export type MarkVariant = "duotone" | "mono";

type CinnabarMarkProps = {
  /** Rendered width and height in px. The board's floor is 16. */
  size?: number;
  variant?: MarkVariant;
  /**
   * Letter colour. Defaults to `currentColor` so the mark inherits from its
   * container — which is what makes the knockout and on-paper lock-ups work
   * without a second component.
   */
  letter?: string;
  /** Block colour. Ignored when `variant` is "mono". */
  block?: string;
  className?: string;
  /**
   * Accessible name. Omit for decorative uses that sit beside a text label;
   * the mark is then hidden from assistive technology.
   */
  title?: string;
};

export default function CinnabarMark({
  size = 32,
  variant = "duotone",
  letter = "currentColor",
  block = "var(--cinnabar)",
  className,
  title,
}: CinnabarMarkProps) {
  const blockFill = variant === "mono" ? letter : block;

  return (
    <svg
      viewBox={MARK_VIEWBOX_STANDALONE}
      width={size}
      height={size}
      className={className}
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      focusable="false"
      style={{ display: "block", flex: "none" }}
    >
      <polygon points={MARK_LETTER_POINTS} fill={letter} />
      <rect {...MARK_BLOCK} fill={blockFill} />
    </svg>
  );
}

/**
 * The same mark sized to sit on the baseline inside a run of text, replacing
 * the C of CINNABAR. Plate 03 fixes the metrics: 0.705 em tall, 0.6698 em
 * wide, 0.031 em of right sidebearing.
 */
export function InlineMark({
  variant = "duotone",
  letter = "currentColor",
  block = "var(--cinnabar)",
}: Pick<CinnabarMarkProps, "variant" | "letter" | "block">) {
  const blockFill = variant === "mono" ? letter : block;

  return (
    <svg
      viewBox={MARK_VIEWBOX_INLINE}
      aria-hidden="true"
      focusable="false"
      style={{
        height: "0.705em",
        width: "0.6698em",
        display: "inline-block",
        verticalAlign: "baseline",
        marginRight: "0.031em",
      }}
    >
      <polygon points={MARK_LETTER_POINTS} fill={letter} />
      <rect {...MARK_BLOCK} fill={blockFill} />
    </svg>
  );
}
