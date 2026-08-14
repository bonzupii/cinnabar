import type { CSSProperties, ReactNode } from "react";
import Window, { WindowBody } from "@/components/Window";
import {
  BORROW_DIAGNOSTIC,
  DIAGNOSTIC_LEGEND,
  type DiagnosticRole,
  type Segment,
} from "@/content/diagnostic";

/*
 * Renders a role-tagged diagnostic in plate 10's palette.
 *
 * The role-to-style table below is the whole styling contract: vermilion is
 * reserved for the error and its primary span, and everything else stays grey.
 * There is no warning role, because the language has no warnings.
 */
const ROLE_STYLE: Record<DiagnosticRole, string> = {
  error: "text-term-error font-semibold",
  message: "text-term-command font-semibold",
  source: "text-term-flag",
  secondary: "text-term-output",
  gutter: "text-term-gutter",
  prompt: "text-term-prompt",
  command: "text-term-command",
  flag: "text-term-flag",
};

/* ------------------------------------------------------------ the rails -- */

/*
 * Why the rails are drawn rather than typed.
 *
 * The diagnostic is authored the way the compiler prints it, with the box
 * drawing spelled out in characters — `╭`, `│`, `┬`, `╰`, `╯`. In a terminal
 * those tile into a continuous frame. In a browser they cannot, and the reason
 * is not line height:
 *
 *   IBM Plex Mono has no box-drawing glyphs. Every one of them is served by a
 *   fallback face, at that face's advance rather than the mono cell. Measured
 *   on the built page at 16px the mono cell is 9.60px, `│ ─ ┬` arrive at
 *   11.34px and `╭ ╰ ╯` at 16px — three different widths in one line. So the
 *   rail breaks at every corner, and every column after a box character falls
 *   out of step with the source line above it. No line-height and no
 *   font-feature setting can fix a glyph the font does not have.
 *
 * So each box character is laid out as an empty cell exactly one `ch` wide —
 * which restores the column arithmetic src/content/diagnostic.ts documents —
 * and the stroke is painted with a CSS border inside that cell. A vertical
 * stroke spans the full height of its line box, so consecutive lines abut with
 * no seam whatever the leading is; a horizontal stroke spans the full width of
 * its cell, so runs of `─` are continuous. Corners meet because both strokes
 * are drawn to the centre of the same cell.
 *
 * The strokes are decoration and are hidden from assistive technology, which
 * is also an improvement: a screen reader announcing "box drawings light
 * vertical" fifteen times is noise. The text stays text and copies as text —
 * the rails copy as the spaces they occupy, so pasted output keeps its
 * alignment.
 */

/** Which way each box character's arms point. */
const BOX_ARMS: Record<
  string,
  Partial<Record<"up" | "down" | "left" | "right", true>>
> = {
  "─": { left: true, right: true },
  "│": { up: true, down: true },
  "┌": { down: true, right: true },
  "┐": { down: true, left: true },
  "└": { up: true, right: true },
  "┘": { up: true, left: true },
  "╭": { down: true, right: true },
  "╮": { down: true, left: true },
  "╰": { up: true, right: true },
  "╯": { up: true, left: true },
  "┬": { left: true, right: true, down: true },
  "┴": { left: true, right: true, up: true },
  "├": { up: true, down: true, right: true },
  "┤": { up: true, down: true, left: true },
  "┼": { up: true, down: true, left: true, right: true },
};

/**
 * Half a stroke of overlap at the centre of a cell.
 *
 * Without it an arm that stops exactly at the midpoint leaves a hairline notch
 * where it meets the perpendicular stroke — the same defect one level down.
 */
const JOIN = "0.5px";

/** The strokes for one box character, positioned in its own column. */
function boxStrokes(char: string, column: number): CSSProperties[] {
  const arms = BOX_ARMS[char];
  if (!arms) return [];

  const left = `calc(${column}ch)`;
  const centre = `calc(${column}ch + 0.5ch)`;
  const strokes: CSSProperties[] = [];

  if (arms.left || arms.right) {
    strokes.push({
      position: "absolute",
      top: "50%",
      left: arms.left ? left : `calc(${centre} - ${JOIN})`,
      width: arms.left && arms.right ? "1ch" : `calc(0.5ch + ${JOIN})`,
      borderTopWidth: 1,
    });
  }
  if (arms.up || arms.down) {
    strokes.push({
      position: "absolute",
      left: centre,
      top: arms.up ? 0 : `calc(50% - ${JOIN})`,
      height: arms.up && arms.down ? "100%" : `calc(50% + ${JOIN})`,
      borderLeftWidth: 1,
    });
  }

  return strokes;
}

/** Splits a string into runs that are either all box characters or none. */
function runs(text: string): { box: boolean; text: string }[] {
  const out: { box: boolean; text: string }[] = [];
  for (const char of text) {
    const box = char in BOX_ARMS;
    const last = out[out.length - 1];
    if (last && last.box === box) last.text += char;
    else out.push({ box, text: char });
  }
  return out;
}

/**
 * One rendered line: the text in flow, the strokes painted over it.
 *
 * A block rather than an inline run, because the strokes are positioned
 * against the line box, and an inline element's containing block is its font
 * box — which would leave the leading unpainted and put the seams back.
 */
function Line({ segments }: { segments: readonly Segment[] }) {
  const flow: ReactNode[] = [];
  const drawn: ReactNode[] = [];
  let column = 0;

  segments.forEach((segment, index) => {
    for (const run of runs(segment.text)) {
      const characters = [...run.text];
      if (run.box) {
        // The cell is held open in the flow by a space of the same width, so
        // everything after it stays in its documented column.
        flow.push(
          <span key={`f${index}-${column}`}>{" ".repeat(characters.length)}</span>,
        );
        characters.forEach((char, offset) => {
          boxStrokes(char, column + offset).forEach((style, stroke) => {
            drawn.push(
              <span
                key={`d${index}-${column + offset}-${stroke}`}
                aria-hidden="true"
                className={`pointer-events-none border-current ${ROLE_STYLE[segment.role]}`}
                style={style}
              />,
            );
          });
        });
      } else {
        flow.push(
          <span key={`f${index}-${column}`} className={ROLE_STYLE[segment.role]}>
            {run.text}
          </span>,
        );
      }
      column += characters.length;
    }
  });

  return (
    <span className="relative block" data-diagnostic-line="">
      {/* A blank line still needs a line box, or the rail passing through it
          would have no height to span. */}
      {flow.length === 0 ? " " : flow}
      {drawn}
    </span>
  );
}

export default function DiagnosticTranscript({
  lines = BORROW_DIAGNOSTIC,
  className,
}: {
  lines?: readonly Segment[][];
  className?: string;
}) {
  return (
    <Window path="~/src/kernel" title="Borrow diagnostic" className={className}>
      <WindowBody scale="diagnostic">
        <code>
          {lines.map((segments, index) => (
            <Line key={index} segments={segments} />
          ))}
        </code>
      </WindowBody>
    </Window>
  );
}

/**
 * The legend beside the transcript.
 *
 * Plate 09 puts a filled swatch next to each label rather than tinting the
 * label itself — which is also the only honest way to show these values, since
 * setting "#7C7570" in #7C7570 would not be legible on this panel.
 */
export function DiagnosticLegend() {
  return (
    <dl className="border-hairline flex flex-col border-t">
      {DIAGNOSTIC_LEGEND.map(({ role, value, weight }) => (
        <div
          key={role}
          className="border-hairline flex items-center gap-3 border-b py-3.5 font-mono text-xs sm:gap-3.5"
        >
          <span
            aria-hidden="true"
            className="border-hairline h-3.5 w-3.5 flex-none border"
            style={{ background: value }}
          />
          <dt className="text-label min-w-0">{role}</dt>
          {/* The value and its weight are one unit and must not break across
              the `·`; at 390px the row still fits because the role label can
              shrink instead. */}
          <dd className="text-secondary ml-auto min-w-0 text-right whitespace-nowrap">
            {value} · {weight}
          </dd>
        </div>
      ))}
    </dl>
  );
}
