"use client";

import CodeMirror from "@uiw/react-codemirror";
import { EditorView } from "@codemirror/view";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import PlaygroundDiagnostics from "@/components/PlaygroundDiagnostics";
import Window from "@/components/Window";
import { PLAYGROUND_SAMPLES } from "@/content/playground-samples";
import {
  cinnabarEditorTheme,
  cinnabarHighlighting,
  cinnabarHoverTooltip,
  cinnabarLineHover,
  cinnabarTokenHover,
  EDITOR_FONT_SIZE,
  EDITOR_LINE_HEIGHT,
  EDITOR_PADDING_BLOCK,
  highlightLine,
} from "@/lib/cinnabar-codemirror";
import type { PlaygroundReport } from "@/lib/cinnabar-diagnostics";
import { checkSource, preloadChecker } from "@/lib/cinnabar-wasm-client";

/**
 * The synthetic path every submission is checked against — never a real
 * file. `crates/cinnabar-wasm::check` feeds this into `analysis::analyze`'s
 * in-memory overlay, which the compiler's own `module_loader::read_source`
 * always resolves from that overlay before it would fall back to a real
 * filesystem read.
 */
const ENTRY_PATH = "playground.cnb";

/*
 * Full mode does not wrap because `PlaygroundLineNumbers` renders one row per
 * logical line. Embedded mode omits that custom gutter and wraps long lines,
 * which keeps the homepage demo on one scroll axis at narrow widths.
 */
const EXTENSIONS = [cinnabarHighlighting, cinnabarLineHover, cinnabarTokenHover, cinnabarHoverTooltip];
const EMBEDDED_EXTENSIONS = [...EXTENSIONS, EditorView.lineWrapping];

// A stable reference, so CodeMirror never mistakes a new render for a
// reconfiguration request the way it would with an inline object literal.
const BASIC_SETUP = {
  lineNumbers: false,
  foldGutter: false,
  highlightActiveLine: false,
  highlightActiveLineGutter: false,
  autocompletion: false,
  closeBrackets: true,
  bracketMatching: true,
};

/**
 * A plain-CSS line-number column, standing in for CodeMirror's own
 * `lineNumbers` gutter extension -- see the comment on the font-metric
 * constants in `cinnabar-codemirror.ts` for why, and for the constraint
 * this places on how long a `PLAYGROUND_SAMPLES` entry can be.
 *
 * Hovering a row highlights its line in the editor (`cinnabarLineHover` in
 * `cinnabar-codemirror.ts`), the same way an IDE's gutter does -- reading
 * that as "point at a line" only works because the numbers already line up
 * exactly with the rows beside them.
 */
function PlaygroundLineNumbers({
  lineCount,
  onHoverLine,
}: {
  lineCount: number;
  onHoverLine: (line: number | null) => void;
}) {
  return (
    <div
      className="text-term-gutter bg-code-terminal flex-none pr-5 pl-6 text-right select-none"
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: EDITOR_FONT_SIZE,
        lineHeight: EDITOR_LINE_HEIGHT,
        paddingTop: EDITOR_PADDING_BLOCK,
        paddingBottom: EDITOR_PADDING_BLOCK,
      }}
      onMouseLeave={() => onHoverLine(null)}
    >
      {Array.from({ length: lineCount }, (_, index) => (
        <div key={index} onMouseEnter={() => onHoverLine(index + 1)}>
          {index + 1}
        </div>
      ))}
    </div>
  );
}

export type PlaygroundEditorProps =
  | { mode?: "full" }
  | { mode: "embedded"; initialSource: string };

export default function PlaygroundEditor(props: PlaygroundEditorProps = {}) {
  const embedded = props.mode === "embedded";
  const initialSource = embedded ? props.initialSource : PLAYGROUND_SAMPLES[0].code;
  const [source, setSource] = useState(initialSource);
  const [report, setReport] = useState<PlaygroundReport | null>(null);
  const latestRequest = useRef(0);
  const viewRef = useRef<EditorView | null>(null);
  // `@uiw/react-codemirror`'s `value` prop only seeds the initial document;
  // once the view exists, further changes to `value` from outside the
  // editor's own `onChange` (loading a different sample) don't get pushed
  // back in. Remounting on a key change is the reliable way to actually
  // replace the document, and it's the right behaviour anyway -- loading a
  // different starter program is a reset, undo history included, not an
  // edit.
  const [loadKey, setLoadKey] = useState(0);
  // Tracked separately from `source`: once a visitor edits the loaded
  // sample, `source` no longer matches any entry's `code` verbatim, but the
  // tab they started from -- and its summary -- should stay selected rather
  // than have every tab go quiet.
  const [selectedSampleId, setSelectedSampleId] = useState(PLAYGROUND_SAMPLES[0].id);
  const selectedSample =
    PLAYGROUND_SAMPLES.find((sample) => sample.id === selectedSampleId) ?? PLAYGROUND_SAMPLES[0];
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  function loadSample(sample: (typeof PLAYGROUND_SAMPLES)[number]) {
    setSource(sample.code);
    setSelectedSampleId(sample.id);
    setLoadKey((key) => key + 1);
  }

  // Roving tabindex implements the WAI-ARIA tabs keyboard contract for the
  // full playground. Embedded mode renders no tablist and never enters this
  // path.
  function onTabKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    let next = -1;
    if (event.key === "ArrowRight") next = (index + 1) % PLAYGROUND_SAMPLES.length;
    else if (event.key === "ArrowLeft") {
      next = (index - 1 + PLAYGROUND_SAMPLES.length) % PLAYGROUND_SAMPLES.length;
    } else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = PLAYGROUND_SAMPLES.length - 1;
    if (next === -1) return;
    event.preventDefault();
    const sample = PLAYGROUND_SAMPLES[next];
    loadSample(sample);
    tabRefs.current[next]?.focus();
  }

  function onHoverLine(line: number | null) {
    if (viewRef.current) highlightLine(viewRef.current, line);
  }

  const lineCount = source.split("\n").length;

  // Warms the wasm module on mount rather than on the first edit, so typing
  // never pays its load latency.
  useEffect(() => {
    preloadChecker();
  }, []);

  useEffect(() => {
    const requestId = (latestRequest.current += 1);
    checkSource(source)
      .then((result) => {
        if (latestRequest.current === requestId) setReport(result);
      })
      .catch((error: unknown) => {
        if (latestRequest.current === requestId) {
          const message = error instanceof Error ? error.message : "the checker failed to load";
          setReport({
            format: "cinnabar.playground-diagnostics.v1",
            diagnostics: [],
            serialization_error: message,
          });
        }
      });
  }, [source]);

  return (
    <div className="flex flex-col gap-6">
      {!embedded ? <p className="text-secondary text-[15px] leading-[1.7] text-pretty">
        The tabs below load six starter programs, each a complete, self-contained example rather
        than an excerpt. Load one, then edit it: every keystroke is checked, live, by the same
        front end the real compiler runs.
      </p> : null}

      {!embedded ? <div>
        <div
          role="tablist"
          aria-label="Load a sample"
          className="border-hairline flex flex-wrap border-b"
        >
          {PLAYGROUND_SAMPLES.map((sample, index) => {
            const Icon = sample.icon;
            const active = sample.id === selectedSampleId;
            return (
              <button
                key={sample.id}
                ref={(element) => {
                  tabRefs.current[index] = element;
                }}
                id={`playground-tab-${sample.id}`}
                type="button"
                role="tab"
                aria-selected={active}
                aria-controls="playground-tabpanel"
                tabIndex={active ? 0 : -1}
                onClick={() => loadSample(sample)}
                onKeyDown={(event) => onTabKeyDown(event, index)}
                className={`panel-hover -mb-px flex items-center gap-2 border-b-2 px-4 py-2.5 text-[12px] font-bold tracking-widest uppercase ${
                  active
                    ? "border-cinnabar text-text"
                    : "text-secondary hover:text-text hover:border-hairline-strong border-transparent"
                }`}
              >
                <Icon size={14} />
                {sample.label}
              </button>
            );
          })}
        </div>
      </div> : null}

      <div
        role={embedded ? undefined : "tabpanel"}
        id={embedded ? undefined : "playground-tabpanel"}
        aria-labelledby={embedded ? undefined : `playground-tab-${selectedSampleId}`}
        tabIndex={embedded ? undefined : 0}
        className={embedded ? undefined : "flex flex-col gap-6"}
      >
        {!embedded ? (
          <p className="text-secondary text-[14px] leading-[1.6] text-pretty">
            {selectedSample.summary}
          </p>
        ) : null}

      {/*
        One frame for the whole tool: diagnostics dock directly under the
        titlebar, above the source, the way an IDE's problems panel sits in
        the same pane as the file it reports on rather than in a second
        window beside it.
      */}
      <Window path={ENTRY_PATH} title="Cinnabar source">
        <PlaygroundDiagnostics report={report} source={source} />
        <div className={`bg-code-ground flex ${embedded ? "max-h-112 overflow-hidden" : ""}`}>
          {!embedded ? (
            <PlaygroundLineNumbers lineCount={lineCount} onHoverLine={onHoverLine} />
          ) : null}
          <CodeMirror
            key={loadKey}
            className="min-w-0 flex-1"
            value={source}
            onChange={setSource}
            theme={cinnabarEditorTheme}
            extensions={embedded ? EMBEDDED_EXTENSIONS : EXTENSIONS}
            maxHeight={embedded ? "28rem" : undefined}
            minHeight={embedded ? undefined : "24rem"}
            aria-label={embedded ? "Editable Cinnabar source" : "Cinnabar source"}
            basicSetup={BASIC_SETUP}
            onCreateEditor={(view) => {
              viewRef.current = view;
              view.contentDOM.setAttribute(
                "aria-label",
                embedded ? "Editable Cinnabar source" : "Cinnabar source",
              );
              view.scrollDOM.tabIndex = 0;
              view.scrollDOM.setAttribute("aria-label", "Cinnabar source editor");
            }}
          />
        </div>
      </Window>
      </div>
    </div>
  );
}
