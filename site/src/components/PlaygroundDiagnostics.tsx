import { motion } from "motion/react";
import Window, { WindowBody } from "@/components/Window";
import { CheckIcon } from "@/components/brand/icons";
import { isClean, locateSpan, type PlaygroundDiagnostic, type PlaygroundReport } from "@/lib/cinnabar-diagnostics";

/*
 * Renders a live `check()` report against the same terminal palette
 * `DiagnosticTranscript` uses (plate 10: vermilion for the error and its
 * primary span, grey for everything else) — but as a plain gutter-and-caret
 * layout rather than DiagnosticTranscript's hand-joined box-drawing rails.
 *
 * That renderer exists to reproduce one fixed, hand-authored transcript
 * character-for-character against ariadne's own output; this one draws an
 * arbitrary, changing span on every keystroke, so it settles for the same
 * palette and a `^^^^` caret run under the span instead of replicating
 * ariadne's box-joining geometry live.
 *
 * Content here can flip on every keystroke, sometimes several times before a
 * debounce settles. `AnimatePresence mode="wait"` was tried for the clean/
 * errors swap and dropped: a key change arriving mid-exit restarts the exit
 * of an element that's already animating out, and a fast enough typist could
 * leave the previous state's element exit-animating forever while the DOM
 * never mounts the current one. `animate`'s key change alone still gives the
 * swap a beat of motion, with no unmount/remount sequencing to race.
 */

function SourceExcerpt({ source, diagnostic }: { source: string; diagnostic: PlaygroundDiagnostic }) {
  const span = diagnostic.source;
  if (!span) return null;
  const located = locateSpan(source, span.start, span.end);
  const gutterLabel = `${located.line}`.padStart(3, " ");
  const caretWidth = Math.max(located.length, 1);

  return (
    <>
      <span className="block">
        <span className="text-term-gutter">{"    ╭─[ "}</span>
        <span className="text-term-flag">
          {span.path}:{located.line}:{located.column}
        </span>
        <span className="text-term-gutter">{" ]"}</span>
      </span>
      <span className="text-term-gutter block">{"    │"}</span>
      <span className="block">
        <span className="text-term-gutter">{` ${gutterLabel} │ `}</span>
        <span className="text-term-flag">{located.lineText}</span>
      </span>
      <span className="block">
        <span className="text-term-gutter">{"     │ "}</span>
        <span className="text-term-error">
          {" ".repeat(located.columnOffset)}
          {"^".repeat(caretWidth)}
        </span>
      </span>
    </>
  );
}

function DiagnosticBlock({ diagnostic, source }: { diagnostic: PlaygroundDiagnostic; source: string }) {
  return (
    <div className="mb-5 last:mb-0">
      <span className="block">
        <span className="text-term-error font-semibold">Error</span>
        <span className="text-term-command font-semibold">: {diagnostic.message}</span>
      </span>
      <SourceExcerpt source={source} diagnostic={diagnostic} />
      {diagnostic.explanations.map((explanation, index) => {
        const span = explanation.source;
        return (
          <span key={index} className="text-term-output mt-1 block pl-4">
            {"↳ "}
            {explanation.message}
            {span ? ` (${span.path}:${locateSpan(source, span.start, span.end).line})` : ""}
          </span>
        );
      })}
    </div>
  );
}

function CleanState() {
  return (
    <span className="text-term-output flex items-center gap-2">
      <CheckIcon size={13} className="text-term-flag" />
      No diagnostics.
    </span>
  );
}

export default function PlaygroundDiagnostics({
  report,
  source,
  path,
  className,
}: {
  report: PlaygroundReport | null;
  source: string;
  /** Shown in the window bar — the synthetic path `check()` was called with. */
  path: string;
  className?: string;
}) {
  const clean = report === null || isClean(report);

  return (
    <Window path={path} title="Diagnostics" className={className}>
      <WindowBody scale="diagnostic">
        <code>
          <motion.span
            key={clean ? "clean" : "errors"}
            initial={{ opacity: 0.4 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.12 }}
            className="block"
          >
            {clean ? (
              <CleanState />
            ) : (
              report?.diagnostics.map((diagnostic, index) => (
                <DiagnosticBlock key={index} diagnostic={diagnostic} source={source} />
              ))
            )}
          </motion.span>
        </code>
      </WindowBody>
    </Window>
  );
}
