/*
 * Types and pure helpers for the JSON `crates/cinnabar-wasm`'s `check()`
 * returns (`cinnabar.playground-diagnostics.v1`) — the same shape
 * `finish_with_diagnostics_json` in the compiler's own `src/main.rs` uses for
 * `--explain-borrow=json`, both built from the `Diag`/`Note` tuples the
 * pipeline attaches.
 */

export type DiagnosticSource = {
  file_id: number;
  path: string | null;
  start: number;
  end: number;
};

export type DiagnosticExplanation = {
  message: string;
  source: DiagnosticSource | null;
};

export type PlaygroundDiagnostic = {
  severity: "error";
  message: string;
  source: DiagnosticSource | null;
  explanations: readonly DiagnosticExplanation[];
};

export type PlaygroundReport = {
  format: string;
  diagnostics: readonly PlaygroundDiagnostic[];
  serialization_error?: string;
};

export type LocatedSpan = {
  /** 1-indexed, matching the compiler's own `path:line:col` diagnostics. */
  line: number;
  /** 1-indexed. */
  column: number;
  /** The full text of the line the span starts on, for a source excerpt. */
  lineText: string;
  /** How many characters into `lineText` the span starts. */
  columnOffset: number;
  /** How many characters of `lineText` the span covers (clipped to the line). */
  length: number;
};

const NEWLINE_BYTE = 10; // '\n', identical in ASCII and UTF-8

/**
 * Locates a byte span within `source` for display — line/column, the line's
 * text, and where within that line the span falls, clipped at the line break
 * if the span crosses one.
 *
 * The compiler's spans are byte offsets into the UTF-8 source, not UTF-16
 * code units, so `start`/`end` cannot index the JS string `source` directly
 * once it carries any character outside the ASCII range — the language
 * allows non-ASCII in string and comment contents (casing rules only
 * confine identifiers), so that's reachable from real playground input, not
 * just a theoretical case. Re-encoding to bytes once and locating the line
 * boundaries and column offset in that byte space, then decoding only the
 * located slices back to text, keeps every offset in the same coordinate
 * system the compiler used to produce it.
 */
export function locateSpan(source: string, start: number, end: number): LocatedSpan {
  const bytes = new TextEncoder().encode(source);
  const decoder = new TextDecoder();

  const lineStart = bytes.lastIndexOf(NEWLINE_BYTE, Math.max(start - 1, 0)) + 1;
  let lineEnd = bytes.indexOf(NEWLINE_BYTE, start);
  if (lineEnd === -1) lineEnd = bytes.length;

  const precedingNewlines = countByte(bytes, NEWLINE_BYTE, start);
  const beforeCaret = decoder.decode(bytes.slice(lineStart, Math.min(start, bytes.length)));
  const spanText = decoder.decode(bytes.slice(start, Math.min(end, lineEnd)));
  return {
    line: precedingNewlines + 1,
    column: beforeCaret.length + 1,
    lineText: decoder.decode(bytes.slice(lineStart, lineEnd)),
    columnOffset: beforeCaret.length,
    length: Math.max(0, spanText.length),
  };
}

function countByte(bytes: Uint8Array, target: number, upTo: number): number {
  let count = 0;
  for (let index = 0; index < upTo && index < bytes.length; index += 1) {
    if (bytes[index] === target) count += 1;
  }
  return count;
}

/** Whether a report carries no diagnostics — the playground's success state. */
export function isClean(report: PlaygroundReport): boolean {
  return report.diagnostics.length === 0 && !report.serialization_error;
}
