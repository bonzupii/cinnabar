# Cinnabar Website Improvement Plan

**Status:** Tranches 1 and 2 were approved together on 2026-08-15 for the
isolated preview branch. G-02 is resolved in `DOCS-SOURCE-MAP.md`.

**Planning location:** This plan and every later site-specific planning artifact
belong under `site/.planning/`. Do not create a repository-root `.planning/`
directory for this work.

## Objective

Make the existing public site explain Cinnabar's value before its setup details:
a first-time systems programmer should see the language's differentiator, watch
the real browser-local compiler frontend reject a real repository fixture, and
reach the full Playground in one interaction.

The implementation must preserve the site's established identity and static
architecture. It is a hierarchy and content correction, not a visual rebrand or
a second compiler demonstration.

## User-visible outcome

After Tranche 1 ships:

- The home page leads with “Systems programming without safety escape hatches.”
  and a primary **Try in Playground** action.
- The first viewport contains a compact editable editor/diagnostics composition loaded
  from `tests/fixtures/explain_leak.cnb`; the existing WASM checker reports a
  real rejection.
- Playground is the first item in the desktop header, mobile menu, and shared
  documentation footer order.
- Three editorially distinct promises replace the eight equal feature cards.
- The first static code sample demonstrates linear resource ownership; tail
  recursion remains available as a later sample.
- The full pipeline, full manifest explanation, and illustrative fixed
  diagnostic no longer compete with the live proof on the home page. The
  Architecture and Reference routes remain the depth destinations.
- Development status is honest and durable: experimental/early development
  plus a Roadmap handoff, without milestone arithmetic or capability bars.
- Search and social metadata use the approved value proposition and do not
  overstate what runs in the browser.

## Non-goals

Tranche 1 does not:

- add, remove, or rename a public route;
- add Learn, Language, Docs, or Contributing navigation groups;
- split Install, Reference, or Architecture content;
- add a package, analytics, a server/API, a Server Action, or a second WASM
  bundle;
- add another checker hook, diagnostic renderer, or source-to-diagnostic path;
- change the generated WASM glue or `public/wasm/cinnabar_wasm_bg.wasm`;
- change compiler behavior, `MANIFESTO.md`, `tests/fixtures/spec.cnb`, or
  `tests/fixtures/explain_leak.cnb`;
- replace the current color tokens, typography, hard edges, rule grids, theme
  behavior, or motion model;
- publish a full anti-feature catalogue on the home page;
- publish invented diagnostics or non-Cinnabar syntax.

## Locked design and content decisions

These decisions are requirements. Task actions cite their IDs for traceability.

| ID | Locked decision |
|---|---|
| D-01 | All site planning artifacts live under `site/.planning/`; no root planning tree is created. |
| D-02 | Tranche 1 is the homepage plus navigation among existing routes. Tranche 2 is a later, separate docs-IA effort. |
| D-03 | Preserve the dark/vermilion identity, Schibsted Grotesk + IBM Plex Mono pairing, warm light theme, hard edges, hairline/rule-grid system, restrained motion, and current brand assets. Do not introduce a parallel visual system. |
| D-04 | The home `h1` remains the accessible Cinnabar wordmark. The value proposition is the next heading, and the wordmark's display size is reduced enough that the proposition, CTA, and proof fit in the first viewport. |
| D-05 | The hero action order is Playground primary, Manifesto secondary, Install and Source tertiary. Installation commands leave the hero. |
| D-06 | The hero proof is loaded from `tests/fixtures/explain_leak.cnb` at static build time and checked through the existing `PlaygroundEditor` → `checkSource` → `PlaygroundDiagnostics` path. There is no copied TypeScript fixture string, second checker path, server, code generation, linking, or execution in the browser. |
| D-07 | Home tells three primary stories: ownership without ceremony; rules without escape hatches; predictable runtime behavior. A compact factual strip may support them; an equal-weight eight-card wall may not return. |
| D-08 | The first static sample is resource ownership. Tail recursion remains available but is not the default sample. Shared samples are selected by stable IDs, not array positions. |
| D-09 | `NAV` remains the single source for desktop, mobile, footer, and current-route behavior in Tranche 1. Only its existing links and labels are reordered: Playground, Manifesto, Install, Reference, Architecture, Roadmap. |
| D-10 | Editable prose remains in `src/app/(home)/content.md`; TypeScript owns only structure and component bindings. Normative language claims come from `MANIFESTO.md`, and source examples come from repository fixtures. |
| D-11 | Preserve `output: "export"`, trailing-slash routes, one `h1`, keyboard semantics, no-JavaScript visibility, reduced-motion behavior, both themes, and 390px overflow guarantees. |
| D-12 | Add no dependency. Do not modify `package.json`, `package-lock.json`, `next.config.ts`, generated WASM files, or `pre_commit_check.sh` for Tranche 1. |
| D-13 | Do not publish `E0442A`, postfix `?`, `if failed`, or claims that the browser generates or executes machine code. Safe browser copy says the real frontend runs through borrow checking and does not link, execute, or send source to a server. |
| D-14 | Keep “Probably better than Rust.” in the footer only. Keep the existing subtle mushroom/easter-egg treatment; add no new literal mushroom or mycelium artwork. |
| D-15 | Replace milestone arithmetic and progress bars with the existing early/experimental status vocabulary and a Roadmap link. |
| D-16 | Tranche 1 is one coherent code unit. Run focused checks while implementing, then the full site checks, then the repository's exact required gate once after the code unit is complete. |
| D-17 | Tranche 1 uses compact editable source in the embedded hero editor. The fixture loads into the existing CodeMirror instance, every source edit rechecks through the existing WASM path, and the full Playground CTA remains visible. Do not add a read-only prop or silently switch modes. Read-only is permitted only as a separately approved plan revision if, after implementing and optimizing the editable composition, the keyboard, 390px, or Lighthouse acceptance criteria still cannot be met. |

## Assumptions and open decisions

| ID | State | Assumption or decision |
|---|---|---|
| A-01 | Confirmed by current code | `Home` is a Server Component and can read an allowlisted repository fixture during `next build`; `PlaygroundEditor` is the existing client boundary. |
| A-02 | Confirmed by current code | `PlaygroundEditor` can be parameterized without changing the full `/playground/` defaults; checker request ordering, preload, errors, hover, gutter geometry, and no-wrap behavior remain inside that component. |
| A-03 | Confirmed by current code | Reordering `NAV` propagates to header, mobile menu, footer, active state, and sitemap without editing those consumers. |
| O-01 | Locked default | The compact hero source is editable per D-17. Its prop contract, CodeMirror configuration, focus order, viewport sizing, and E2E assertions must all implement that behavior. |
| O-02 | Locked default | Use the existing `STATUS_BADGE` wording and the phrase “experimental and under active development.” Any stronger capability claim needs an auditable source and a separate copy decision. |
| O-03 | Decision gate G-02, Tranche 2 only | Learn/Docs grouping, public chapter URLs, and authoritative sources for educational prose are unresolved. They must be approved before any Tranche 2 route is published. |

## Reversibility and decision gates

| Decision | Rating | Consequence |
|---|---|---|
| Embedded `PlaygroundEditor` prop shape | Reversible | Two internal call sites; change is local if the hero composition changes. |
| Editable versus read-only hero editor | Reversible | Compact editable source is the approved Tranche 1 default. A read-only variant is a local prop/configuration revision, but may be proposed only after the implemented editable path still fails keyboard, 390px, or Lighthouse acceptance following optimization. |
| Three-promise copy and home metadata | Reversible | Public copy changes, but no URL, storage, or external contract changes. |
| Reordering existing `NAV` entries | Reversible | One shared array and tests; all destinations remain stable. |
| Separating exported routes from primary navigation in Tranche 2 | Costly | Touches every navigation, sitemap, metadata, accessibility, and visual route loop. Record and test the new contract once. |
| Publishing new Learn/Contributing/Architecture URLs | One-way public contract | External links and search indexes may depend on the paths; static export provides no runtime redirect fallback. Gate G-02 before creating routes. |
| Keeping `/reference/` while changing its visible label to “CLI Reference” | Reversible and preferred | Avoids breaking the existing public URL. No decision gate is needed. |

### Tranche 1 approved hero-editor default

Implement compact editable source per D-17. The full fixture loads into the
existing CodeMirror editor, every edit rechecks through the existing WASM path,
and the full Playground CTA remains visible. This is the locked Tranche 1 path,
not an implementation-time choice.

If the implemented editable composition still cannot meet the keyboard, 390px,
or Lighthouse acceptance criteria after optimizing the existing composition and
loading path, stop and request a separate plan revision approving read-only
behavior. Do not add dormant read-only configuration, switch modes during
implementation, or create a different component/checker architecture.

### G-02 — Documentation IA and public URLs (approved 2026-08-15)

The approval to implement both tranches resolved the following in
`DOCS-SOURCE-MAP.md` before the preview is published:

- the final Learn/Docs top-level labels and their primary-navigation behavior;
- the public URL list, including `/contributing/development/` and any Learn or
  Architecture child routes;
- the authoritative source for every educational page;
- whether Architecture chapters are separate URLs or anchored sections under
  `/architecture/`;
- confirmation that `/reference/` remains the permalink while its label becomes
  “CLI Reference.”

## Existing interfaces the implementation must preserve

- `site/src/lib/site.ts`
  - `NAV: readonly NavItem[]` feeds `NavLinks`, `MobileMenu`, and `SiteFooter`.
  - `ROUTES = ["/", ...NAV.map(...)]` feeds the sitemap in Tranche 1.
  - `TAGLINE`, `DESCRIPTION`, `BADGES`, `STATUS_BADGE`, `QUIP`, `REPO_URL`, and
    `SITE_URL` are shared metadata/identity strings.
- `site/src/components/PlaygroundEditor.tsx`
  - owns source state, sample state, CodeMirror, WASM preload, stale-request
    suppression, error fallback, line-number geometry, and
    `PlaygroundDiagnostics` composition;
  - currently has no props and initializes from `PLAYGROUND_SAMPLES[0]`.
- `site/src/lib/cinnabar-wasm-client.ts`
  - `checkSource(source): Promise<PlaygroundReport>` and `preloadChecker()` are
    the only checker client path;
  - the module stops after borrow checking and cannot link or execute source.
- `site/src/lib/repo-docs.ts`
  - already establishes `REPO_ROOT` and build-time repository reads; extend it
    narrowly with an allowlisted fixture key rather than accepting arbitrary
    paths.
- `site/src/lib/page-content.ts`
  - `readPageContent("(home)")` reads route-adjacent blocks;
  - `items(name)` binds `###` sections by slug and fails loudly on drift.
- `site/src/content/samples.ts`
  - `SAMPLES` owns static sample order and fixture provenance;
  - `PLAYGROUND_SAMPLES` currently consumes two entries by array position, an
    ordering hazard WP-03 must remove before changing the home default.

## Required implementation preflight

Before any code edit, the implementation agent must:

1. Read `AGENTS.md`, `site/AGENTS.md`, `MANIFESTO.md`, and
   `tests/fixtures/spec.cnb` end to end. The manifesto is normative; fixtures
   are reference data. Do not edit `tests/fixtures/spec.cnb`.
2. Read `tests/fixtures/explain_leak.cnb` and confirm it remains the rejected
   one-path resource-leak fixture. Do not alter it to fit the page.
3. Read the task's listed source and test files before editing them.
4. If `site/node_modules/` is absent, run `npm ci` from `site/` using the
   existing lockfile. This installs existing dependencies; it must not change
   `package.json` or `package-lock.json`.
5. Read the installed Next 16.3.1 guides
   `site/node_modules/next/dist/docs/01-app/02-guides/static-exports.md` and
   `site/node_modules/next/dist/docs/01-app/01-getting-started/server-and-client-components.md`
   before editing the Server/Client boundary. If the installed package names
   the second file differently, use the built-in file glob under
   `site/node_modules/next/dist/docs/01-app/` to resolve the single
   `server-and-client-components.md` match and record the exact path read.
6. Never read, search, inspect, or edit `.semgrep.yml` or any Semgrep rule.
7. Use built-in read/search/edit/write operations for files. `rm` is prohibited.
   No git action is part of this plan unless the user separately authorizes it.

## Tranche 1 — Homepage and existing-route navigation

### Dependency and wave map

| Wave | Work package | Depends on | File overlap rule |
|---|---|---|---|
| 1 | WP-01 tracer: value-led hero + real rejection | None | Runs alone; proves the architecture before expansion. |
| 2 | WP-02 three-promise hierarchy | WP-01 | Parallel-safe with WP-05. |
| 2 | WP-05 shared navigation reorder | WP-01 | No files overlap WP-02. |
| 3 | WP-03 resource-first samples | WP-02 | Parallel-safe with WP-06. |
| 3 | WP-06 metadata and durable copy | WP-01, WP-02, WP-05 | No files overlap WP-03. |
| 4 | WP-04 retire duplicate diagnostic and finish status handoff | WP-03, WP-06 | Owns the final home page/content edits. |
| 5 | WP-07 release verification | WP-04, WP-05 | No code edits unless a check fails. |

WP-01 is the first implementation task. Do not start a Wave 2 package until its
focused browser contract is green.

### WP-01 — Tracer: prove the proposition with the real checker

**Type:** tracer, test-first  
**Wave:** 1  
**Depends on:** None  
**Decisions:** D-03, D-04, D-05, D-06, D-10, D-11, D-12, D-13, D-17

**Read first**

- `site/src/app/(home)/page.tsx`
- `site/src/app/(home)/content.md`
- `site/src/components/PlaygroundEditor.tsx`
- `site/src/components/PlaygroundDiagnostics.tsx`
- `site/src/lib/cinnabar-wasm-client.ts`
- `site/src/lib/repo-docs.ts`
- `site/src/app/playground/page.tsx`
- `site/tests/e2e/playground.spec.ts`
- `site/tests/e2e/prepare.ts`
- `tests/fixtures/explain_leak.cnb`

**Modify**

- `site/src/lib/repo-docs.ts`
- `site/src/components/PlaygroundEditor.tsx`
- `site/src/app/(home)/page.tsx`
- `site/src/app/(home)/content.md`
- `site/tests/e2e/home-playground.spec.ts` (new)

**Action**

1. Write `home-playground.spec.ts` first. It must load `/`, wait for the hero's
   existing `data-testid="playground-diagnostics"` region to reach a real error
   state, assert the editor contains the fixture-specific `leak_on_one_path`
   function, and assert the clean verdict is no longer shown. Then prove the
   D-17 editing contract: keyboard-focus the `.cm-content` textbox after the
   hero CTA links, replace its contents with the existing verified-clean
   `PLAYGROUND_SAMPLES` entry whose stable ID is `memory`, assert the visible
   editor changed, and wait for “No diagnostics.” This test must import/reuse
   that shared sample rather than copy another Cinnabar program. Finally follow
   the primary **Try in Playground** link to `/playground/`. Do not pin a
   made-up error identifier or the compiler's full sentence; the stable
   contracts are that the unmodified fixture is initially present and rejected,
   the embedded editor accepts keyboard input, and each edit reaches the real
   checker.
2. Extend `repo-docs.ts` with an allowlisted repository-fixture contract. Add a
   key such as `explainLeak` mapped internally to
   `tests/fixtures/explain_leak.cnb`, an exported `RepoFixtureName` type derived
   from that map, and `readRepoFixture(name: RepoFixtureName): Promise<string>`.
   Callers must not supply filesystem paths. Reuse the existing `REPO_ROOT` and
   `readFile`; do not duplicate fixture text or expose a path-traversal surface.
3. Export a documented discriminated `PlaygroundEditorProps` contract with the
   existing full mode as the zero-config default and exactly one embedded shape:
   `{ mode: "embedded"; initialSource: string }`. Do not add an `editable` or
   `readOnly` property. Embedded mode hides the six-sample intro/tab picker,
   initializes its controlled `source` state from `initialSource`, gives the
   CodeMirror textbox the accessible name “Editable Cinnabar source,” and keeps
   CodeMirror editable with the same `onChange={setSource}` path. Use a 12rem
   embedded editor viewport with internal vertical and horizontal scrolling;
   retain no-wrap geometry so the custom line gutter remains aligned. The full
   mode keeps its current 24rem minimum and `/playground/` continues rendering
   `<PlaygroundEditor />` with unchanged tabs, default source, and behavior.
   Both modes retain the same CodeMirror extensions, WASM preload, request-ID
   ordering, checker error handling, line gutter, and
   `PlaygroundDiagnostics` composition.
4. In the async Home Server Component, read `explainLeak` at build time and pass
   the returned source to `<PlaygroundEditor mode="embedded"
   initialSource={source} />`. Replace the installation terminal column with
   the value proposition, approved CTA hierarchy, a short fixture-provenance
   label, and the embedded editor. Keep DOM and keyboard order as primary
   Playground CTA, Manifesto, Install, Source, then the single editable
   CodeMirror textbox; use no positive `tabIndex`, and ensure Tab can enter and
   leave the editor using the existing CodeMirror keyboard behavior. Keep the
   wordmark as the only `h1`; use an `h2` for the proposition. Reduce the
   wordmark from its current 184px maximum to roughly 148px and from its 56px
   minimum to roughly 44px, then let the 1280x900 and 390x844 review in WP-07
   confirm the balance.
5. Put editable hero prose in `content.md`. Use the fact-checked copy:
   “Systems programming without safety escape hatches.” and “Linear resources
   and flow-sensitive borrow checking, without lifetime annotations or a
   garbage collector.” State that the browser runs the compiler frontend
   through borrow checking and does not link, execute, or send source to a
   server. Keep install and source as quiet tertiary links.

**Automated verification** (from `site/`)

1. `npm run typecheck`
2. `npm run build`
3. `npm run test:e2e -- tests/e2e/home-playground.spec.ts tests/e2e/playground.spec.ts`

**Acceptance criteria**

- The new focused test fails before implementation and passes afterward.
- `/` statically exports with the exact repository fixture and reaches a real
  WASM error without a network API or server runtime.
- The primary CTA reaches `/playground/` and the full Playground regression
  suite remains green.
- There is one checker loader, one check call path, and one live diagnostics
  renderer.
- The hero has one `h1`; its CTA links precede one keyboard-reachable CodeMirror
  textbox named “Editable Cinnabar source,” typing the shared clean `memory`
  sample changes the verdict from error to “No diagnostics.”, and Tab can leave
  the editor without a positive `tabIndex`.
- The embedded CodeMirror viewport is 12rem with internal vertical/horizontal
  scrolling, and neither the editor nor its diagnostic widens the 390px page.
- No browser copy claims code generation or execution.

### WP-02 — Replace the feature wall with three primary promises

**Type:** expansion  
**Wave:** 2  
**Depends on:** WP-01  
**Decisions:** D-03, D-07, D-10, D-13

**Read first**

- `site/src/app/(home)/page.tsx`
- `site/src/app/(home)/content.md`
- `site/src/content/highlights.ts`
- `site/src/content/pipeline.ts`
- `site/src/app/architecture/page.tsx`
- `site/src/app/reference/page.tsx`
- `site/tests/unit/content-bindings.test.ts`
- `site/tests/unit/repo-docs.test.ts`
- `MANIFESTO.md`

**Modify**

- `site/src/app/(home)/page.tsx`
- `site/src/app/(home)/content.md`
- `site/src/content/highlights.ts`
- `site/tests/unit/content-bindings.test.ts`
- `site/tests/unit/repo-docs.test.ts`

**Action**

1. Replace `@highlights` with a three-item `@promises` block. Rename the
   structural map export from `HIGHLIGHT_ICONS` to `PROMISE_ICONS` and bind
   exactly these slugs: `ownership-without-ceremony`,
   `rules-without-escape-hatches`, and `predictable-runtime-behavior`.
   Assertions in `content-bindings.test.ts` must require exact set equality so
   a missing icon or orphan promise fails in Vitest.
2. Compose the three items editorially using existing `rule-grid`, `Panel`,
   typography, spacing, and reveal primitives. Vary hierarchy/width or section
   treatment so they are not three generic identical cards. Do not add colors,
   gradients, glass, rounded panels, or a new CSS token system.
3. Fold the current “zero-trust” argument into the escape-hatches promise.
   Phrase runtime claims narrowly: dynamic failures are values, self-recursion
   must be in tail position, and valid compiled programs have no
   user-reachable panic. Do not imply all recursion is accepted or all runtime
   inputs are statically decidable.
4. Remove the home pipeline grid and home manifest section from the page
   composition. Keep concise, visible deep links to `/architecture/` and
   `/reference/#manifest`; do not alter either destination or delete shared
   `STAGES`/`MANIFEST_SAMPLE`, which those routes still consume.
5. Add unit coverage for `readRepoFixture("explainLeak")` in
   `repo-docs.test.ts`: assert the returned source includes the fixture's
   rejection marker and `leak_on_one_path`. The API's typed key, rather than a
   caller path, is the allowlist contract.

**Automated verification** (from `site/`)

1. `npm test -- tests/unit/content-bindings.test.ts tests/unit/repo-docs.test.ts`
2. `npm run typecheck`

**Acceptance criteria**

- Exactly three primary promise sections render and all three have structural
  icon bindings.
- Home contains no full seven-stage pipeline grid and no `build.cnb` manifest
  presentation; Architecture and Reference remain intact and linked.
- Promise prose is traceable to `MANIFESTO.md` and contains no invented syntax,
  identifier, or browser capability.
- Existing design tokens and route-adjacent prose ownership remain intact.

### WP-03 — Make resource ownership the first static sample safely

**Type:** expansion, test-first  
**Wave:** 3  
**Depends on:** WP-02  
**Decisions:** D-08, D-10, D-13

**Read first**

- `site/src/content/samples.ts`
- `site/src/content/playground-samples.ts`
- `site/src/components/SampleExplorer.tsx`
- `site/src/app/(home)/content.md`
- `site/tests/e2e/samples.spec.ts`
- `site/tests/e2e/content.spec.ts`
- `tests/fixtures/repro/vec_test.cnb`
- `tests/fixtures/repro/hanoi.cnb`

**Modify**

- `site/src/content/samples.ts`
- `site/src/content/playground-samples.ts`
- `site/src/app/(home)/content.md`
- `site/tests/e2e/samples.spec.ts`
- `site/tests/e2e/content.spec.ts`

**Action**

1. Change the E2E expectation first: the selected home tab on initial load is
   **Linear handles**, its panel carries the `vec_test.cnb` provenance, and the
   existing Arrow/Home/End/wrap/tabindex behavior still works.
2. Reorder `SAMPLES` so the `vec` resource-ownership sample is first, `hanoi`
   remains available, and `slice` remains available. Do not rewrite the sample
   text around the test; it remains fixture-attributed source.
3. Remove `PLAYGROUND_SAMPLES`' positional dependence on `SAMPLES[0]` and
   `SAMPLES[2]`. Export or define a checked ID lookup and retrieve the shared
   `hanoi` and `slice` entries by ID; fail loudly if an expected shared sample
   is absent. Keep the full Playground's default sample and six clean starter
   programs unchanged.
4. Update route-adjacent summaries and token-level content tests for the new
   default. The first panel should teach linear ownership, mutation, explicit
   cleanup, and Result handling; tail recursion becomes a later tab rather than
   disappearing.

**Automated verification** (from `site/`)

1. `npm run build`
2. `npm run test:e2e -- tests/e2e/samples.spec.ts tests/e2e/content.spec.ts tests/e2e/playground.spec.ts`

**Acceptance criteria**

- The home sample explorer starts on **Linear handles** and preserves APG tab
  keyboard behavior.
- The full Playground still starts with its current clean program and all six
  starters check clean.
- No consumer selects a shared sample by array position.
- Tail recursion and slice examples remain reachable and fixture-attributed.

### WP-04 — Retire the illustrative duplicate and finish the status handoff

**Type:** expansion/refinement  
**Wave:** 4  
**Depends on:** WP-03, WP-06  
**Decisions:** D-06, D-10, D-11, D-14, D-15

**Read first**

- `site/src/app/(home)/page.tsx`
- `site/src/app/(home)/content.md`
- `site/src/components/DiagnosticTranscript.tsx`
- `site/src/content/diagnostic.ts`
- `site/src/components/PlaygroundDiagnostics.tsx`
- `site/tests/e2e/diagnostic.spec.ts`
- `site/src/app/roadmap/page.tsx`

**Modify/delete**

- `site/src/app/(home)/page.tsx`
- `site/src/app/(home)/content.md`
- `site/src/components/DiagnosticTranscript.tsx` (delete with a patch only after
  built-in search confirms no remaining import)
- `site/src/content/diagnostic.ts` (delete with a patch only after the same
  reference check)
- `site/tests/e2e/diagnostic.spec.ts`

**Action**

1. Remove the lower fixed illustrative diagnostic from home after the live hero
   proof owns that responsibility. Delete its component/data together only
   after confirming no remaining production import. Do not use `rm`.
2. Rewrite `diagnostic.spec.ts` around the real home
   `PlaygroundDiagnostics`: assert the actual error state is visible, error and
   primary-span accents use the existing terminal palette, its source excerpt
   is readable/focusable, and the component scrolls internally rather than
   widening a 390px page. Remove obsolete box-rail assertions with the deleted
   renderer.
3. Replace the closing milestone count with concise experimental-status copy
   and a Roadmap handoff. Keep links to Roadmap, Install, Architecture, and
   Reference appropriate to the section hierarchy; do not introduce a hand
   maintained capability matrix or progress bar.
4. Leave `QUIP`, `SiteFooter`, the existing easter egg, and brand assets
   unchanged.

**Automated verification** (from `site/`)

1. `npm run build`
2. `npm run test:e2e -- tests/e2e/home-playground.spec.ts tests/e2e/diagnostic.spec.ts tests/e2e/navigation.spec.ts tests/e2e/motion.spec.ts`

**Acceptance criteria**

- Home has one diagnostic proof, and it is produced by the real WASM path.
- The deleted illustrative component/data have no imports or test assumptions.
- At 390px the live proof does not cause document-level horizontal overflow.
- Status copy says experimental/early development and links to Roadmap without
  milestones, percentages, or capability bars.
- The footer-only quip and existing mushroom easter egg remain unchanged.

### WP-05 — Reorder the shared existing-route navigation

**Type:** expansion, test-first  
**Wave:** 2  
**Depends on:** WP-01  
**Decisions:** D-09, D-11, D-12

**Read first**

- `site/src/lib/site.ts`
- `site/src/components/NavLinks.tsx`
- `site/src/components/MobileMenu.tsx`
- `site/src/components/SiteFooter.tsx`
- `site/src/app/sitemap.ts`
- `site/tests/unit/site.test.ts`
- `site/tests/e2e/navigation.spec.ts`

**Modify**

- `site/src/lib/site.ts`
- `site/tests/unit/site.test.ts`
- `site/tests/e2e/navigation.spec.ts`

**Action**

1. Add an exact-order unit assertion for Playground, Manifesto, Install,
   Reference, Architecture, Roadmap, then reorder the existing `NAV` entries to
   satisfy it. Do not rename Manifesto to Language and do not add grouping or
   dropdown state.
2. Add browser assertions that Playground is first in the desktop primary nav,
   open mobile dialog, and documentation footer. Retain active-route behavior,
   mobile focus trapping, close-on-link, footer coverage, and trailing slashes.
3. Do not edit `NavLinks`, `MobileMenu`, `SiteFooter`, or `sitemap.ts`; this task
   must prove they still derive from shared data rather than acquiring copies.

**Automated verification** (from `site/`)

1. `npm test -- tests/unit/site.test.ts`
2. `npm run typecheck`

**Acceptance criteria**

- `NAV` has the locked exact order and unchanged destinations/labels.
- Header, mobile, footer, sitemap, and `ROUTES` still consume the shared source.
- No new route or separate navigation list exists.

### WP-06 — Align metadata, social copy, and reveal selectors

**Type:** expansion, test-first  
**Wave:** 3  
**Depends on:** WP-01, WP-02, WP-05  
**Decisions:** D-04, D-05, D-10, D-13, D-15

**Read first**

- `site/src/lib/site.ts`
- `site/src/app/layout.tsx`
- `site/src/app/(home)/page.tsx`
- `site/src/app/og-image/route.tsx`
- `site/src/lib/og-image.tsx`
- `site/tests/e2e/metadata.spec.ts`
- `site/tests/e2e/motion.spec.ts`

**Modify**

- `site/src/lib/site.ts`
- `site/src/app/layout.tsx`
- `site/src/app/(home)/page.tsx`
- `site/tests/e2e/metadata.spec.ts`
- `site/tests/e2e/motion.spec.ts`

**Action**

1. Add a home-specific metadata test for the approved proposition before
   changing copy. Update `TAGLINE`, `DESCRIPTION`, the root default title, and
   the home `og` object together so HTML metadata, Open Graph, Twitter, and the
   generated social card share the same truthful hierarchy.
2. Metadata may state linear resources, flow-sensitive borrow checking, no
   lifetime annotations, no garbage collector, and rules that cannot be
   disabled. Browser-specific wording must stop at frontend checking through
   borrow checking; do not claim codegen, linking, or execution.
3. Keep descriptions substantive enough for the existing greater-than-50
   character contract, canonical `/`, the current OG route, dimensions, static
   PNG generation, and `SITE_URL` behavior.
4. Update motion test selectors from removed highlight/closing sentences to a
   stable new promise and experimental-status section. Preserve the behavioral
   contracts: content becomes opaque when revealed, reduced motion is final
   immediately, and served HTML contains no hidden reveal content.

**Automated verification** (from `site/`)

1. `npm run typecheck`
2. `npm run build`
3. `npm run test:e2e -- tests/e2e/metadata.spec.ts tests/e2e/motion.spec.ts`

**Acceptance criteria**

- Home title, description, OG, Twitter, and social image copy use the approved
  value proposition and pass exact browser assertions.
- Canonical URLs, route social images, sitemap, and robots behavior remain
  unchanged.
- Reveal behavior remains safe with normal motion, reduced motion, and no
  JavaScript.

### WP-07 — Tranche 1 release verification

**Type:** automated verification plus one human visual/editorial checkpoint  
**Wave:** 5  
**Depends on:** WP-04, WP-05  
**Decisions:** D-03, D-11, D-12, D-13, D-16

Do not modify code during the command sequence. If a check exposes a defect,
return to the owning work package, make the scoped change, rerun its focused
check, and begin WP-07 again only after the code unit is complete.

**Automated verification order**

From `site/`:

1. `npm run lint`
2. `npm run typecheck`
3. `npm test`
4. `npm run build`
5. `npm run test:e2e`
6. `npm run test:links`
7. `npm run test:lighthouse`

Then, from the repository root, run the required gate exactly once for the
completed code unit:

8. `nix develop --command ./pre_commit_check.sh`
9. Read `pre_commit.log` with the built-in Read tool; do not use shell text
   utilities and do not inspect Semgrep rules.

**Manual visual/editorial checkpoint**

Inspect the built `/` page at 1280×900 and 390×844 in dark and light themes,
then with reduced motion. Confirm all of the following:

- the first viewport clearly prioritizes proposition → Playground CTA → real
  rejection;
- the wordmark remains unmistakable but does not crowd the proposition;
- the diagnostic is real, readable, and not presented as browser execution;
- the CTA links precede the single editable source textbox in keyboard order,
  editing rechecks through WASM, and keyboard users can leave the editor;
- no content clips or overflows; long code lines scroll inside the editor;
- the three promises have hierarchy rather than reading as equal generic cards;
- the footer quip and subtle existing mushroom treatment remain secondary;
- both themes preserve contrast and code surfaces remain dark;
- there is no visible milestone arithmetic, fabricated error ID, invalid syntax,
  or un-sourced capability percentage.

**Release acceptance**

- All commands pass against a fresh static export.
- Axe remains clean on every existing route and the mobile dialog.
- Playwright covers the real hero rejection, full Playground behavior,
  navigation, one `h1`, 390px overflow, themes, motion, metadata, and links.
- Visual captures render home in dark, light, and mobile. Because snapshots are
  intentionally uncommitted, the human checkpoint reviews the changed images.
- Lighthouse passes without adding a dependency or server; if the embedded
  editor causes a regression, optimize the existing composition/loading path
  rather than creating a second checker or weakening the budget.
- If, after that implementation and optimization attempt, the editable editor
  still cannot satisfy keyboard, 390px, or Lighthouse acceptance, stop Tranche
  1 and request a separately approved plan revision before introducing any
  read-only behavior. A failing check does not authorize an automatic mode
  switch or a dormant read-only prop.
- Explain in the implementation summary why the general architecture is
  correct: one fixture source, one checker path, one navigation source, one
  route-adjacent copy source, and no fixture-specific compiler behavior.

## Tranche 2 — Documentation information architecture (separate effort)

Tranche 2 begins only after Tranche 1 is released and G-02 is approved. It has
its own code unit, verification cycle, and required repository gate. Do not
continue into these tasks as part of a Tranche 1 implementation run.

### DIA-00 — Audit sources and approve the public IA

**Read**

- `MANIFESTO.md`, `ARCHITECTURE.md`, `README.md`,
  `CONTAINER_DEVELOPMENT.md`
- `tests/fixtures/explain_leak.cnb`
- `tests/fixtures/repro/linear_branch_consume.cnb`
- `site/src/app/install/page.tsx` and `content.md`
- `site/src/app/reference/page.tsx` and `content.md`
- `site/src/app/architecture/page.tsx` and `content.md`
- `site/src/lib/repo-docs.ts`, `site/src/lib/site.ts`

**Create**

- `site/.planning/website-improvement/DOCS-SOURCE-MAP.md`

**Contract**

Map every proposed page/section to an authoritative repository source, exact
headings or fixture paths, permitted claims, and unresolved editorial choices.
The map must cover Install onboarding, contributor development, CLI Reference,
Introduction, Why Cinnabar, Linear Types, Borrowing, Error Handling, First
Program, and each proposed Architecture chapter. Missing sources are blockers,
not invitations to invent prose. Present G-02 after the audit.

### DIA-01 — Separate exported routes from primary navigation

**Depends on:** DIA-00 and G-02  
**Costly contract change:** yes

**Expected files**

- `site/src/lib/site.ts`
- `site/src/lib/docs-navigation.ts` (new only if G-02 approves grouped/nested
  documentation navigation)
- `site/src/components/NavLinks.tsx`
- `site/src/components/MobileMenu.tsx`
- `site/src/components/SiteFooter.tsx`
- `site/src/app/sitemap.ts`
- `site/tests/unit/site.test.ts`
- `site/tests/e2e/routes.ts`
- `site/tests/e2e/navigation.spec.ts`
- `site/tests/e2e/metadata.spec.ts`
- `site/tests/e2e/a11y.spec.ts`
- `site/tests/e2e/visual.spec.ts`

**Contract**

Keep one primary-navigation data source and introduce one explicit all-exported-
routes source so a page can be in the static export/sitemap/test loops without
being a top-level nav item. Do not maintain desktop/mobile/footer lists by hand.
Add route metadata (heading and OG path) in the same registry or a tested
test-only projection; every exported route must be covered by navigation where
appropriate, sitemap, metadata, Axe, overflow, and visual loops.

### DIA-02 — Split public install from contributor development

**Depends on:** DIA-01

**Expected files**

- `site/src/app/install/page.tsx`
- `site/src/app/install/content.md`
- `site/src/app/contributing/development/page.tsx` (new)
- `site/src/app/contributing/development/content.md` (new)
- `site/src/app/contributing/development/og-image/route.tsx` (new)
- the approved route registry from DIA-01
- `site/tests/unit/content-bindings.test.ts`
- `site/tests/e2e/content.spec.ts`
- `site/tests/e2e/metadata.spec.ts`

**Contract**

`/install/` keeps requirements, Nix setup, compiler build, verification, first
project, and editor/LSP setup. Move Docker Desktop, linked worktrees, cache
volumes, Compose internals, container VS Code setup, helper scripts, and the
repository gate explanation to `/contributing/development/`. Move the existing
route-adjacent prose and structural data; do not duplicate it. Update Install
metadata so it no longer advertises contributor/container material. The new
route receives complete metadata, OG output, sitemap coverage, a11y, responsive,
link, and visual tests.

### DIA-03 — Label the existing reference honestly

**Depends on:** DIA-01

**Expected files**

- `site/src/lib/site.ts`
- `site/src/app/reference/page.tsx`
- `site/src/app/reference/content.md`
- `site/tests/unit/site.test.ts`
- `site/tests/e2e/navigation.spec.ts`
- `site/tests/e2e/metadata.spec.ts`

**Contract**

Change the visible label/title to **CLI Reference** while retaining
`/reference/` as the canonical permalink. Keep all current flags, commands,
manifest, test layout, and profile content. Do not call it a language reference.

### DIA-04 — Add the approved Learn path from audited sources

**Depends on:** DIA-00, DIA-01, G-02

**Expected route pairs, only when their source-map rows are APPROVED**

- `site/src/app/learn/page.tsx` + `content.md`
- `site/src/app/learn/why-cinnabar/page.tsx` + `content.md`
- `site/src/app/learn/linear-types/page.tsx` + `content.md`
- `site/src/app/learn/borrowing/page.tsx` + `content.md`
- `site/src/app/learn/error-handling/page.tsx` + `content.md`
- `site/src/app/learn/first-program/page.tsx` + `content.md`
- route-adjacent `og-image/route.tsx` files using the existing shared OG helper
- the approved route registry and existing route-loop tests

**Contract**

Build the learning sequence in source-map order. Use route-adjacent editorial
prose, direct Manifesto/deeper-reference links, and allowlisted repository
fixtures for code. The Linear Types lesson should pair
`explain_leak.cnb` with `linear_branch_consume.cnb` and, if interactive, reuse
the same `PlaygroundEditor`/WASM path. No copied normative document, invented
syntax, or alternative checker is permitted. Each route needs metadata, a
single `h1`, keyboard/a11y coverage, 390px overflow coverage, both-theme visual
review, links, and Lighthouse coverage.

### DIA-05 — Replace the Architecture full-document disclosure with chapters

**Depends on:** DIA-00, DIA-01, G-02

**Expected files**

- `site/src/app/architecture/page.tsx`
- `site/src/app/architecture/content.md`
- `site/src/lib/repo-docs.ts` or a new narrowly tested repository-section
  reader if approved chapters are derived directly from `ARCHITECTURE.md`
- architecture child `page.tsx`/`content.md`/`og-image/route.tsx` files only if
  G-02 selected public chapter URLs
- `site/tests/unit/repo-docs.test.ts`
- `site/tests/e2e/content.spec.ts`
- the route-loop tests from DIA-01

**Contract**

Keep `/architecture/` as the stable overview, preserve the pipeline, flat arena,
and Single-Fact Rule summaries, replace the embedded full-document disclosure
with approved chapter navigation, and retain a direct raw
`ARCHITECTURE.md` GitHub link. Chapter prose must be sourced through the audit;
do not manually create a second architecture document. If G-02 selects anchored
sections rather than child URLs, do not create unused routes.

### Tranche 2 verification boundary

After the approved DIA tasks form their own completed code unit, run the same
full site command sequence as WP-07, manually review every new route in both
themes and at 390px, then run
`nix develop --command ./pre_commit_check.sh` once from the repository root and
read `pre_commit.log` with the built-in Read tool.

## Test coverage matrix

| Concern | Test/contract |
|---|---|
| Fixture reader and content bindings | `tests/unit/repo-docs.test.ts`, `tests/unit/content-bindings.test.ts` |
| Shared navigation order and route contract | `tests/unit/site.test.ts`, `tests/e2e/navigation.spec.ts` |
| Editable real hero WASM rejection, recheck, focus order, and CTA | new `tests/e2e/home-playground.spec.ts` |
| Full Playground remains intact | `tests/e2e/playground.spec.ts` |
| Resource-first tabs and keyboard behavior | `tests/e2e/samples.spec.ts`, `tests/e2e/content.spec.ts` |
| Live diagnostic styling and narrow-screen containment | rewritten `tests/e2e/diagnostic.spec.ts` |
| WCAG and single-heading behavior | `tests/e2e/a11y.spec.ts` |
| Responsive overflow | existing all-route 390px assertion in `tests/e2e/navigation.spec.ts`, plus hero-specific checks |
| Reduced/no motion and no-JS visibility | `tests/e2e/motion.spec.ts` |
| Dark/light behavior and code-surface invariants | `tests/e2e/theme.spec.ts`, `tests/unit/palette.test.ts` |
| Metadata, social PNGs, canonical, sitemap, robots | `tests/e2e/metadata.spec.ts` |
| Visual review | `tests/e2e/visual.spec.ts` plus WP-07 manual first-viewport review |
| Static export and types | `npm run typecheck`, `npm run build` |
| Links | `npm run test:links` against fresh `out/` |
| Performance/accessibility budgets | `npm run test:lighthouse` |
| Repository-wide required gate | `nix develop --command ./pre_commit_check.sh` once after each completed tranche code unit |

## Threat and failure model

| Risk | Mitigation and proof |
|---|---|
| Arbitrary filesystem access from a fixture reader | Accept only a typed allowlist key mapped inside `repo-docs.ts`; unit-test the selected fixture. No caller path. |
| A second compiler fact/checker implementation drifts | Home configures `PlaygroundEditor`; that component remains the sole owner of `checkSource` state and `PlaygroundDiagnostics`. Search and review prove no second hook/renderer. |
| Visitors infer browser execution | Approved copy states frontend through borrow checking and explicitly excludes linking/execution/server submission; E2E/metadata review checks the rendered wording. |
| An invalid invented example becomes public | Read the untouched repository fixture at build time and assert the live checker rejects it. Do not transcribe or rewrite it. |
| Static export accidentally gains runtime requirements | Keep all repository reads in the Home Server Component at build time; `next build` and static `out/` Playwright/link checks are blocking. |
| Editable hero editor harms focus, overflow, motion, or contrast | D-17, Axe, keyboard/edit-recheck tests, 390px overflow, both themes, reduced motion, live diagnostic tests, and manual first-viewport review. Read-only requires a separately approved revision after an attempted editable implementation and optimization. |
| CodeMirror/WASM in the hero harms performance | Lighthouse is blocking; optimize the existing embedded mode and loading path, not by adding a server, dependency, or alternate checker. |
| Navigation copies drift | Modify `NAV` only in Tranche 1; unit and browser tests assert order across every consumer. |
| Public documentation paths later become impossible to change | G-02 treats new URLs as a one-way public contract and keeps existing `/reference/` stable. |

## Artifacts this effort produces

### Definite Tranche 1 artifacts

- `site/tests/e2e/home-playground.spec.ts`
- `RepoFixtureName` and `readRepoFixture` exports in
  `site/src/lib/repo-docs.ts`
- exported `PlaygroundEditorProps` with full and embedded modes in
  `site/src/components/PlaygroundEditor.tsx`
- `PROMISE_ICONS` in `site/src/content/highlights.ts`
- fact-checked `@hero-*`, `@promises`, sample, and status blocks in
  `site/src/app/(home)/content.md`
- focused unit/E2E contracts for fixture provenance, real rejection, shared
  navigation order, resource-first samples, metadata, and live diagnostics

### Tranche 1 removals

- `site/src/components/DiagnosticTranscript.tsx`
- `site/src/content/diagnostic.ts`
- the home-only full pipeline and manifest compositions
- milestone arithmetic and obsolete illustrative-diagnostic test assumptions

### Conditional Tranche 2 artifacts

- `site/.planning/website-improvement/DOCS-SOURCE-MAP.md`
- an explicit all-exported-routes contract and, if approved, one shared nested
  docs-navigation contract
- `/contributing/development/` route files
- approved Learn route pairs and OG handlers
- approved Architecture chapter route files or anchored-section navigation
- updated route-loop tests covering every new static page

## Review recommendation disposition

### Review 1

| ID | Recommendation | Disposition | Plan mapping / reason |
|---|---|---|---|
| R1-01 | Preserve the existing visual identity; fix hierarchy | Adopted | D-03; WP-01/WP-02/WP-07. |
| R1-02 | Make Playground primary; Manifesto secondary; Install/Source tertiary | Adopted | D-05; WP-01. |
| R1-03 | Replace the install terminal with compact editable real proof; move setup down | Adopted | D-06 and D-17; WP-01. Compact editable source is locked for Tranche 1. Read-only is available only through a separately approved revision after the implemented editable path still fails keyboard, 390px, or Lighthouse acceptance following optimization. |
| R1-04 | Sharpen the headline and demote technical detail to support copy | Adopted | WP-01 and WP-06. |
| R1-05 | Reorder nav visitor-first; call Manifesto “Language” | Partially adopted | Existing routes reorder in WP-05. The new label/group is deferred to G-02 because no Language route exists and a misleading link is worse than the current label. |
| R1-06 | Cut home repetition and move detail to depth routes | Adopted | WP-02 and WP-04. |
| R1-07 | Reduce eight cards to four ideas | Conflict resolved | The shared goal is hierarchy; Review 2's three-pillar framing wins, with a factual supporting strip (D-07). |
| R1-08 | Make diagnostics the signature visual | Adopted | Real live diagnostics in WP-01; fixed duplicate retired in WP-04. |
| R1-09 | Reduce wordmark authority 15–25% | Adopted | D-04; concrete sizing target and visual gate in WP-01/WP-07. |
| R1-10 | Use one restrained mushroom reference | Already satisfied | Keep the existing easter egg; D-14 rejects new decorative art. |
| R1-11 | Replace milestone arithmetic with capability/status information | Partially adopted | Honest early/experimental status and Roadmap link in WP-04. Hand-maintained capability rows are rejected because no audited status source exists. |
| R1-12 | Demote the Rust quip | Already satisfied | Footer only under D-14. |
| R1-13 | Avoid gradients, glass, generic cards, excessive animation | Adopted | D-03 and WP-02/WP-07. |

### Review 2

| ID | Recommendation | Disposition | Plan mapping / reason |
|---|---|---|---|
| R2-01 | Tell three primary ideas | Adopted | D-07; WP-02. |
| R2-02 | Make the rejection diagnostic a centerpiece | Concept adopted; sketch rejected | WP-01 uses the real fixture/WASM result. The proposed `E0442A`, postfix `?`, and `if failed` are not publishable facts or syntax (D-13). |
| R2-03 | Replace equal cards with editorial asymmetry | Adopted | WP-02 within existing tokens. |
| R2-04 | Move the full pipeline off home; leave a compact teaser | Adopted as a concise Architecture handoff | WP-02 removes the grid and retains a deep link. Another hand-maintained pipeline graphic is unnecessary duplication. |
| R2-05 | Remove `build.cnb`; show anti-features | Partially adopted | Manifest moves off home in WP-02. A curated subset belongs inside the three promises; a second large anti-feature wall is rejected as another flattened list. |
| R2-06 | Add “Why linear types?” and `/learn/linear-types` | Deferred to Tranche 2 | DIA-00/DIA-04 require an approved source map before educational prose or a public route. |
| R2-07 | Lead code examples with resource ownership; offer VALID/REJECTED tabs | Resource-first adopted; alternate toggle deferred | WP-03 reorders the real sample. The live hero already demonstrates rejection, so a second state/checker interaction is not added without a later learning-page decision. |
| R2-08 | Split Install from contributor infrastructure | Adopted in Tranche 2 | DIA-02, after the separate release boundary. |
| R2-09 | Reorganize global Learn/Docs IA | Deferred with explicit gate | DIA-00/DIA-01 and G-02; source/route decisions are missing. |
| R2-10 | Rename current Reference to CLI Reference and later add language reference | Adopted in Tranche 2 | DIA-03 keeps `/reference/` stable; a language reference requires its own approved source map. |
| R2-11 | Split Architecture into chapters and retain raw source | Adopted conditionally | DIA-05; G-02 decides public child URLs versus anchored chapters. |
| R2-12 | Keep dark/vermilion direction; use asymmetry and mycelium-inspired geometry | Visual hierarchy adopted; palette/art replacement rejected | Existing audited tokens and easter egg remain D-03/D-14. No new art or token palette is needed to solve hierarchy. |
| R2-13 | Proposed seven-section homepage, progress bars, and footer quip | Partially adopted | Hero, contract/promises, proof, resource code, status, deeper links, and footer quip map to WP-01–WP-04. Progress bars and full section proliferation are rejected as unaudited/repetitive. |
| R2-14 | Borrow IA lessons, not appearances, from Zig/Hare/Austral | Adopted as principle | Tranche split and source-led Learn IA; no visual imitation. |

## Multi-source coverage audit

| Source | Item | Covered by | Status |
|---|---|---|---|
| GOAL | First-time visitor sees differentiator, real rejection, and Playground path | WP-01, WP-07 | COVERED |
| RESEARCH | Existing-route Playground-first navigation | WP-05 | COVERED |
| RESEARCH | Value-led hero and truthful browser boundary | WP-01, WP-06 | COVERED |
| RESEARCH | Fixture-backed live rejection through existing checker | WP-01 | COVERED |
| RESEARCH | Three promises plus factual supporting strip | WP-02 | COVERED |
| RESEARCH | Resource-first static code example | WP-03 | COVERED |
| RESEARCH | Remove home pipeline/manifest duplication | WP-02 | COVERED |
| RESEARCH | Replace illustrative diagnostic with live proof | WP-01, WP-04 | COVERED |
| RESEARCH | Experimental status and Roadmap handoff | WP-04 | COVERED |
| RESEARCH | Metadata aligned to proposition | WP-06 | COVERED |
| RESEARCH | Static export, shared NAV, route-adjacent prose, no dependency/server/checker duplication | D-06, D-09–D-12; WP-01, WP-05, WP-07 | COVERED |
| RESEARCH | Accessibility, motion, theme, responsive, links, Lighthouse, full gate | WP-07 | COVERED |
| RESEARCH | Public install/contributor split | DIA-02 | COVERED in separate Tranche 2 |
| RESEARCH | CLI Reference label with stable URL | DIA-03 | COVERED in separate Tranche 2 |
| RESEARCH | Audited Learn path | DIA-00, DIA-04 | COVERED behind G-02 |
| RESEARCH | Architecture chapter navigation/raw source | DIA-00, DIA-05 | COVERED behind G-02 |
| REVIEW 1 | All recommendations R1-01–R1-13 | Review 1 disposition table | COVERED / explicitly resolved |
| REVIEW 2 | All recommendations R2-01–R2-14 | Review 2 disposition table | COVERED / explicitly resolved |
| CONTEXT | Locked decisions D-01–D-17 | Task decision citations and acceptance criteria | COVERED |

No adopted source item is unplanned. Deferred items are assigned to Tranche 2
with a concrete source/decision gate rather than silently entering Tranche 1.
