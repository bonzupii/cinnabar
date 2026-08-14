/*
 * A tokenizer for Cinnabar source, driving the "Cinnabar Dark" theme on
 * plate 09 of the brand board.
 *
 * This is a highlighter, not a parser: it never needs to be right about
 * meaning, only about the seven categories the theme actually colours. Two
 * properties of the language make that unusually reliable:
 *
 *   - Casing is grammar (Manifesto principle 8), enforced by the lexer. A
 *     mis-cased identifier is a lexical error, so an identifier's shape tells
 *     us its kind with no symbol table: PascalCase is a type/trait/module/
 *     variant, SCREAMING_SNAKE_CASE is a constant, snake_case is a binding,
 *     function or parameter.
 *   - `#` always opens a comment. There is no `#` operator to disambiguate
 *     against, and no preprocessor.
 *
 * Linearity is the one thing shape cannot tell us — whether a handle is linear
 * depends on facts the typechecker computes. Plate 09 marks linear handles
 * with a dotted rule rather than a colour, and this module takes the set of
 * such names from the caller rather than guessing.
 */

export type TokenKind =
  | "comment"
  | "doc-comment"
  | "keyword"
  | "type"
  | "constant"
  | "function"
  | "identifier"
  | "literal"
  | "string"
  | "punctuation"
  | "text";

export type Token = {
  kind: TokenKind;
  value: string;
  /** True when the caller declared this identifier a linear handle. */
  linear?: boolean;
};

/**
 * Every keyword in the language surface, from MANIFESTO.md. Keeping this list
 * exhaustive matters: an unknown keyword falls through to the identifier rules
 * and would be mis-coloured as a binding.
 */
export const CINNABAR_KEYWORDS = new Set([
  // Declarations and visibility.
  "pub",
  "mod",
  "type",
  "nat",
  "fun",
  "const",
  "val",
  "var",
  "use",
  "as",
  "impure",
  "trait",
  "impl",
  "end",
  // Control flow.
  "if",
  "elif",
  "else",
  "while",
  "break",
  "continue",
  "return",
  "match",
  "try",
]);

/** Boolean literals are literals, not keywords — plate 09 colours them as such. */
const BOOLEAN_LITERALS = new Set(["true", "false"]);

/**
 * Compiler builtins. These are needed because the casing heuristic alone would
 * read `I64` or `U8` as SCREAMING_SNAKE_CASE — they carry no lowercase letter.
 */
export const CINNABAR_BUILTIN_TYPES = new Set([
  "Unit",
  "Result",
  "Option",
  "IndexError",
  "DivError",
  "Bool",
  "I8",
  "I16",
  "I32",
  "I64",
  "Isize",
  "U8",
  "U16",
  "U32",
  "U64",
  "Usize",
]);

const IDENT_START = /[A-Za-z_]/;
const IDENT_PART = /[A-Za-z0-9_]/;
const DIGIT = /[0-9]/;

/**
 * Classifies an identifier by the casing the lexer already enforced.
 *
 * A single uppercase letter (`T`, `K`, `V`) is a generic parameter and so a
 * type, not a one-letter constant — that is the only case where "no lowercase
 * letter" does not imply SCREAMING_SNAKE_CASE, apart from the builtins.
 */
export function classifyIdentifier(name: string): TokenKind {
  if (CINNABAR_KEYWORDS.has(name)) return "keyword";
  if (BOOLEAN_LITERALS.has(name)) return "literal";
  if (CINNABAR_BUILTIN_TYPES.has(name)) return "type";

  const first = name[0];
  if (first >= "A" && first <= "Z") {
    if (name.length === 1) return "type";
    // PascalCase carries at least one lowercase letter; SCREAMING_SNAKE_CASE
    // carries none.
    return /[a-z]/.test(name) ? "type" : "constant";
  }
  return "identifier";
}

/**
 * Tokenizes a Cinnabar source string.
 *
 * @param source the program text
 * @param linearHandles names to mark with the dotted linear rule. The
 *   typechecker owns this fact; a document that shows a linear handle states
 *   which bindings those are rather than having this module infer it.
 */
export function tokenizeCinnabar(
  source: string,
  linearHandles: readonly string[] = [],
): Token[] {
  const linear = new Set(linearHandles);
  const tokens: Token[] = [];
  let index = 0;

  const push = (kind: TokenKind, value: string, isLinear = false) => {
    if (value.length === 0) return;
    tokens.push(isLinear ? { kind, value, linear: true } : { kind, value });
  };

  while (index < source.length) {
    const char = source[index];

    // Whitespace, including newlines, is passed through untouched so the
    // renderer can preserve the source's own layout.
    if (char === " " || char === "\t" || char === "\n" || char === "\r") {
      let end = index;
      while (end < source.length && /[ \t\n\r]/.test(source[end])) end += 1;
      push("text", source.slice(index, end));
      index = end;
      continue;
    }

    // Comments. Four forms; the block forms are terminated by `|#`. Block
    // comments do not nest in Cinnabar, so the first closer is the real one
    // and scanning for it cannot overshoot.
    if (char === "#") {
      const isDoc = source[index + 1] === "!";
      const blockOpensAt = isDoc ? index + 2 : index + 1;
      const isBlock = source[blockOpensAt] === "|";

      if (isBlock) {
        const close = source.indexOf("|#", blockOpensAt + 1);
        const end = close === -1 ? source.length : close + 2;
        push(isDoc ? "doc-comment" : "comment", source.slice(index, end));
        index = end;
      } else {
        let end = index;
        while (end < source.length && source[end] !== "\n") end += 1;
        push(isDoc ? "doc-comment" : "comment", source.slice(index, end));
        index = end;
      }
      continue;
    }

    // String literals. Exactly five escapes exist and a literal never spans a
    // line, so an unterminated string ends at the newline rather than
    // swallowing the rest of the document.
    if (char === '"') {
      let end = index + 1;
      while (end < source.length && source[end] !== '"' && source[end] !== "\n") {
        end += source[end] === "\\" ? 2 : 1;
      }
      if (end < source.length && source[end] === '"') end += 1;
      push("string", source.slice(index, Math.min(end, source.length)));
      index = Math.min(end, source.length);
      continue;
    }

    // Numeric literals: decimal, and `0x` hex.
    if (DIGIT.test(char)) {
      let end = index;
      if (char === "0" && (source[index + 1] === "x" || source[index + 1] === "X")) {
        end = index + 2;
        while (end < source.length && /[0-9a-fA-F_]/.test(source[end])) end += 1;
      } else {
        while (end < source.length && /[0-9_]/.test(source[end])) end += 1;
      }
      push("literal", source.slice(index, end));
      index = end;
      continue;
    }

    // Identifiers and keywords.
    if (IDENT_START.test(char)) {
      let end = index;
      while (end < source.length && IDENT_PART.test(source[end])) end += 1;
      const name = source.slice(index, end);
      let kind = classifyIdentifier(name);

      // A snake_case identifier applied to arguments or type arguments is a
      // call; plate 09 sets those a step brighter than a plain binding.
      if (kind === "identifier") {
        let lookahead = end;
        while (lookahead < source.length && /[ \t]/.test(source[lookahead])) {
          lookahead += 1;
        }
        const next = source[lookahead];
        if (next === "(" || next === "[") kind = "function";
      }

      push(kind, name, kind === "identifier" && linear.has(name));
      index = end;
      continue;
    }

    // Multi-character operators, longest first so `<<` never lexes as two `<`.
    const two = source.slice(index, index + 2);
    if (["==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "=>", "->", ".."].includes(two)) {
      push("punctuation", two);
      index += 2;
      continue;
    }

    push("punctuation", char);
    index += 1;
  }

  return tokens;
}
