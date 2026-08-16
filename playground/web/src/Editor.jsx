// The Monaco editor, with the compiler's diagnostics drawn onto it.
//
// Markers come from the `--emit-json` diagnostic envelope, which carries
// both byte offsets and the line/UTF-16-column pair Monaco wants — mapped
// by the compiler through the same function the language server uses. So a
// squiggle here is in the same place the editor extension would put it, and
// neither had to compute a position from the other's.

import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
// The editor core only. The default entry point pulls every language
// Monaco ships with — TypeScript, JSON, HTML, and forty others — none of
// which this page can open. Importing the API directly leaves a bundle
// holding one language: Cinnabar.
import * as monaco from "monaco-editor/esm/vs/editor/editor.api";
import { registerCinnabar } from "cinnabar-monaco";
import { registerCinnabarTheme } from "./monacoTheme.js";

const languageId = registerCinnabar(monaco);
const themeId = registerCinnabarTheme(monaco);

const Editor = forwardRef(function Editor({ value, onChange, diagnostics }, ref) {
  const host = useRef(null);
  const editor = useRef(null);

  useEffect(() => {
    if (!host.current || editor.current) {
      return undefined;
    }
    editor.current = monaco.editor.create(host.current, {
      value,
      language: languageId,
      // The split is draggable, so this editor's width changes without the
      // window's ever doing so. `automaticLayout` watches the container for
      // exactly that.
      automaticLayout: true,
      minimap: { enabled: false },
      fontLigatures: false,
      scrollBeyondLastLine: false,
      renderWhitespace: "none",
      tabSize: 2,
      // Plate 09 is specified against the dark ground and plate 05 keeps the
      // screen system dark, so the code surface does not follow the
      // visitor's light/dark preference — the same rule the site applies to
      // its own `<pre>` blocks.
      theme: themeId,
      fontFamily: '"IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      fontSize: 14,
      lineHeight: 26,
      // The code gets the same air the panels around it do: a top inset so
      // the first line is not against the tab strip, and a gap between the
      // line numbers and the text so the gutter reads as a margin rather
      // than as a column pressed up against the code.
      padding: { top: 26, bottom: 40 },
      lineNumbersMinChars: 3,
      lineDecorationsWidth: 20,
      glyphMargin: false,
      smoothScrolling: true,
      cursorBlinking: "smooth",
      roundedSelection: false,
      overviewRulerBorder: false,
      scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10, useShadows: false },
      guides: { indentation: true, highlightActiveIndentation: false },
    });
    const instance = editor.current;
    const subscription = instance.onDidChangeModelContent(() => {
      onChange(instance.getValue());
    });
    return () => {
      subscription.dispose();
      instance.dispose();
      editor.current = null;
    };
    // Created once. `value` is applied below when it changes from outside,
    // which is what choosing an example does; re-creating the editor for
    // every keystroke would lose the cursor.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const instance = editor.current;
    if (instance && instance.getValue() !== value) {
      instance.setValue(value);
    }
  }, [value]);

  useEffect(() => {
    const instance = editor.current;
    if (!instance) {
      return;
    }
    const model = instance.getModel();
    if (!model) {
      return;
    }
    const markers = [];
    for (const diagnostic of diagnostics) {
      if (!diagnostic.source || typeof diagnostic.source.start_line !== "number") {
        // Diagnostics with a null source carry no line or column and are
        // skipped here; the Diagnostics tab still lists them.
        continue;
      }
      markers.push({
        severity: monaco.MarkerSeverity.Error,
        message: diagnostic.message,
        startLineNumber: diagnostic.source.start_line + 1,
        startColumn: diagnostic.source.start_column + 1,
        endLineNumber: diagnostic.source.end_line + 1,
        endColumn: diagnostic.source.end_column + 1,
        relatedInformation: diagnostic.explanations
          .filter((explanation) => explanation.source)
          .map((explanation) => ({
            message: `${explanation.kind}: ${explanation.message}`,
            resource: model.uri,
            startLineNumber: explanation.source.start_line + 1,
            startColumn: explanation.source.start_column + 1,
            endLineNumber: explanation.source.end_line + 1,
            endColumn: explanation.source.end_column + 1,
          })),
      });
    }
    monaco.editor.setModelMarkers(model, "cinnabar", markers);
  }, [diagnostics]);

  useImperativeHandle(ref, () => ({
    revealSpan(span) {
      const instance = editor.current;
      if (!instance || !span || typeof span.start_line !== "number") {
        return;
      }
      const selection = {
        startLineNumber: span.start_line + 1,
        startColumn: span.start_column + 1,
        endLineNumber: span.end_line + 1,
        endColumn: span.end_column + 1,
      };
      instance.setSelection(selection);
      instance.revealRangeInCenter(selection);
      instance.focus();
    },
  }));

  return <div className="editor" ref={host} />;
});

export default Editor;
