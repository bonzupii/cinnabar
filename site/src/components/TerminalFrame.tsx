import type { ReactNode } from "react";
import CinnabarMark from "@/components/brand/CinnabarMark";

/*
 * A faux terminal.
 *
 * The chrome is plate 10's "Prompt mark": the mark, `cnb`, and the working
 * directory, set in mono. Deliberately not the usual three coloured circles —
 * the identity has no curves outside the LSP dot, and three extra hues would
 * break plate 05's "one accent, five greys, nothing else" on the one surface
 * where the accent already means something specific.
 *
 * The frame is decorative: its label is presentational, and the transcript
 * inside is what carries meaning.
 */
export default function TerminalFrame({
  cwd = "~/src/kernel",
  label,
  children,
  className,
}: {
  /** Shown after the prompt mark. */
  cwd?: string;
  /** Optional right-hand caption — a file path, or what the transcript shows. */
  label?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <figure className={`rule-grid flex min-w-0 flex-col ${className ?? ""}`}>
      <figcaption className="bg-panel flex items-center gap-2.5 px-4 py-2.5">
        <CinnabarMark size={13} letter="var(--text)" />
        <span className="text-text font-mono text-[11px] tracking-[0.06em]">cnb</span>
        <span className="text-label font-mono text-[11px]">{cwd}</span>
        {label ? (
          <span className="text-label ml-auto hidden font-mono text-[10px] tracking-[0.14em] uppercase sm:block">
            {label}
          </span>
        ) : null}
      </figcaption>
      {children}
    </figure>
  );
}

/**
 * The scrollable body of a terminal frame.
 *
 * Focusable, because a region that scrolls horizontally is unreachable by
 * keyboard otherwise.
 */
export function TerminalBody({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <pre
      tabIndex={0}
      className={`bg-code-terminal w-full overflow-x-auto px-6 py-6 font-mono text-[13px] leading-[1.85] sm:text-[14px] ${className ?? ""}`}
    >
      {children}
    </pre>
  );
}
