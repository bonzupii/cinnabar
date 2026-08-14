/*
 * A tokenizer for shell transcripts and CLI usage synopses.
 *
 * Plate 10 styles terminal output as well as source, and gives the roles it
 * uses: the prompt sits in the gutter grey, the command reads brightest, flags
 * sit a step below it, and everything else stays quiet. Vermilion is reserved
 * for an error — "there is no warning colour, because there are no warnings".
 *
 * This is a presentation tokenizer, not a shell parser: it never needs to know
 * what a line means, only which of six roles each run of characters plays.
 */

export type ShellTokenKind =
  | "prompt"
  | "command"
  | "flag"
  | "placeholder"
  | "operator"
  | "comment"
  | "plain";

export type ShellToken = { kind: ShellTokenKind; value: string };

const OPERATORS = new Set(["&&", "||", "|", ">", ">>", "<", ";"]);

/**
 * Tokenizes one command line.
 *
 * @param line the command, without its `$ ` prompt
 * @param isCommand false for a line of program output, which takes no roles
 */
export function tokenizeShellLine(line: string, isCommand = true): ShellToken[] {
  if (!isCommand) return line.length > 0 ? [{ kind: "plain", value: line }] : [];

  const tokens: ShellToken[] = [];
  // A comment runs to the end of the line and must be split off first, or its
  // contents would be tokenized as arguments.
  const commentAt = findCommentStart(line);
  const code = commentAt === -1 ? line : line.slice(0, commentAt);
  const comment = commentAt === -1 ? "" : line.slice(commentAt);

  // Split on whitespace but keep it, so the transcript's own spacing survives.
  const parts = code.split(/(\s+)/);
  let seenCommand = false;

  for (const part of parts) {
    if (part.length === 0) continue;
    if (/^\s+$/.test(part)) {
      tokens.push({ kind: "plain", value: part });
      continue;
    }
    if (OPERATORS.has(part)) {
      tokens.push({ kind: "operator", value: part });
      // What follows an operator is a fresh command.
      seenCommand = false;
      continue;
    }
    if (part.startsWith("-")) {
      tokens.push({ kind: "flag", value: part });
      continue;
    }
    if (/^[<[].*[>\]]$/.test(part)) {
      tokens.push({ kind: "placeholder", value: part });
      continue;
    }
    if (!seenCommand) {
      tokens.push({ kind: "command", value: part });
      seenCommand = true;
      continue;
    }
    tokens.push({ kind: "plain", value: part });
  }

  if (comment.length > 0) tokens.push({ kind: "comment", value: comment });
  return tokens;
}

/**
 * Locates a trailing `#` comment.
 *
 * A `#` inside quotes is not a comment, and neither is one glued to a word —
 * `foo#bar` is an argument. Only a `#` at the start of a word counts.
 */
function findCommentStart(line: string): number {
  let quote: string | null = null;
  for (let index = 0; index < line.length; index += 1) {
    const char = line[index];
    if (quote) {
      if (char === quote) quote = null;
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
      continue;
    }
    if (char === "#" && (index === 0 || /\s/.test(line[index - 1]))) return index;
  }
  return -1;
}

/**
 * Tokenizes a usage synopsis — the shape `cinnabar --help` prints, where
 * bracketed groups are placeholders rather than literal arguments.
 */
export function tokenizeUsageLine(line: string): ShellToken[] {
  const tokens: ShellToken[] = [];
  // Placeholders may contain spaces and nested brackets, so they are matched
  // as whole groups before the line is split on whitespace.
  const pattern = /(\[[^\]]*\]|<[^>]*>)|(\s+)|([^\s]+)/g;
  let seenCommand = false;

  for (const match of line.matchAll(pattern)) {
    const [, placeholder, space, word] = match;
    if (placeholder) {
      tokens.push({ kind: "placeholder", value: placeholder });
    } else if (space) {
      tokens.push({ kind: "plain", value: space });
    } else if (word) {
      if (word.startsWith("-")) tokens.push({ kind: "flag", value: word });
      else if (!seenCommand) {
        tokens.push({ kind: "command", value: word });
        seenCommand = true;
      } else tokens.push({ kind: "plain", value: word });
    }
  }
  return tokens;
}
