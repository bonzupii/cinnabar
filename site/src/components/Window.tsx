import type { ReactNode } from "react";
import CinnabarMark from "@/components/brand/CinnabarMark";

/*
 * The chrome every dark block shares.
 *
 * One frame for source files, shell transcripts, diagnostics and usage
 * synopses, so a reader never has to work out why two code blocks on the same
 * page look like different kinds of thing.
 *
 * The bar reads left to right: the mark and the path it is showing, then what
 * the block is explaining, then the window controls.
 *
 * The controls are the familiar three, drawn round and glossy the way Tiger
 * drew them, but in the brand's own colours rather than the system
 * red/amber/green. Plate 05 allows one accent and five greys, and inventing
 * two more hues for decoration is exactly what plate 14's misuse rules exist
 * to prevent — so the accent marks the close control and the other two are
 * greys. The gloss itself is white and black at low alpha, which adds no hue.
 *
 * They are decoration and are hidden from assistive technology — there is no
 * window to close, and offering a control that does nothing is worse than
 * offering none.
 */

const CONTROL = "window-control h-2.5 w-2.5 flex-none";

function Controls() {
  return (
    <div aria-hidden="true" className="flex flex-none items-center gap-2">
      <span className={`${CONTROL} bg-hairline-strong`} />
      <span className={`${CONTROL} bg-grey`} />
      <span className={`${CONTROL} bg-cinnabar`} />
    </div>
  );
}

export type WindowProps = {
  /** Left of the bar, beside the mark — a file path or working directory. */
  path: string;
  /**
   * Centred: what the block is showing. Required, so no window can ship with
   * an empty middle — each component supplies a default for its own kind.
   */
  title: string;
  children: ReactNode;
  className?: string;
};

export default function Window({ path, title, children, className }: WindowProps) {
  return (
    <figure
      /*
       * Marks this figure as window chrome. Not every <figure> on the site is
       * a window — a diagram drawn in a repository document is a figure too —
       * and tests/e2e/window.spec.ts asserts the bar's three slots, which only
       * these have.
       */
      data-window=""
      className={`rule-grid window-frame flex min-w-0 flex-col ${className ?? ""}`}
    >
      {/*
        The centre column is centred against the bar rather than against the
        space left over beside the path, so the two side tracks are equal
        fractions with the title in an auto track between them.

        Both side tracks are clamped with minmax(0,1fr): a grid track's default
        minimum is its content's, so a long path would widen the bar past the
        frame on a phone instead of being truncated inside it.
      */}
      <figcaption className="bg-panel grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-2.5 px-3.5 py-2.5 sm:gap-3 sm:px-4">
        <span className="flex min-w-0 items-center gap-2.5">
          <CinnabarMark size={13} letter="var(--text)" />
          <span className="text-text truncate font-mono text-[11px] tracking-[0.04em]">
            {path}
          </span>
        </span>
        <span className="text-label min-w-0 truncate text-center font-mono text-[10px] tracking-[0.14em] uppercase">
          {title}
        </span>
        <span className="flex min-w-0 justify-end">
          <Controls />
        </span>
      </figcaption>
      {children}
    </figure>
  );
}

/**
 * The type treatment of a window body, by what the block is showing.
 *
 * These are a closed set of named settings rather than something a caller
 * passes through `className`, and the reason is a bug that took three attempts
 * to land: a caller writing `leading-[1.2]` alongside the base
 * `leading-[1.85]` produces two arbitrary values of the same property with
 * equal specificity, so which one wins depends on the order Tailwind happens
 * to emit them in — not on the order of the class string. The override
 * silently did nothing, and the fix at the call site was an `!important` that
 * the next person to pass a spacing class would have had to rediscover.
 * Choosing from this table means exactly one size and one leading are ever
 * emitted.
 *
 * `diagnostic` is the tight one: ariadne draws its rails from box-drawing
 * glyphs, which tile vertically only when the line box is about the height of
 * the glyph. At 1.85 the `│` runs are broken by half a line of gap and the box
 * never closes. 1.2 is roughly what a terminal uses, which is what the output
 * was drawn against.
 */
const BODY_TYPE = {
  /** Shell transcripts — the default. */
  terminal: "text-[13px] leading-[1.85] sm:text-[14px]",
  /** Cinnabar source, set a step larger. */
  source: "text-[13.5px] leading-[1.75] sm:text-[15px]",
  /** A `--help` synopsis: transcript size, a little tighter. */
  usage: "text-[13px] leading-[1.75] sm:text-[14px]",
  /** Plain text with no theme — a config file, an unlabelled fenced block. */
  plain: "text-[12.5px] leading-[1.7]",
  /** Compiler diagnostics, at terminal leading so the rails join up. */
  diagnostic: "text-[12.5px] leading-[1.75] sm:text-[13.5px]",
} as const;

export type WindowBodyScale = keyof typeof BODY_TYPE;

/**
 * The scrollable body of a window.
 *
 * Focusable, because a region that scrolls horizontally is unreachable by
 * keyboard otherwise.
 */
export function WindowBody({
  children,
  tone = "terminal",
  scale = "terminal",
  className,
}: {
  children: ReactNode;
  /** `terminal` is the darker ground used for sessions; `code` for source. */
  tone?: "terminal" | "code";
  /**
   * Size and leading, chosen by what the block shows. Pass this rather than a
   * `text-*` or `leading-*` class in `className`, which cannot reliably win.
   */
  scale?: WindowBodyScale;
  className?: string;
}) {
  return (
    <pre
      tabIndex={0}
      /*
       * The gutter narrows on a phone: at 390px, 24px of padding either side
       * costs a tenth of the line before any code is shown.
       *
       * `flex-1` is load-bearing rather than cosmetic. A window placed in a
       * grid or flex row is stretched to the height of the tallest column, but
       * the bar and this body only add up to their own content — and the gap
       * was filled by the frame's own background, which `.rule-grid` paints in
       * `--hairline` so its 1px seams show as rules. The result was a grey
       * band across the bottom of the frame. Growing the body means the
       * terminal ground always reaches the frame's inner edge, for every
       * window rather than for the one that was reported.
       */
      className={`w-full flex-1 overflow-x-auto px-4 py-5 font-mono sm:px-6 sm:py-6 ${
        BODY_TYPE[scale]
      } ${tone === "code" ? "bg-code-ground" : "bg-code-terminal"} ${
        className ?? ""
      }`}
    >
      {children}
    </pre>
  );
}
