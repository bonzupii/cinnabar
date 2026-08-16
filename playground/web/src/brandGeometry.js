// The board's coordinates, with no JSX around them.
//
// Split out of `brandMarks.jsx` so `test/brand.drift.test.js` can import the
// geometry under plain Node, which has no loader for `.jsx`. The components
// read these same constants, so the drawing and the thing under test cannot
// be different numbers.

/* ---- the mark (plates 01, 02) ------------------------------------- */

export const MARK_LETTER_POINTS =
  "95,0 44,0 0,30 0,70 44,100 95,100 95,78.72 52.7,78.72 24.8,59.69 24.8,40.31 52.7,21.28 95,21.28";
export const MARK_BLOCK = { x: 52, y: 33, width: 34, height: 34 };
export const MARK_VIEWBOX_STANDALONE = "-2.5 0 100 100";

/* ---- the icon set (plate 07) --------------------------------------- */

export const FIGURE = {
  diamond: "12,2.5 21.5,12 12,21.5 2.5,12",
  play: "10,8.5 16,12 10,15.5",
  tick: "8,12 11,15 16,9",
  chevron: "8.5,11 12,14.5 15.5,11",
  bracketLeft: "8,3.5 3,12 8,20.5",
  bracketRight: "16,3.5 21,12 16,20.5",
  gem: "12,2.5 19,9.5 12,16.5 5,9.5",
};

/**
 * Every whole-attribute string this port copies, for the drift test.
 *
 * Redrawing one, or adding an icon without listing its geometry, fails that
 * test — the only thing stopping the two copies from parting ways.
 *
 * `line` and `rect` primitives carry their coordinates in separate
 * attributes with no single string to compare; `MARK_BLOCK` is checked
 * field by field instead, and the icons' plain lines are left to review.
 */
export const PORTED_GEOMETRY = [
  MARK_LETTER_POINTS,
  MARK_VIEWBOX_STANDALONE,
  ...Object.values(FIGURE),
];
