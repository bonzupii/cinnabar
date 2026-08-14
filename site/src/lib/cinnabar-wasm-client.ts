import type { PlaygroundReport } from "@/lib/cinnabar-diagnostics";

/*
 * Loads and calls the compiled `crates/cinnabar-wasm` module.
 *
 * `check()` runs the front end through borrow-checking and nothing past
 * it — no LLVM, no linking, no execution of whatever source it's given, by
 * construction of the crate it's built from (see
 * `src/generated/cinnabar-wasm/README.md`). That's what makes calling it
 * directly from the browser, with no server in between, a reasonable thing
 * to expose publicly with no rate limiting or sandboxing of its own: there's
 * nothing here for a sandbox to contain.
 */

type CheckerModule = {
  check: (source: string) => string;
  hover: (source: string, offset: number) => string;
};

export type HoverResult = {
  text: string;
  source: { file_id: number; path: string | null; start: number; end: number } | null;
};

let checkerPromise: Promise<CheckerModule> | null = null;

async function loadChecker(): Promise<CheckerModule> {
  if (!checkerPromise) {
    checkerPromise = (async () => {
      const checkerModule = await import("@/generated/cinnabar-wasm/cinnabar_wasm");
      await checkerModule.default("/wasm/cinnabar_wasm_bg.wasm");
      return checkerModule;
    })().catch((error: unknown) => {
      // A transient failure (flaky network, ad-blocker, captive portal)
      // must not poison every future call: without this, the rejected
      // promise stays cached forever and "try again" can never succeed
      // until a full page reload.
      checkerPromise = null;
      throw error;
    });
  }
  return checkerPromise;
}

/** Pre-warms the wasm module without running a check, for use on mount. */
export function preloadChecker(): void {
  void loadChecker();
}

export async function checkSource(source: string): Promise<PlaygroundReport> {
  const checker = await loadChecker();
  return JSON.parse(checker.check(source)) as PlaygroundReport;
}

/**
 * Hover text (a resolved signature, canonical type, and linearity, however
 * many of those are attached at `offset`) built from
 * `cinnabar::analysis::hover` -- the exact function the language server
 * calls, so a playground hover can never show something the LSP wouldn't.
 * `null` when nothing is attached at that position, which is a real answer
 * (whitespace, punctuation) rather than a failure to look one up.
 */
export async function hoverAt(source: string, offset: number): Promise<HoverResult | null> {
  const checker = await loadChecker();
  return JSON.parse(checker.hover(source, offset)) as HoverResult | null;
}
