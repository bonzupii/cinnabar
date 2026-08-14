/*
 * A borrow-checker diagnostic, as data.
 *
 * The layout follows what the compiler actually prints through ariadne: a
 * capitalised `Error:` line, a bracketed source header, then a gutter of line
 * numbers and rails, with each span underlined directly beneath the code it
 * refers to and a connector dropping to its label.
 *
 * Column alignment is load-bearing and easy to break by hand, so it is stated
 * once here rather than being re-derived per line:
 *
 *   - the rail sits at column 4, and the opening `╭` and closing `╯` sit in
 *     that same column so the box closes;
 *   - the gutter is ` NN │` — five columns wide;
 *   - source text begins at column 8;
 *   - an underline sits under the span it marks, and its `┬` is the column its
 *     `╰──` label hangs from.
 *
 * Plate 10 fixes the palette: vermilion for the error and its primary span,
 * grey for everything else. There is no warning role, because the language has
 * no warnings.
 */

export type DiagnosticRole =
  /** The word `Error` and its primary span. The only vermilion on the plate. */
  | "error"
  /** The message beside `Error`, and `help`. */
  | "message"
  /** Quoted source lines. */
  | "source"
  /** Secondary spans, notes and help text. */
  | "secondary"
  /** Line numbers, rails and box drawing. */
  | "gutter"
  /** The shell prompt. */
  | "prompt"
  /** A command name. */
  | "command"
  /** A flag or a path. */
  | "flag";

export type Segment = { role: DiagnosticRole; text: string };

/** Each entry is one rendered line; an empty array is a blank line. */
export const BORROW_DIAGNOSTIC: readonly Segment[][] = [
  [
    { role: "prompt", text: "$ " },
    { role: "command", text: "cinnabar" },
    { role: "secondary", text: " src/main.cnb " },
    { role: "flag", text: "--explain-borrow" },
  ],
  [],
  [
    { role: "error", text: "Error" },
    { role: "message", text: ": linear value 'vec' is consumed on some paths but not on all paths" },
  ],
  [
    { role: "gutter", text: "    ╭─[ " },
    { role: "flag", text: "src/main.cnb:14:5" },
    { role: "gutter", text: " ]" },
  ],
  [{ role: "gutter", text: "    │" }],
  [
    { role: "gutter", text: " 11 │   " },
    { role: "source", text: "val vec = vec_new[I64]()?" },
  ],
  [
    { role: "gutter", text: "    │       " },
    { role: "secondary", text: "─┬─" },
  ],
  [
    { role: "gutter", text: "    │        " },
    { role: "secondary", text: "╰── bound here as 'Collections.Vec(I64)', linear" },
  ],
  [{ role: "gutter", text: "    │" }],
  [
    { role: "gutter", text: " 15 │     " },
    { role: "source", text: "return 0" },
  ],
  [
    { role: "gutter", text: "    │     " },
    { role: "error", text: "────┬───" },
  ],
  [
    { role: "gutter", text: "    │         " },
    { role: "error", text: "╰── this path returns without consuming 'vec'" },
  ],
  [{ role: "gutter", text: "    │" }],
  [
    { role: "gutter", text: " 18 │   " },
    { role: "source", text: "vec_free(vec)" },
  ],
  [
    { role: "gutter", text: "    │   " },
    { role: "secondary", text: "──────┬──────" },
  ],
  [
    { role: "gutter", text: "    │         " },
    { role: "secondary", text: "╰── consumed on the other path" },
  ],
  [{ role: "gutter", text: "────╯" }],
  [
    { role: "message", text: "help" },
    { role: "secondary", text: ": consume 'vec' before returning, or restructure so both" },
  ],
  [{ role: "secondary", text: "      paths leave through one exit." }],
];

/** Plate 10's legend: the role, and the value and weight the board gives it. */
export const DIAGNOSTIC_LEGEND = [
  { role: "error", value: "#E0442A", weight: "600" },
  { role: "message", value: "#EDE9E6", weight: "600" },
  { role: "source", value: "#C9C2BD", weight: "400" },
  { role: "secondary", value: "#A29B96", weight: "400" },
  { role: "gutter", value: "#7C7570", weight: "300" },
] as const;
