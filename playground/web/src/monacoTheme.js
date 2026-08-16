// "Cinnabar Dark" as a Monaco theme.
//
// Plate 09 names six syntax roles. Monaco's tokenizer in
// `packages/cinnabar-monaco` emits more token types than that, so each one
// is assigned to whichever of the six roles it *is* — a function name and a
// variable are both identifiers, a number and a string are both literals —
// rather than given a colour of its own. Plate 14's misuse rules forbid
// adding colours to the theme, and a seventh hue here would be exactly that.
//
// Doc comments are the one place two roles would be useful and only one is
// available: `#!` attaches to a declaration and `#` does not. They are
// separated by italics instead, which is a style and not a colour.

import { SURFACE, SYNTAX } from "./brand.js";

export const THEME_ID = "cinnabar-dark";

/** Hex with an alpha byte appended, for Monaco's 8-digit colour slots. */
function alpha(hex, byte) {
  return `${hex}${byte}`;
}

const RULES = [
  // Keywords — the accent, and the only place it appears in code.
  { token: "keyword", foreground: SYNTAX.keyword },

  // Types: declared type names, built-in types, and the type position.
  { token: "type", foreground: SYNTAX.type },
  { token: "type.identifier", foreground: SYNTAX.type },
  { token: "entity.name.type", foreground: SYNTAX.type },

  // Identifiers: bindings, functions, and anything the tokenizer could not
  // classify. `entity.name.function` is an identifier that happens to be
  // called; it is not a seventh role.
  { token: "identifier", foreground: SYNTAX.identifier },
  { token: "variable", foreground: SYNTAX.identifier },
  { token: "entity.name.function", foreground: SYNTAX.identifier },

  // Literals: numbers, strings, constants, and the built-in constructors,
  // which are values written where a value goes.
  { token: "number", foreground: SYNTAX.literal },
  { token: "string", foreground: SYNTAX.literal },
  { token: "constant", foreground: SYNTAX.literal },
  { token: "constant.language", foreground: SYNTAX.literal },
  // An escape is still part of the literal; it is marked by weight so a
  // reader can see where one is without a colour being spent on it.
  { token: "string.escape", foreground: SYNTAX.literal, fontStyle: "bold" },
  // An invalid escape is a fault, and faults are the accent's other job.
  { token: "string.escape.invalid", foreground: SYNTAX.keyword, fontStyle: "bold" },

  { token: "operator", foreground: SYNTAX.punctuation },
  { token: "delimiter", foreground: SYNTAX.punctuation },

  { token: "comment", foreground: SYNTAX.comment },
  { token: "comment.doc", foreground: SYNTAX.comment, fontStyle: "italic" },
];

export const theme = {
  base: "vs-dark",
  // Nothing is inherited. `vs-dark`'s own rules carry blues, greens and
  // oranges, and inheriting them would let any token these rules do not
  // name arrive on the page in a colour the board does not contain. With
  // inheritance off, an unnamed token falls back to `editor.foreground` —
  // an identifier grey — which is the right answer for a token that has
  // not been classified.
  inherit: false,
  // Monaco matches a rule to a token by dot-prefix, and the tokenizer's
  // `tokenPostfix` appends ".cnb" to every one — so "keyword" here matches
  // "keyword.cnb". The rules are written without the postfix so the theme
  // stays usable for any Cinnabar-shaped token stream.
  rules: RULES,
  colors: {
    "editor.background": SYNTAX.ground,
    "editor.foreground": SYNTAX.identifier,

    // The gutter carries plate 10's gutter grey, the same one the site's
    // terminal transcripts number their lines with.
    "editorLineNumber.foreground": SURFACE.mute,
    "editorLineNumber.activeForeground": SURFACE.bright,
    "editorGutter.background": SYNTAX.ground,

    "editorCursor.foreground": SURFACE.cinnabar,
    "editor.lineHighlightBackground": SURFACE.panel,
    "editor.lineHighlightBorder": "#00000000",
    "editor.selectionBackground": alpha(SURFACE.cinnabarDeep, "66"),
    "editor.inactiveSelectionBackground": alpha(SURFACE.cinnabarDeep, "33"),
    "editor.selectionHighlightBackground": alpha(SURFACE.cinnabarDeep, "33"),
    "editor.wordHighlightBackground": alpha(SURFACE.cinnabarDeep, "26"),
    "editor.findMatchBackground": alpha(SURFACE.cinnabarDeep, "80"),
    "editor.findMatchHighlightBackground": alpha(SURFACE.cinnabarDeep, "40"),

    "editorIndentGuide.background1": SURFACE.hairline,
    "editorIndentGuide.activeBackground1": SURFACE.hairlineStrong,
    "editorWhitespace.foreground": SURFACE.hairlineStrong,
    "editorRuler.foreground": SURFACE.hairline,

    // Diagnostics. The compiler has one severity, so there is one squiggle.
    "editorError.foreground": SURFACE.cinnabar,
    "editorWarning.foreground": SURFACE.cinnabar,
    "editorInfo.foreground": SURFACE.grey,
    "editorOverviewRuler.errorForeground": SURFACE.cinnabar,
    "editorOverviewRuler.border": "#00000000",

    "editorWidget.background": SURFACE.panel,
    "editorWidget.border": SURFACE.hairline,
    "editorHoverWidget.background": SURFACE.panel,
    "editorHoverWidget.border": SURFACE.hairline,
    "editorSuggestWidget.background": SURFACE.panel,
    "editorSuggestWidget.border": SURFACE.hairline,
    "editorSuggestWidget.selectedBackground": SURFACE.panelRaised,

    "scrollbarSlider.background": alpha(SURFACE.hairlineStrong, "aa"),
    "scrollbarSlider.hoverBackground": SURFACE.hairlineStrong,
    "scrollbarSlider.activeBackground": SURFACE.grey,
    "editorOverviewRuler.background": SYNTAX.ground,

    "editorBracketMatch.background": "#00000000",
    "editorBracketMatch.border": SURFACE.grey,
  },
};

/** Define the theme on a Monaco instance and return its id. */
export function registerCinnabarTheme(monaco) {
  monaco.editor.defineTheme(THEME_ID, theme);
  return THEME_ID;
}
