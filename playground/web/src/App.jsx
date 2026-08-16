// The playground: an editor on the left, what the compiler said on the right.
//
// The tabs are the compiler's own surfaces, one per `--emit-json` document,
// and each one shows what that document actually contains rather than a
// summary of it. That is the whole point of the service: a reader who wants
// to know what the compiler thinks of their program should be able to see
// it, not be told about it.
//
// Nothing here re-derives anything. A diagnostic's position, a type's size,
// a variant's tag — all of it is read out of the response. The status bar
// is held to the same rule: every fact in it is a field of the response,
// not a count this page worked out for itself.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
// react-resizable-panels v4: `Group` / `Panel` / `Separator`, and
// `orientation` rather than v3's `direction`.
import { Group, Panel, Separator } from "react-resizable-panels";
import Editor from "./Editor.jsx";
import { DiagnosticsView, AstView, LayoutView, IrView, ProgramView } from "./views.jsx";
import {
  BuildIcon,
  CheckIcon,
  CinnabarMark,
  CodegenIcon,
  DiagnosticIcon,
  DocIcon,
  LinearIcon,
  LspIcon,
  RunIcon,
} from "./brandMarks.jsx";

// One icon per surface, taken from plate 07 rather than drawn for this page,
// and chosen by what the surface is rather than by what looks tidy: the
// attributed arena gets the icon the language server uses, because attached
// facts are what the language server reads; the IR gets the codegen icon.
const TABS = [
  { id: "diagnostics", label: "Diagnostics", Icon: DiagnosticIcon },
  { id: "program", label: "Program", Icon: RunIcon },
  { id: "ast", label: "AST", Icon: BuildIcon },
  { id: "typed", label: "Typed AST", Icon: LspIcon },
  { id: "layout", label: "Layout", Icon: LinearIcon },
  { id: "ir", label: "LLVM IR", Icon: CodegenIcon },
];

const FALLBACK_SOURCE = `pub fun main() I64
  return 0
end
`;

// The name the service compiles a submission under, and so the name in
// every span it reports. The file tab shows that name rather than a made-up
// one, so a reader matching a diagnostic's path to the editor sees the same
// string in both places.
const ENTRY_NAME = "playground.cnb";

const MOD_KEY =
  typeof navigator !== "undefined" && /mac|iphone|ipad/i.test(navigator.platform || navigator.userAgent)
    ? "⌘"
    : "Ctrl";

export default function App() {
  const [examples, setExamples] = useState([]);
  const [exampleId, setExampleId] = useState(null);
  const [source, setSource] = useState(FALLBACK_SOURCE);
  const [result, setResult] = useState(null);
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState(null);
  const [tab, setTab] = useState("diagnostics");
  const editorRef = useRef(null);

  useEffect(() => {
    let cancelled = false;
    fetch("/api/examples")
      .then((response) => response.json())
      .then((body) => {
        if (cancelled || !Array.isArray(body.examples) || body.examples.length === 0) {
          return;
        }
        setExamples(body.examples);
        setExampleId(body.examples[0].id);
        setSource(body.examples[0].source);
      })
      .catch(() => {
        // A missing example corpus is not worth an error banner: the editor
        // still works, it just opens on the fallback program.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const submit = useCallback(
    async (execute) => {
      setPending(true);
      setFailure(null);
      try {
        const response = await fetch("/api/compile", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ source, execute }),
        });
        const body = await response.json();
        if (!response.ok) {
          setFailure(body.error || `the service answered ${response.status}`);
          setResult(null);
          return;
        }
        setResult(body);
        // Land the reader on the tab that has the answer they asked for: a
        // rejected program's answer is its diagnostics, and a run's answer
        // is what the program did.
        if (!body.accepted) {
          setTab("diagnostics");
        } else if (execute) {
          setTab("program");
        }
      } catch (error) {
        setFailure(error.message);
        setResult(null);
      } finally {
        setPending(false);
      }
    },
    [source],
  );

  // Ctrl/Cmd+Enter runs, which is the shortcut every editor-shaped thing
  // has and the one a visitor will try first.
  useEffect(() => {
    function onKeyDown(event) {
      if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
        event.preventDefault();
        submit(true);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [submit]);

  const diagnostics = useMemo(() => {
    const document = result?.diagnostics?.ok ? result.diagnostics.document : null;
    return Array.isArray(document?.diagnostics) ? document.diagnostics : [];
  }, [result]);

  const verdict = useMemo(() => {
    if (pending) {
      return { tone: "pending", text: "Compiling…" };
    }
    if (failure) {
      return { tone: "error", text: failure };
    }
    if (!result) {
      return { tone: "idle", text: "Not compiled yet" };
    }
    if (result.accepted) {
      return { tone: "ok", text: "Accepted — the front end found nothing to object to" };
    }
    const count = diagnostics.length;
    return { tone: "error", text: `Rejected — ${count} diagnostic${count === 1 ? "" : "s"}` };
  }, [pending, failure, result, diagnostics]);

  // Read straight off the documents. `target` is the triple the layout
  // report was measured for, which is the machine the program would run on
  // — not this browser, and worth saying so.
  const facts = useMemo(() => {
    const entries = [];
    const nodes = result?.typedAst?.ok
      ? result.typedAst.document.nodes?.length
      : result?.ast?.ok
        ? result.ast.document.nodes?.length
        : null;
    if (typeof nodes === "number") {
      entries.push({ key: "nodes", text: `${nodes} nodes`, wide: false });
    }
    const target = result?.layout?.ok ? result.layout.document.target : null;
    if (target) {
      entries.push({ key: "target", text: target, wide: true });
    }
    return entries;
  }, [result]);

  function chooseExample(id) {
    const example = examples.find((entry) => entry.id === id);
    if (!example) {
      return;
    }
    setExampleId(id);
    setSource(example.source);
    setResult(null);
    setFailure(null);
  }

  function revealSpan(span) {
    if (!span || !editorRef.current) {
      return;
    }
    editorRef.current.revealSpan(span);
  }

  const selected = examples.find((entry) => entry.id === exampleId);

  return (
    <div className="app">
      <header className="titlebar">
        <div className="brand">
          <CinnabarMark size={22} title="Cinnabar" />
          <h1 className="brand__name">Cinnabar Playground</h1>
          <span className="brand__what">
            compiled and run on a server &mdash; every panel is the compiler&rsquo;s own output
          </span>
        </div>

        <div className="titlebar__spacer" />

        <div className="titlebar__actions">
          <label className="picker">
            <span className="label">Example</span>
            <select value={exampleId ?? ""} onChange={(event) => chooseExample(event.target.value)}>
              {examples.map((example) => (
                <option key={example.id} value={example.id}>
                  {example.title}
                </option>
              ))}
            </select>
          </label>
          <button type="button" onClick={() => submit(false)} disabled={pending}>
            <CheckIcon size={16} />
            Check
          </button>
          {/* `onAccent` rebinds the icon's vermilion detail to the button's
              own text colour: on the accent fill the vermilion would be
              invisible, and the arrow is the half of the figure that says
              "run". */}
          <button type="button" className="primary" onClick={() => submit(true)} disabled={pending}>
            <RunIcon size={16} onAccent />
            Run
            <span className="kbd">{MOD_KEY}↵</span>
          </button>
        </div>
      </header>

      <Group orientation="horizontal" className="panes" id="playground-panes">
        <Panel defaultSize="50%" minSize="22%" className="pane">
          <div className="panehead">
            <span className="tab tab--active tab__file">
              <DocIcon size={16} />
              {ENTRY_NAME}
            </span>
          </div>
          {selected ? <p className="blurb">{selected.blurb}</p> : null}
          <Editor ref={editorRef} value={source} onChange={setSource} diagnostics={diagnostics} />
        </Panel>

        <Separator className="handle" />

        <Panel defaultSize="50%" minSize="22%" className="pane">
          <nav className="panehead">
            {TABS.map((entry) => (
              <button
                key={entry.id}
                type="button"
                className={entry.id === tab ? "tab tab--active" : "tab"}
                onClick={() => setTab(entry.id)}
              >
                <entry.Icon size={16} />
                {entry.label}
                {entry.id === "diagnostics" && diagnostics.length > 0 ? (
                  <span className="tab__count">{diagnostics.length}</span>
                ) : null}
              </button>
            ))}
          </nav>
          <div className="tabbody">
            {tab === "diagnostics" ? <DiagnosticsView result={result} onReveal={revealSpan} /> : null}
            {tab === "program" ? <ProgramView result={result} /> : null}
            {tab === "ast" ? <AstView section={result?.ast} title="the arena as parsing left it" /> : null}
            {tab === "typed" ? (
              <AstView section={result?.typedAst} title="the same arena with every front-end attachment" />
            ) : null}
            {tab === "layout" ? <LayoutView section={result?.layout} /> : null}
            {tab === "ir" ? <IrView section={result?.llvmIr} /> : null}
          </div>
        </Panel>
      </Group>

      <footer className={`statusbar statusbar--${verdict.tone}`}>
        <div className="statusbar__verdict">
          <span className={`dot dot--${verdict.tone}`} aria-hidden="true" />
          <span className="statusbar__text" role="status">
            {verdict.text}
          </span>
        </div>
        <div className="statusbar__spacer" />
        {facts.map((fact) => (
          <span key={fact.key} className={fact.wide ? "statusbar__fact statusbar__fact--wide" : "statusbar__fact"}>
            {fact.text}
          </span>
        ))}
        <span className="statusbar__fact statusbar__fact--wide">{MOD_KEY}↵ to run</span>
      </footer>
    </div>
  );
}
