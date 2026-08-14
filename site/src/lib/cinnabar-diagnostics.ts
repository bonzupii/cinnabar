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

/**
 * Locates a byte span within `source` for display — line/column, the line's
 * text, and where within that line the span falls, clipped at the line break
 * if the span crosses one.
 *
 * Byte offsets rather than UTF-16 code units: the compiler's spans are byte
 * offsets into the UTF-8 source, and every fixture and playground program is
 * ASCII-only source text (comments and strings aside), so indexing `source`
 * by JS string index is byte-accurate here — a real multi-byte identifier
 * would need a UTF-8-aware remap, which the language's casing rules make
 * unreachable for anything but string/comment contents.
 */
export function locateSpan(source: string, start: number, end: number): LocatedSpan {
  const lineStart = source.lastIndexOf("\n", Math.max(start - 1, 0)) + 1;
  let lineEnd = source.indexOf("\n", start);
  if (lineEnd === -1) lineEnd = source.length;

  const precedingNewlines = countChar(source, "\n", start);
  return {
    line: precedingNewlines + 1,
    column: start - lineStart + 1,
    lineText: source.slice(lineStart, lineEnd),
    columnOffset: start - lineStart,
    length: Math.max(0, Math.min(end, lineEnd) - start),
  };
}

function countChar(source: string, char: string, upTo: number): number {
  let count = 0;
  for (let index = 0; index < upTo && index < source.length; index += 1) {
    if (source[index] === char) count += 1;
  }
  return count;
}

/** Whether a report carries no diagnostics — the playground's success state. */
export function isClean(report: PlaygroundReport): boolean {
  return report.diagnostics.length === 0 && !report.serialization_error;
}
