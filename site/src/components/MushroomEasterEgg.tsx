"use client";

import { motion, useReducedMotion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { CheckIcon } from "@/components/brand/icons";
import CodeBlock from "@/components/CodeBlock";
import PlaygroundDiagnostics from "@/components/PlaygroundDiagnostics";
import {
  assembleMushroomProgram,
  isPuzzleComplete,
  MUSHROOM_ENTRY_PATH,
  MUSHROOM_IDS,
  mushroomHandles,
  mushroomState,
  type MushroomId,
  type MushroomState,
  type MushroomMove,
} from "@/content/mushroom-easter-egg";
import {
  isClean,
  type DiagnosticSource,
  type PlaygroundReport,
} from "@/lib/cinnabar-diagnostics";
import { checkSource } from "@/lib/cinnabar-wasm-client";
import { endsWithKonami, KONAMI_SEQUENCE } from "@/lib/konami";
import { OPEN_MUSHROOM_EGG_EVENT } from "@/lib/logo-easter-egg";

/*
 * The Konami-code easter egg: the mushroom the language is named for, and a
 * small foraging puzzle refereed live by the borrow checker.
 *
 * Three mushroom tiles; clicking one forages it, clicking again eats it.
 * Every move appends real Cinnabar to a growing `main()` (the template
 * pieces and the pure move-log helpers live in content/mushroom-easter-egg),
 * and the assembled program is re-run through the real check() after each
 * click. The puzzle is that forage-all-then-eat-all leaks a held mushroom on
 * the next forage's error path and the checker says so; the solve is to
 * fully dispatch each mushroom before starting the next. Solved = every
 * mushroom foraged and eaten exactly once (move-log logic) AND the live
 * report is clean (the checker's half).
 *
 * The key listener and its rolling buffer live here rather than in lib/ —
 * the same split Reveal keeps for its useInView wiring; lib/konami.ts holds
 * only the sequence and the pure matcher. Keys landing in an editable region
 * are ignored entirely: the playground's CodeMirror editor is a
 * contenteditable div where arrow keys are real text navigation, and the egg
 * firing mid-edit would be a bug, not a delight.
 *
 * The checker is not touched until the dialog first opens. This component is
 * mounted globally, and most visitors will never find the egg — they should
 * not pay for the wasm download. After that it's the playground's own
 * pattern: one check per move, a request counter so a stale result can never
 * overwrite a newer one. No debounce — moves are discrete clicks, not
 * keystrokes.
 *
 * Modal mechanics follow MobileMenu: portalled to <body>, focus moved in on
 * open and returned on close, Tab wrapped inside the panel, Escape and the
 * backdrop close it, body scroll locked while open. z-100 puts it above the
 * header's z-50 and level with the skip link. Undo/Reset stay real enabled
 * buttons even when the log is empty (aria-disabled + no-op instead of
 * `disabled`) so the focus trap's first/last arithmetic never lands on an
 * unfocusable element.
 */

/*
 * The wasm crate checks every submission under its own synthetic name,
 * `playground.cnb`; the egg's windows are titled `mushroom.cnb`. Only the
 * label is renamed so the excerpt header matches the window it sits in —
 * messages, spans and offsets are the checker's own, untouched.
 */
function relabel(report: PlaygroundReport): PlaygroundReport {
  const rename = (source: DiagnosticSource | null) =>
    source ? { ...source, path: MUSHROOM_ENTRY_PATH } : source;
  return {
    ...report,
    diagnostics: report.diagnostics.map((diagnostic) => ({
      ...diagnostic,
      source: rename(diagnostic.source),
      explanations: diagnostic.explanations.map((explanation) => ({
        ...explanation,
        source: rename(explanation.source),
      })),
    })),
  };
}

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement
  ) {
    return true;
  }
  // Covers CodeMirror: contenteditable is inherited, so any element inside
  // the editor reports true here.
  return target.isContentEditable;
}

/*
 * The cinnabar chanterelle, drawn in the brand's own language: chamfered
 * polygons, straight segments near 34°/45°/90°, one accent, greys otherwise.
 * The species' wavy cap margin becomes a faceted zigzag with the depressed
 * centre a real chanterelle has; the mycelium threads borrow AsciiDiagram's
 * hairline-stem-with-vermilion-tip connector. The single spore dot is the
 * same licence the icon set grants the LSP glyph — one small circle, nothing
 * else curved.
 */
const CAP_POINTS = "52,52 68,36 84,52 102,34 120,46 138,34 156,52 172,36 188,52 148,92 92,92";
const STEM_POINTS = "108,92 132,92 132,142 126,150 114,150 108,142";

const THREADS = [
  { points: "114,150 88,176 70,176", tip: { x1: 70, y1: 176, x2: 60, y2: 176 } },
  { points: "117,150 104,163", tip: { x1: 104, y1: 163, x2: 98, y2: 169 } },
  { points: "120,150 120,170 132,182", tip: { x1: 132, y1: 182, x2: 139, y2: 189 } },
  { points: "123,150 137,164", tip: { x1: 137, y1: 164, x2: 143, y2: 170 } },
  { points: "126,150 152,176 170,176", tip: { x1: 170, y1: 176, x2: 180, y2: 176 } },
] as const;

function MushroomFigure() {
  const reducedMotion = useReducedMotion();

  return (
    <svg
      viewBox="0 0 240 200"
      width={250}
      className="max-w-full"
      aria-hidden="true"
      focusable="false"
    >
      <motion.line
        x1={28}
        y1={150.5}
        x2={212}
        y2={150.5}
        stroke="var(--hairline-strong)"
        strokeWidth={1}
        initial={reducedMotion ? false : { pathLength: 0 }}
        animate={{ pathLength: 1 }}
        transition={{ duration: 0.2, ease: "easeOut" }}
      />
      {THREADS.map((thread, index) => (
        <g key={index}>
          <motion.polyline
            points={thread.points}
            fill="none"
            stroke="var(--grey)"
            strokeWidth={1}
            initial={reducedMotion ? false : { pathLength: 0, opacity: 0 }}
            animate={{ pathLength: 1, opacity: 1 }}
            transition={{ delay: 0.3 + index * 0.06, duration: 0.22, ease: "easeOut" }}
          />
          <motion.line
            {...thread.tip}
            stroke="var(--cinnabar)"
            strokeWidth={1.4}
            initial={reducedMotion ? false : { opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ delay: 0.52 + index * 0.06, duration: 0.12 }}
          />
        </g>
      ))}
      <motion.g
        style={{ transformBox: "view-box", transformOrigin: "120px 150px" }}
        initial={reducedMotion ? false : { scale: 0.2, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ delay: 0.08, duration: 0.3, ease: "easeOut" }}
      >
        <polygon points={STEM_POINTS} fill="var(--grey)" />
        <polygon points={CAP_POINTS} fill="var(--cinnabar)" />
        {/*
          Decurrent ridges, drawn in the panel colour so they read as cuts in
          the cap rather than a second hue: one vertical, two parallel to the
          cap's own 45° sides.
        */}
        <line x1={120} y1={54} x2={120} y2={88} stroke="var(--panel)" strokeWidth={1.25} />
        <line x1={68} y1={52} x2={104} y2={88} stroke="var(--panel)" strokeWidth={1.25} />
        <line x1={172} y1={52} x2={136} y2={88} stroke="var(--panel)" strokeWidth={1.25} />
      </motion.g>
      {/* The one circle, per the icon set's LSP exception: a spore. */}
      <motion.circle
        cx={196}
        cy={30}
        r={1.8}
        fill="var(--cinnabar)"
        initial={reducedMotion ? false : { opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 0.6, duration: 0.15 }}
      />
    </svg>
  );
}

/*
 * A tile-sized cut of the same artwork: cap and stem only, cropped to their
 * bounds, no threads or spore. State is carried by treatment, not new
 * shapes — growing keeps the solid accent cap; held drops the fill for an
 * accent outline (picked up, not yet consumed); eaten fades the whole thing
 * to grey. One accent colour, same geometry throughout.
 */
function MushroomTileFigure({ state }: { state: MushroomState }) {
  return (
    <svg viewBox="46 28 148 128" width={72} aria-hidden="true" focusable="false">
      <g opacity={state === "eaten" ? 0.35 : 1}>
        <polygon points={STEM_POINTS} fill="var(--grey)" />
        {state === "held" ? (
          <polygon
            points={CAP_POINTS}
            fill="none"
            stroke="var(--cinnabar)"
            strokeWidth={2.5}
            strokeLinejoin="miter"
          />
        ) : (
          <>
            <polygon
              points={CAP_POINTS}
              fill={state === "eaten" ? "var(--grey)" : "var(--cinnabar)"}
            />
            <line x1={120} y1={54} x2={120} y2={88} stroke="var(--panel)" strokeWidth={2} />
            <line x1={68} y1={52} x2={104} y2={88} stroke="var(--panel)" strokeWidth={2} />
            <line x1={172} y1={52} x2={136} y2={88} stroke="var(--panel)" strokeWidth={2} />
          </>
        )}
      </g>
    </svg>
  );
}

function tileLabel(id: MushroomId, state: MushroomState): string {
  if (state === "growing") return `Forage mushroom ${id}`;
  if (state === "held") return `Eat mushroom ${id}`;
  return `Eat mushroom ${id} again — it's already been eaten`;
}

export default function MushroomEasterEgg() {
  const [open, setOpen] = useState(false);
  const [moves, setMoves] = useState<readonly MushroomMove[]>([]);
  const [report, setReport] = useState<PlaygroundReport | null>(null);
  const [checkFailed, setCheckFailed] = useState(false);
  const latestRequest = useRef(0);
  const bufferRef = useRef<string[]>([]);
  const panelRef = useRef<HTMLDivElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const reducedMotion = useReducedMotion();

  const close = () => setOpen(false);

  useEffect(() => {
    // Both triggers open the same way: a fresh puzzle, clear any stale load
    // failure, open.
    const openEgg = () => {
      setMoves([]);
      setReport(null);
      setCheckFailed(false);
      setOpen(true);
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (isEditableTarget(event.target)) return;
      const buffer = bufferRef.current;
      buffer.push(event.code);
      if (buffer.length > KONAMI_SEQUENCE.length) {
        buffer.splice(0, buffer.length - KONAMI_SEQUENCE.length);
      }
      if (endsWithKonami(buffer)) {
        buffer.length = 0;
        openEgg();
      }
    };
    // The second way in: SiteHeaderLogo dispatches this when the site logo is
    // spam-clicked fast enough (lib/logo-easter-egg.ts holds the rule).
    document.addEventListener("keydown", onKeyDown);
    window.addEventListener(OPEN_MUSHROOM_EGG_EVENT, openEgg);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      window.removeEventListener(OPEN_MUSHROOM_EGG_EVENT, openEgg);
    };
  }, []);

  const source = assembleMushroomProgram(moves);

  // One real check per move (the empty program included — the panel opens
  // with a true answer, not a placeholder verdict). The counter is the
  // playground's own stale-response guard: only the newest request may set
  // state. Opening triggers the first check, which is also what triggers the
  // wasm load.
  useEffect(() => {
    if (!open) return;
    const requestId = (latestRequest.current += 1);
    checkSource(source)
      .then((result) => {
        if (latestRequest.current === requestId) {
          setReport(relabel(result));
          setCheckFailed(false);
        }
      })
      .catch(() => {
        if (latestRequest.current === requestId) setCheckFailed(true);
      });
  }, [open, source]);

  useEffect(() => {
    if (!open) return;

    previousFocusRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;

    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";

    const panel = panelRef.current;
    panel?.querySelector<HTMLElement>("button")?.focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        setOpen(false);
        return;
      }
      if (event.key !== "Tab" || !panel) return;

      // Includes the window bodies: WindowBody's <pre> is focusable so its
      // horizontal scroll stays keyboard-reachable, and a trap that only knew
      // about buttons would wrap right past them.
      const focusable = panel.querySelectorAll<HTMLElement>('a[href], button, [tabindex="0"]');
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];

      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.body.style.overflow = previousOverflow;
      previousFocusRef.current?.focus();
    };
  }, [open]);

  if (!open) return null;

  // Functional updates throughout: the kind of the next move depends on the
  // log the move lands on, not the log this render saw.
  const clickMushroom = (id: MushroomId) => {
    setMoves((previous) => {
      const kind = mushroomState(previous, id) === "growing" ? "forage" : "eat";
      return [...previous, { kind, mushroom: id }];
    });
  };
  const undo = () => setMoves((previous) => previous.slice(0, -1));
  const reset = () => setMoves([]);

  const complete = isPuzzleComplete(moves);
  // `report` lags one move behind while a check is in flight; that can't
  // fake a win, because any log one move short of complete either holds an
  // unconsumed mushroom or awaits a third forage — both real errors — so the
  // stale report is never clean when `complete` first turns true.
  const solved = complete && report !== null && isClean(report);
  const logEmpty = moves.length === 0;

  const controlClass =
    "border-hairline-strong text-text hover:border-text hover:bg-panel-raised panel-hover pressable inline-flex items-center border px-3 py-1.5 text-[11px] font-bold tracking-widest uppercase";

  return createPortal(
    <div className="fixed inset-0 z-100 flex items-center justify-center p-4 sm:p-8">
      <div aria-hidden="true" onClick={close} className="bg-ground/80 absolute inset-0" />
      <motion.div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label="Cantharellus cinnabarinus, the mushroom behind Cinnabar"
        className="border-hairline bg-panel relative max-h-full w-full max-w-5xl overflow-y-auto border"
        initial={reducedMotion ? false : { opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.16, ease: "easeOut" }}
      >
        <div className="flex flex-col gap-6 p-5 sm:p-8">
          <div className="flex items-start justify-between gap-4">
            <div className="grid items-center gap-2 sm:grid-cols-[auto_1fr] sm:gap-8">
              <MushroomFigure />
              <div className="max-w-prose">
                <p className="text-label font-mono text-[11px] tracking-[0.16em] uppercase">
                  ↑ ↑ ↓ ↓ ← → ← → B A
                </p>
                <h2 className="text-text mt-2 text-xl font-bold italic sm:text-2xl">
                  Cantharellus cinnabarinus
                </h2>
                <p className="text-secondary mt-3 text-sm leading-relaxed">
                  The cinnabar chanterelle — the actual mushroom Cinnabar&rsquo;s name and
                  colour came from, found in leaf litter. A mushroom can be eaten exactly
                  once; the type system holds the same view. Three grow below. Forage and
                  eat them all, and leave <span className="font-mono">main()</span> with
                  nothing for the checker to say.
                </p>
              </div>
            </div>
            <button
              type="button"
              onClick={close}
              aria-label="Close"
              className="border-hairline-strong text-text hover:border-text hover:bg-panel-raised panel-hover pressable inline-flex h-9 w-9 flex-none items-center justify-center border"
            >
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                aria-hidden="true"
              >
                <line x1="5" y1="5" x2="19" y2="19" />
                <line x1="19" y1="5" x2="5" y2="19" />
              </svg>
            </button>
          </div>

          <div className="border-hairline flex flex-col gap-4 border-t pt-5">
            <div className="flex flex-wrap items-center justify-between gap-4">
              <div className="flex flex-wrap gap-3" role="group" aria-label="Three mushrooms">
                {MUSHROOM_IDS.map((id) => {
                  const state = mushroomState(moves, id);
                  return (
                    <button
                      key={id}
                      type="button"
                      data-testid={`mushroom-tile-${id}`}
                      data-state={state}
                      aria-label={tileLabel(id, state)}
                      onClick={() => clickMushroom(id)}
                      className="border-hairline hover:border-hairline-strong hover:bg-panel-raised panel-hover pressable flex flex-col items-center gap-1 border px-5 pt-2 pb-3"
                    >
                      <MushroomTileFigure state={state} />
                      <span
                        className={`font-mono text-[10px] tracking-[0.14em] uppercase ${
                          state === "held" ? "text-cinnabar-text" : "text-label"
                        }`}
                      >
                        {id} · {state}
                      </span>
                    </button>
                  );
                })}
              </div>
              <div className="flex flex-col items-start gap-2 sm:items-end">
                <span
                  data-testid="mushroom-move-count"
                  className="text-label font-mono text-[11px] tracking-[0.16em] uppercase"
                >
                  Moves {moves.length}
                </span>
                <div className="flex gap-2">
                  <button
                    type="button"
                    data-testid="mushroom-undo"
                    onClick={undo}
                    aria-disabled={logEmpty}
                    className={`${controlClass}${logEmpty ? " opacity-40" : ""}`}
                  >
                    Undo last move
                  </button>
                  <button
                    type="button"
                    data-testid="mushroom-reset"
                    onClick={reset}
                    aria-disabled={logEmpty}
                    className={`${controlClass}${logEmpty ? " opacity-40" : ""}`}
                  >
                    Reset
                  </button>
                </div>
              </div>
            </div>
            {solved ? (
              <motion.p
                data-testid="mushroom-solved"
                className="text-text flex items-center gap-2 text-sm"
                initial={reducedMotion ? false : { opacity: 0 }}
                animate={{ opacity: 1 }}
                transition={{ duration: 0.15 }}
              >
                <CheckIcon size={13} className="text-cinnabar" />
                All three foraged and eaten, zero diagnostics. The checker signs off.
              </motion.p>
            ) : (
              <p className="text-secondary text-sm">
                {complete && report !== null && !checkFailed
                  ? "Every mushroom foraged and eaten — but the checker objects to the order. Undo and try another."
                  : logEmpty
                    ? "Before a single move the checker already objects: SPECIES is unused. Click a mushroom to forage it."
                    : "Click a mushroom to forage it, again to eat it. Every move is checked; win with zero diagnostics."}
              </p>
            )}
          </div>

          <div className="grid grid-cols-1 gap-6 lg:grid-cols-2 lg:items-start">
            <CodeBlock
              code={source}
              linearHandles={mushroomHandles(moves)}
              path={MUSHROOM_ENTRY_PATH}
            />
            {/*
              PlaygroundDiagnostics is built to dock under a Window titlebar
              and only draws its own bottom rule; standing alone beside the
              CodeBlock it gets the rest of its frame from border-x/border-t.
            */}
            {report ? (
              <PlaygroundDiagnostics
                report={report}
                source={source}
                className="border-x border-t"
              />
            ) : (
              <div className="border-hairline bg-code-terminal border">
                <div className="text-label border-hairline flex items-center gap-2 border-b px-4 py-2 font-mono text-[10px] tracking-[0.14em] uppercase sm:px-6">
                  Diagnostics
                </div>
                <pre className="w-full overflow-x-auto px-4 py-4 font-mono text-[12.5px] leading-[1.75] sm:px-6 sm:text-[13.5px]">
                  <code className="text-term-output">
                    {checkFailed
                      ? "The checker failed to load. Close this and try again."
                      : "Loading the checker…"}
                  </code>
                </pre>
              </div>
            )}
          </div>

          <p className="text-label font-mono text-[11px] tracking-[0.04em]">
            Checked live in your browser, by the same wasm build of the compiler front end
            the playground runs.
          </p>
        </div>
      </motion.div>
    </div>,
    document.body,
  );
}
