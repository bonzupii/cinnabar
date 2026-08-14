"use client";

import CodeMirror from "@uiw/react-codemirror";
import { useEffect, useRef, useState } from "react";
import { EditorView } from "@codemirror/view";
import PlaygroundDiagnostics from "@/components/PlaygroundDiagnostics";
import Window from "@/components/Window";
import { SAMPLES } from "@/content/samples";
import { cinnabarEditorTheme, cinnabarHighlighting } from "@/lib/cinnabar-codemirror";
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

const EXTENSIONS = [cinnabarHighlighting, EditorView.lineWrapping];

// A stable reference, so CodeMirror never mistakes a new render for a
// reconfiguration request the way it would with an inline object literal.
const BASIC_SETUP = {
  lineNumbers: true,
  foldGutter: false,
  highlightActiveLine: false,
  highlightActiveLineGutter: false,
  autocompletion: false,
  closeBrackets: true,
  bracketMatching: true,
};

export default function PlaygroundEditor() {
  const [source, setSource] = useState(SAMPLES[0].code);
  const [report, setReport] = useState<PlaygroundReport | null>(null);
  const latestRequest = useRef(0);
  // `@uiw/react-codemirror`'s `value` prop only seeds the initial document;
  // once the view exists, further changes to `value` from outside the
  // editor's own `onChange` (loading a different sample) don't get pushed
  // back in. Remounting on a key change is the reliable way to actually
  // replace the document, and it's the right behaviour anyway -- loading a
  // different starter program is a reset, undo history included, not an
  // edit.
  const [loadKey, setLoadKey] = useState(0);

  function loadSample(code: string) {
    setSource(code);
    setLoadKey((key) => key + 1);
  }

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
      .catch(() => {
        if (latestRequest.current === requestId) {
          setReport({
            format: "cinnabar.playground-diagnostics.v1",
            diagnostics: [],
            serialization_error: "the checker failed to load",
          });
        }
      });
  }, [source]);

  return (
    <div className="flex flex-col gap-6">
      <div
        role="tablist"
        aria-label="Load a sample"
        className="border-hairline flex flex-wrap border-b"
      >
        {SAMPLES.map((sample) => (
          <button
            key={sample.id}
            type="button"
            role="tab"
            aria-selected={source === sample.code}
            onClick={() => loadSample(sample.code)}
            className="panel-hover text-secondary hover:text-text hover:border-hairline-strong -mb-px border-b-2 border-transparent px-4 py-2.5 text-[12px] font-bold tracking-widest uppercase"
          >
            {sample.label}
          </button>
        ))}
      </div>

      <Window path={ENTRY_PATH} title="Cinnabar source">
        <CodeMirror
          key={loadKey}
          value={source}
          onChange={setSource}
          theme={cinnabarEditorTheme}
          extensions={EXTENSIONS}
          height="24rem"
          basicSetup={BASIC_SETUP}
        />
      </Window>

      <PlaygroundDiagnostics report={report} source={source} path={ENTRY_PATH} />
    </div>
  );
}
