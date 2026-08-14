/*
 * Recognising the diagrams the repository documents draw in text.
 *
 * ARCHITECTURE.md draws the compiler pipeline as an untagged fenced block of
 * box-drawing characters. Rendered as a terminal window it read as program
 * output, which is what it is not: it is a figure. These helpers let the
 * markdown renderer tell a figure from a transcript, and — when the figure is
 * a plain top-to-bottom flow — recover the labels so it can be drawn properly
 * instead of reproduced as text.
 *
 * Nothing here matches the current wording of any block. The tests decide from
 * the shape of the characters, so an upstream edit to ARCHITECTURE.md changes
 * what the figure says without changing whether it is recognised. If the block
 * grows a branch, a legend or a second column, `parseFlow` declines and the
 * caller falls back to presenting the art as written; if it stops being a
 * diagram at all, `isAsciiDiagram` declines and it is an ordinary block again.
 * No path renders nothing.
 */

/**
 * Box-drawing (U+2500–U+257F), block and geometric arrowheads (U+25B2, U+25BC,
 * U+25C0, U+25B6) and the arrow block (U+2190–U+21FF).
 *
 * A character from these ranges is drawing a rule or an arrow; none of them
 * occurs in prose, in shell transcripts or in Cinnabar source.
 */
const DRAWING = /[─-╿←-⇿▲▶▼◀]/u;

/**
 * A line that draws only a connector.
 *
 * The ASCII fallbacks (`|`, `-`, `+`, `v`, `^`, `>`, `<`) are included because
 * plenty of documents draw flows without the Unicode set, but a line of them
 * alone is not enough to call a block a diagram — `isAsciiDiagram` still wants
 * a real drawing character somewhere in the block before any of this applies.
 */
const CONNECTOR_ONLY =
  /^[\s─-╿←-⇿▲▶▼◀|+\-^v<>]+$/u;

/** True when a fenced block is drawing a figure rather than showing text. */
export function isAsciiDiagram(text: string): boolean {
  return DRAWING.test(text);
}

/**
 * The labels of a single-column, top-to-bottom flow, in order.
 *
 * Returns `null` for anything else — a branching diagram, a table, a box with
 * text inside it, a figure with fewer than two labels — because those cannot
 * be redrawn as a run of boxes without inventing structure the source does not
 * have.
 */
export function parseFlow(text: string): string[] | null {
  const labels: string[] = [];
  let sawConnectorSinceLabel = true;

  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.length === 0) continue;

    if (CONNECTOR_ONLY.test(line)) {
      sawConnectorSinceLabel = true;
      continue;
    }

    // A label with a rule through it is part of a drawn box, not a node this
    // can lift out. Decline rather than guess.
    if (DRAWING.test(line)) return null;

    // Two labels running together with nothing between them are a paragraph,
    // not a flow.
    if (!sawConnectorSinceLabel) return null;

    labels.push(line);
    sawConnectorSinceLabel = false;
  }

  return labels.length >= 2 ? labels : null;
}
