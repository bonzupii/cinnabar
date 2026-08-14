import type { ReactNode } from "react";

/*
 * A collapsible section.
 *
 * Built on <details>/<summary> rather than on a button and a piece of state,
 * for two reasons that both matter more than the styling freedom a custom
 * toggle would buy:
 *
 * - The content is in the DOM whether the section is open or not, so a crawler
 *   and a reader without JavaScript get the whole document. A JS toggle that
 *   unmounts its children hides them from both.
 * - The open/closed state, the keyboard handling (Enter and Space) and the
 *   announcement of "expanded"/"collapsed" are the browser's, not ours. Every
 *   hand-rolled version of this has to reimplement all three, and usually gets
 *   the announcement wrong.
 *
 * The marker is a plus turning into a minus, drawn from two straight strokes:
 * plate 07 builds the icon set from squares, diamonds and lines at 34°, 45° or
 * 90°, and allows exactly one curve in the whole set. A chevron would have
 * needed either a curve or a third angle.
 */

export type DisclosureProps = {
  /** The always-visible label. Set in mono uppercase, like every other rule. */
  summary: string;
  children: ReactNode;
  /** Renders the section already open. Off by default. */
  defaultOpen?: boolean;
  className?: string;
};

export default function Disclosure({
  summary,
  children,
  defaultOpen,
  className,
}: DisclosureProps) {
  return (
    <details
      /*
       * `open` is set once, at mount. React has no `defaultOpen` for <details>,
       * and passing `open` as a live prop would fight the browser every time
       * the reader toggled it — but these are server-rendered and never
       * re-render with a different value, so the attribute behaves as the
       * initial state it is meant to be.
       */
      open={defaultOpen || undefined}
      data-disclosure=""
      className={`border-hairline group border ${className ?? ""}`}
    >
      <summary
        /*
         * `list-none` plus the WebKit pseudo-element removes the native
         * triangle in every engine; the marker below replaces it. The cursor
         * is set in both states, because a summary is a control whether the
         * section is open or closed.
         */
        className="panel-hover text-text hover:text-cinnabar-text hover:bg-panel flex min-w-0 cursor-pointer list-none items-center gap-4 px-5 py-4 font-mono text-[11px] tracking-[0.16em] uppercase select-none marker:content-none focus-visible:-outline-offset-2 sm:px-6 sm:py-5 [&::-webkit-details-marker]:hidden"
      >
        <span
          aria-hidden="true"
          className="border-hairline-strong group-hover:border-cinnabar panel-hover relative inline-flex h-4 w-4 flex-none items-center justify-center border"
        >
          {/* The horizontal stroke stays; the vertical one is removed when the
              section opens, turning the plus into a minus. */}
          <span className="bg-cinnabar absolute h-[1.5px] w-2" />
          <span className="bg-cinnabar absolute h-2 w-[1.5px] transition-opacity duration-150 ease-out group-open:opacity-0 motion-reduce:transition-none" />
        </span>
        <span className="min-w-0">{summary}</span>
      </summary>
      {/* The rule and the padding live on the content, so a closed section
          collapses to exactly the height of its own label. */}
      <div className="border-hairline min-w-0 border-t p-5 sm:p-6">{children}</div>
    </details>
  );
}
