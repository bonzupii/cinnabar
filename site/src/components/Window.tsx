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
 * The controls are the familiar three, in the brand's own colours rather than
 * the system red/amber/green. Plate 05 allows one accent and five greys, and
 * inventing two more hues for decoration is exactly what plate 14's misuse
 * rules exist to prevent. They are decoration and are hidden from assistive
 * technology — there is no window to close, and offering a control that does
 * nothing is worse than offering none.
 */

function Controls() {
  return (
    <div aria-hidden="true" className="flex flex-none items-center gap-2">
      <span className="bg-[color:var(--hairline-strong)] h-2.5 w-2.5" />
      <span className="bg-[color:var(--grey)] h-2.5 w-2.5" />
      <span className="bg-cinnabar h-2.5 w-2.5" />
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
    <figure className={`rule-grid flex min-w-0 flex-col ${className ?? ""}`}>
      {/*
        The centre column is centred against the bar rather than against the
        space left over beside the path, so the two side tracks are equal
        fractions with the title in an auto track between them.
      */}
      <figcaption className="bg-panel grid grid-cols-[1fr_auto_1fr] items-center gap-3 px-4 py-2.5">
        <span className="flex min-w-0 items-center gap-2.5">
          <CinnabarMark size={13} letter="var(--text)" />
          <span className="text-text truncate font-mono text-[11px] tracking-[0.04em]">
            {path}
          </span>
        </span>
        <span className="text-label min-w-0 truncate text-center font-mono text-[10px] tracking-[0.14em] uppercase">
          {title}
        </span>
        <span className="flex justify-end">
          <Controls />
        </span>
      </figcaption>
      {children}
    </figure>
  );
}

/**
 * The scrollable body of a window.
 *
 * Focusable, because a region that scrolls horizontally is unreachable by
 * keyboard otherwise.
 */
export function WindowBody({
  children,
  tone = "terminal",
  className,
}: {
  children: ReactNode;
  /** `terminal` is the darker ground used for sessions; `code` for source. */
  tone?: "terminal" | "code";
  className?: string;
}) {
  return (
    <pre
      tabIndex={0}
      className={`w-full overflow-x-auto px-6 py-6 font-mono text-[13px] leading-[1.85] sm:text-[14px] ${
        tone === "code" ? "bg-code-ground" : "bg-code-terminal"
      } ${className ?? ""}`}
    >
      {children}
    </pre>
  );
}
