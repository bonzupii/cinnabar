import { Prec, RangeSetBuilder, StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, hoverTooltip, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import { hoverAt } from "@/lib/cinnabar-wasm-client";
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
 * Highlights one line (1-indexed), or clears the highlight for `null`. Driven
 * by `PlaygroundLineNumbers` hovering its own gutter rows -- since those
 * rows are hand-drawn rather than CodeMirror's own gutter, there's no
 * built-in hover-to-highlight behaviour to inherit, so this reimplements
 * just that one piece as a small `StateField` the gutter dispatches into.
 */
export const setHighlightedLine = StateEffect.define<number | null>();

export const cinnabarLineHover = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(decorations, transaction) {
    for (const effect of transaction.effects) {
      if (effect.is(setHighlightedLine)) {
        if (effect.value === null) return Decoration.none;
        const line = transaction.state.doc.line(
          Math.min(effect.value, transaction.state.doc.lines),
        );
        return Decoration.set([Decoration.line({ class: "cm-hovered-line" }).range(line.from)]);
      }
    }
    return decorations.map(transaction.changes);
  },
  provide: (field) => EditorView.decorations.from(field),
});

/** Dispatches a `setHighlightedLine` effect against a live editor view. */
export function highlightLine(view: EditorView, line: number | null) {
  view.dispatch({ effects: setHighlightedLine.of(line) });
}

/**
 * Highlights whatever token the pointer is over -- keywords included, which
 * is the reason this cannot just piggyback on `cinnabarHoverTooltip`'s own
 * hover tracking: `analysis::hover` (`hoverAt`) only ever resolves symbols
 * (identifiers, calls, types), so a keyword or a piece of punctuation never
 * gets a tooltip and would never light up if this reused that plugin's
 * position. This re-tokenizes with the same `tokenizeCinnabar` the highlighter
 * itself uses and highlights whichever token the cursor's document offset
 * falls inside, independent of what (if anything) has hover info attached.
 */
const setHoveredToken = StateEffect.define<{ from: number; to: number } | null>();

const cinnabarTokenHoverField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(decorations, transaction) {
    for (const effect of transaction.effects) {
      if (effect.is(setHoveredToken)) {
        if (effect.value === null) return Decoration.none;
        const { from, to } = effect.value;
        if (from === to) return Decoration.none;
        return Decoration.set([Decoration.mark({ class: "cm-hovered-token" }).range(from, to)]);
      }
    }
    return decorations.map(transaction.changes);
  },
  provide: (field) => EditorView.decorations.from(field),
});

function tokenAt(source: string, pos: number): { from: number; to: number } | null {
  let offset = 0;
  for (const token of tokenizeCinnabar(source)) {
    const end = offset + token.value.length;
    if (token.kind !== "text" && pos >= offset && pos < end) return { from: offset, to: end };
    offset = end;
  }
  return null;
}

export const cinnabarTokenHover = [
  cinnabarTokenHoverField,
  EditorView.domEventHandlers({
    mousemove(event, view) {
      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
      const span = pos === null ? null : tokenAt(view.state.doc.toString(), pos);
      view.dispatch({ effects: setHoveredToken.of(span) });
    },
    mouseleave(_event, view) {
      view.dispatch({ effects: setHoveredToken.of(null) });
    },
  }),
];

/**
 * Font metrics shared with `PlaygroundLineNumbers`'s hand-drawn gutter in
 * `PlaygroundEditor.tsx`, which stands in for CodeMirror's own `lineNumbers`
 * gutter extension.
 *
 * The built-in gutter positions each row using a line height it measures
 * once internally and caches. In practice that cached number came out
 * shorter than the real, CSS-rendered row height of `.cm-content`'s lines --
 * confirmed live, independent of line wrapping, and it persisted even after
 * fixing the separate CSS-specificity bug below and forcing a fresh
 * `view.requestMeasure()` once fonts had loaded. Nothing reachable from
 * outside CodeMirror recalibrates that cache, so the built-in gutter is
 * disabled and drawn by hand instead, sharing these exact values with
 * `.cm-content` -- no second measurement to fall out of sync with the first.
 *
 * The trade-off: a hand-drawn gutter always renders one row per line the
 * source has, but CodeMirror only *renders* lines within (or near) its
 * viewport for performance, so a document long enough to leave lines
 * unrendered below the fold makes the two columns' row counts genuinely
 * stop matching -- not just look misaligned. Every `PLAYGROUND_SAMPLES`
 * entry (`content/playground-samples.ts`) is kept short enough to render in
 * full on this page without scrolling, which is what keeps this safe in
 * practice; it is not a fix for arbitrarily long pasted or typed input.
 */
export const EDITOR_FONT_SIZE = "13.5px";
export const EDITOR_LINE_HEIGHT = "1.75";
export const EDITOR_PADDING_BLOCK = "1.25rem";
export const EDITOR_PADDING_INLINE = "1.5rem";

/**
 * The editor's chrome, matched to `WindowBody`'s `tone="code" scale="source"`
 * treatment (`CodeBlock.tsx`) so the editable pane sits at rest looking like
 * every other source block on the site. CodeMirror needs a real container
 * rather than `WindowBody`'s `<pre>`, so this theme restates that treatment
 * directly against the same CSS custom properties instead of reusing the
 * Tailwind classes, which a `<pre>`-only component doesn't expose here.
 */
const baseEditorTheme = EditorView.theme(
  {
    "&": {
      backgroundColor: "var(--code-ground)",
      color: "var(--syn-identifier)",
    },
    ".cm-content": {
      padding: `${EDITOR_PADDING_BLOCK} ${EDITOR_PADDING_INLINE}`,
      caretColor: "var(--cinnabar-text)",
      // Set directly rather than left to inherit from `&`: CodeMirror's own
      // base styling declares font-family and line-height straight on
      // `.cm-content` too, and an explicit declaration on the element beats
      // an inherited one regardless of either rule's specificity. That's
      // what silently won before -- computed style showed the generic
      // "monospace" family and a 1.4 line-height no matter what this theme
      // set at the root, and `.cm-content`'s real per-line row height came
      // out shorter than `EDITOR_LINE_HEIGHT` as a result, which is what
      // threw off the line-number gutter positioned against it.
      fontFamily: "var(--font-mono)",
      fontSize: EDITOR_FONT_SIZE,
      lineHeight: EDITOR_LINE_HEIGHT,
    },
    ".cm-line": { padding: 0 },
    "&.cm-focused": { outline: "none" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--cinnabar-text)" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
      backgroundColor: "color-mix(in srgb, var(--cinnabar) 28%, transparent)",
    },
    ".cm-activeLine": { backgroundColor: "transparent" },
    // Same low-alpha accent the selection background uses (`.cm-focused
    // .cm-selectionBackground` above) at a fraction of its strength, so a
    // hovered line reads as "pointed at" rather than "selected".
    ".cm-hovered-line": {
      backgroundColor: "color-mix(in srgb, var(--cinnabar) 10%, transparent)",
    },
    // A stronger mix than the line highlight above: this marks a single
    // token rather than a whole row, so it needs to read at a glance.
    ".cm-hovered-token": {
      backgroundColor: "color-mix(in srgb, var(--cinnabar) 22%, transparent)",
      borderRadius: "3px",
    },
  },
  { dark: true },
);

// `Prec.high` because the plain declarations above turned out not to be
// enough on their own: CodeMirror's bundled styling and this theme's rules
// can carry equal CSS specificity, and without an explicit precedence the
// one that happens to load second in the stylesheet wins -- which was, in
// practice, not this one.
export const cinnabarEditorTheme = Prec.high(baseEditorTheme);

/** One rendered `label: value` metadata line -- `type: ...` / `linear: ...`. */
function appendMetaLine(container: HTMLElement, paragraph: string): void {
  const line = document.createElement("div");
  line.style.display = "flex";
  line.style.gap = "0.4em";

  const labelMatch = /^([A-Za-z ]+):\s*/.exec(paragraph);
  const rest = labelMatch ? paragraph.slice(labelMatch[0].length) : paragraph;
  if (labelMatch) {
    const label = document.createElement("span");
    label.textContent = `${labelMatch[1]}:`;
    label.style.color = "var(--term-gutter)";
    label.style.flex = "none";
    line.appendChild(label);
  }

  // Splits on **bold** and `code` without consuming the delimiters, so
  // odd-indexed pieces are exactly the marked-up runs in source order.
  for (const piece of rest.split(/(\*\*[^*]+\*\*|`[^`]+`)/)) {
    if (piece.startsWith("**") && piece.endsWith("**")) {
      const strong = document.createElement("strong");
      strong.textContent = piece.slice(2, -2);
      line.appendChild(strong);
    } else if (piece.startsWith("`") && piece.endsWith("`")) {
      const code = document.createElement("code");
      code.textContent = piece.slice(1, -1);
      code.style.color = "var(--syn-type)";
      line.appendChild(code);
    } else if (piece) {
      line.appendChild(document.createTextNode(piece));
    }
  }
  container.appendChild(line);
}

/**
 * Renders `hoverAt`'s text as an IDE-style hover card: a syntax-highlighted
 * signature row (VS Code's own hover puts the declaration first, on the
 * editor's own code background), a hairline divider, then a metadata row in
 * a slightly darker tone for whatever else is attached -- the resolved
 * `type:` and, for a linear handle, `linear: ...`.
 *
 * `hoverAt`'s text is `analysis::hover`'s own shape (a fenced ```cinnabar
 * block, then `**kind** name`, `type: ...`, `linear: ...` lines joined by
 * blank lines -- see `crates/cinnabar-wasm/src/lib.rs`), not markdown in
 * general, so this only ever needs to handle that one shape rather than
 * write a markdown parser for it.
 */
function renderHoverCard(container: HTMLElement, text: string): void {
  const paragraphs = text.split("\n\n");
  const meta: string[] = [];

  for (const paragraph of paragraphs) {
    const fenced = /^```cinnabar\n([\s\S]*?)\n```$/.exec(paragraph);
    if (!fenced) {
      meta.push(paragraph);
      continue;
    }
    const code = document.createElement("div");
    code.style.padding = "0.55rem 0.85rem";
    code.style.backgroundColor = "var(--code-ground)";
    code.style.whiteSpace = "pre-wrap";
    for (const token of tokenizeCinnabar(fenced[1])) {
      const span = document.createElement("span");
      span.textContent = token.value;
      const className = TOKEN_STYLE[token.kind];
      if (className) span.className = className;
      code.appendChild(span);
    }
    container.appendChild(code);
  }

  if (meta.length === 0) return;
  const metaSection = document.createElement("div");
  metaSection.style.borderTop = "1px solid var(--hairline)";
  metaSection.style.padding = "0.5rem 0.85rem";
  metaSection.style.display = "flex";
  metaSection.style.flexDirection = "column";
  metaSection.style.gap = "0.3rem";
  metaSection.style.fontSize = "12px";
  metaSection.style.color = "var(--term-output)";
  for (const paragraph of meta) appendMetaLine(metaSection, paragraph);
  container.appendChild(metaSection);
}

/**
 * Hover tooltips over resolved signatures, canonical types and linearity --
 * `cinnabar::analysis::hover` through `hoverAt` (`cinnabar-wasm-client.ts`),
 * the exact query the language server answers, so a playground hover can
 * never show something the LSP wouldn't.
 *
 * Styled with inline styles rather than through `cinnabarEditorTheme`:
 * CodeMirror mounts tooltips as a sibling of `.cm-editor`, outside the
 * subtree `EditorView.theme()`'s generated class actually scopes to, so a
 * rule written there would silently never match here. The CSS custom
 * properties themselves (`var(--code-terminal)` etc.) still resolve
 * correctly from any point in the document, which is what makes plain
 * inline styles work regardless of where CodeMirror ends up mounting this.
 */
export const cinnabarHoverTooltip = hoverTooltip(async (view, pos) => {
  const source = view.state.doc.toString();
  const result = await hoverAt(source, pos);
  if (!result) return null;
  const from = result.source?.start ?? pos;
  const to = result.source?.end ?? pos;

  return {
    pos: from,
    end: to,
    above: true,
    create: () => {
      const dom = document.createElement("div");
      dom.style.maxWidth = "34rem";
      dom.style.overflow = "hidden";
      dom.style.borderRadius = "6px";
      dom.style.border = "1px solid var(--hairline-strong)";
      // Two-layer shadow -- a tight contact shadow plus a soft ambient one --
      // is what actually reads as "floating card" rather than "outlined box";
      // a single flat shadow looks pasted on regardless of its blur radius.
      dom.style.boxShadow = "0 2px 6px rgba(0, 0, 0, 0.35), 0 12px 28px rgba(0, 0, 0, 0.35)";
      dom.style.backgroundColor = "var(--code-terminal)";
      dom.style.color = "var(--syn-identifier)";
      dom.style.fontFamily = "var(--font-mono)";
      dom.style.fontSize = "12.5px";
      dom.style.lineHeight = "1.6";
      renderHoverCard(dom, result.text);
      return { dom };
    },
  };
});
