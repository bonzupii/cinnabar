import { RangeSetBuilder } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import { TOKEN_STYLE, tokenizeCinnabar } from "@/lib/cinnabar-syntax";

/*
 * Live syntax highlighting for the playground editor, built on the same
 * `tokenizeCinnabar` shape-based highlighter `CodeBlock` uses for the static
 * samples — a `ViewPlugin` re-tokenizes the whole document on every change
 * and paints one `Decoration.mark` per token, rather than a Lezer grammar.
 * That is the same trade `cinnabar-syntax.ts` itself documents: it never
 * needs to be right about meaning, only about which of the seven categories
 * plate 09 colours a token falls in, and a playground snippet is short
 * enough that re-tokenizing on every keystroke costs nothing worth avoiding.
 */

function buildDecorations(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  let offset = 0;
  for (const token of tokenizeCinnabar(view.state.doc.toString())) {
    const className = TOKEN_STYLE[token.kind];
    if (className) {
      builder.add(offset, offset + token.value.length, Decoration.mark({ class: className }));
    }
    offset += token.value.length;
  }
  return builder.finish();
}

export const cinnabarHighlighting = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildDecorations(view);
    }

    update(update: ViewUpdate) {
      if (update.docChanged) this.decorations = buildDecorations(update.view);
    }
  },
  { decorations: (instance) => instance.decorations },
);

/**
 * The editor's chrome, matched to `WindowBody`'s `tone="code" scale="source"`
 * treatment (`CodeBlock.tsx`) so the editable pane sits at rest looking like
 * every other source block on the site. CodeMirror needs a real container
 * rather than `WindowBody`'s `<pre>`, so this theme restates that treatment
 * directly against the same CSS custom properties instead of reusing the
 * Tailwind classes, which a `<pre>`-only component doesn't expose here.
 */
export const cinnabarEditorTheme = EditorView.theme(
  {
    "&": {
      backgroundColor: "var(--code-ground)",
      color: "var(--syn-identifier)",
      height: "100%",
    },
    ".cm-content": {
      padding: "1.25rem 1.5rem",
      caretColor: "var(--cinnabar-text)",
      fontFamily: "var(--font-mono)",
      fontSize: "13.5px",
      lineHeight: "1.75",
    },
    ".cm-line": { padding: 0 },
    "&.cm-focused": { outline: "none" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--cinnabar-text)" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
      backgroundColor: "color-mix(in srgb, var(--cinnabar) 28%, transparent)",
    },
    ".cm-activeLine": { backgroundColor: "transparent" },
    ".cm-gutters": {
      backgroundColor: "var(--code-ground)",
      color: "var(--term-gutter)",
      border: "none",
    },
    ".cm-activeLineGutter": { backgroundColor: "transparent" },
    ".cm-lineNumbers .cm-gutterElement": {
      padding: "0 1rem 0 1.25rem",
      fontSize: "12px",
    },
  },
  { dark: true },
);
