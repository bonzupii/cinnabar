# Cinnabar Website Improvements - Research

**Researched:** 2026-08-14  
**Scope:** Public-site positioning, homepage/navigation tranche, and a later documentation information-architecture tranche.  
**Confidence:** HIGH for the current implementation and language claims; MEDIUM for the proposed documentation split because it needs editorial decisions before implementation.

## Summary

The site already has a cohesive, distinctive presentation: dark-first hard-edged surfaces, one vermilion accent, Schibsted Grotesk prose, IBM Plex Mono technical text, accessible theme support, restrained reveals, and a full static-export test stack. The core issue is hierarchy, not visual identity. The current home page begins with installation commands and manifesto/install/source actions, gives equal weight to eight highlights, and repeats material that the dedicated Architecture and Reference routes already own. [VERIFIED: `src/app/(home)/page.tsx`, `src/app/(home)/content.md`, `src/app/globals.css`, `site/README.md`]

The strongest existing proof point is not hypothetical: `/playground/` loads a local WASM asset, calls the compiler frontend through borrow checking on every source change, and renders the resulting diagnostics without sending source to a server or executing it. It already has end-to-end tests for a clean initial program, an invalid edit, sample resets, and semantic hover. The first release should promote this exact capability into the home-page hero rather than construct a second, hand-maintained compiler demonstration. [VERIFIED: `src/lib/cinnabar-wasm-client.ts`, `src/components/PlaygroundEditor.tsx`, `src/app/playground/page.tsx`, `src/generated/cinnabar-wasm/README.md`, `tests/e2e/playground.spec.ts`]

**Primary recommendation:** Ship a focused homepage/navigation tranche first: Playground-first navigation, a value-led hero with a compact live rejection fixture, three or four editorially distinct promises, a resource-safety-first code example, and an honest experimental-status handoff. Defer route proliferation, install/contributor splitting, a Learn hierarchy, and Architecture chaptering to a separately scoped documentation-IA tranche.

## Project Constraints (from AGENTS.md)

- Use project built-in read/search/edit/write tools for implementation work; do not use shell commands as convenience substitutes. `rm` is prohibited. [VERIFIED: `AGENTS.md`]
- Do not inspect, search, edit, or otherwise read `.semgrep.yml` or Semgrep rule files. [VERIFIED: `AGENTS.md`]
- Before any code change, read `MANIFESTO.md` and `tests/fixtures/spec.cnb` end to end; treat the manifesto as normative and fixtures as reference data rather than language authority. [VERIFIED: `AGENTS.md`]
- Any implementation that changes code must run the repository gate exactly once per completed unit: `nix develop --command ./pre_commit_check.sh`; documentation-only edits do not require that gate. [VERIFIED: `AGENTS.md`]
- Preserve Cinnabar’s general semantics rather than optimizing website examples around a current fixture. In particular, do not invent string-name-specific semantics, duplicate compiler facts in a second representation, or promote a fixture-only behavior as a language law. [VERIFIED: `AGENTS.md`]
- Before writing future Next.js code, read the relevant guide in `site/node_modules/next/dist/docs/`, because this project uses a version with breaking changes relative to common Next.js conventions. [VERIFIED: `site/AGENTS.md`]

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|---|---|---|---|
| Homepage proposition and content order | Static frontend | — | The site is statically exported; route JSX and adjacent content files determine the first-visit narrative. [VERIFIED: `site/next.config.ts`, `src/app/(home)/page.tsx`, `src/app/(home)/content.md`] |
| Live compiler proof | Browser / client | Static asset delivery | `PlaygroundEditor` owns client state and calls the WASM checker; the static export publishes the generated glue and `.wasm` asset. [VERIFIED: `src/components/PlaygroundEditor.tsx`, `src/lib/cinnabar-wasm-client.ts`, `src/generated/cinnabar-wasm/README.md`, `site/next.config.ts`] |
| Navigation and route discovery | Static frontend | Static metadata | `NAV` drives header, mobile menu, footer, sitemap routes, and current-route state. [VERIFIED: `src/lib/site.ts`, `src/components/NavLinks.tsx`, `src/components/MobileMenu.tsx`, `src/components/SiteFooter.tsx`, `src/app/sitemap.ts`] |
| Normative language claims | Repository documents | Static frontend | The manifesto is the authoritative language specification and site document routes read repository markdown at build time instead of copying it. [VERIFIED: `MANIFESTO.md`, `src/lib/repo-docs.ts`, `site/README.md`] |
| Install versus contributor guidance | Static frontend | Repository documentation | `/install/` currently renders both beginner installation and Docker/worktree/contributor material from its route content and page component. [VERIFIED: `src/app/install/page.tsx`, `src/app/install/content.md`]

## Review Synthesis

### Strong agreements

| Recommendation | Review 1 | Review 2 | Evidence and decision |
|---|---|---|---|
| Lead with proof, not setup | Make Playground the hero action and replace the hero shell with immediate language proof. | Make the diagnostic the visual centerpiece and show ordinary code rejected. | Adopt. The existing browser-local WASM checker is the proof mechanism; reuse it in the hero. [VERIFIED: review inputs; `src/components/PlaygroundEditor.tsx`, `src/lib/cinnabar-wasm-client.ts`] |
| Simplify the product story | Reduce eight equal cards to four big ideas. | Reduce to three pillars. | Adopt as three primary promises plus one supporting outcomes/status row; do not retain an eight-card wall. The current home content has eight highlights. [VERIFIED: review inputs; `src/app/(home)/content.md`, `src/content/highlights.ts`] |
| Move duplicated detail off home | Pipeline belongs on Architecture; manifest belongs on Reference. | Pipeline and `build.cnb` are inside-baseball material. | Adopt. Keep short deep links on home; retain full content on dedicated existing routes. [VERIFIED: review inputs; `src/app/(home)/page.tsx`, `src/app/architecture/page.tsx`, `src/app/reference/page.tsx`] |
| Preserve the visual system | Keep hard edges, near-black surfaces, hairlines, mono metadata, one vermilion accent; avoid generic glass/gradient polish. | Keep restrained dark/vermilion direction; use editorial asymmetry rather than generic cards. | Adopt as a non-negotiable implementation constraint. Existing CSS and tests already encode this system. [VERIFIED: review inputs; `src/app/globals.css`, `tests/unit/palette.test.ts`, `tests/e2e/theme.spec.ts`] |
| Separate public install from contributor infrastructure | Move commands lower on home. | Split Docker/worktree/Compose guidance out of `/install/`. | Adopt, but defer the actual route/content move to documentation IA. `/install/` currently owns both audiences. [VERIFIED: review inputs; `src/app/(home)/page.tsx`, `src/app/install/page.tsx`, `src/app/install/content.md`] |

### Conflicts and resolution

| Topic | Conflict | Resolution |
|---|---|---|
| Global navigation | Review 1 proposes a visitor-first reordering of existing labels; review 2 proposes new Learn/Docs groups. | First tranche only reorders existing, functional destination links: Playground, Manifesto, Install, Reference, Architecture, Roadmap. Do not label `/manifesto/` as “Language” or create dropdowns until the Learn/Docs routes exist. [VERIFIED: review inputs; `src/lib/site.ts`] |
| Hero proof | Review 1 asks for a compact editable example with a real diagnostic; review 2 sketches a large static transcript. | Use a compact instance of the existing live editor/diagnostics path. Do not create another fixed transcript or a fabricated error-code presentation. The existing home transcript is explicitly marked illustrative rather than captured compiler output. [VERIFIED: review inputs; `src/components/PlaygroundEditor.tsx`, `src/components/PlaygroundDiagnostics.tsx`, `src/app/(home)/content.md`] |
| Number of promises | Four big ideas versus three pillars. | Use three primary editorial sections: ownership without ceremony; rules without escape hatches; predictable runtime behavior. Support them with a compact factual outcomes/status strip rather than a fourth equal section. This creates hierarchy while retaining the most important language facts. [VERIFIED: review inputs; `MANIFESTO.md`] |
| Footer quip | Review 1 would demote “Probably better than Rust”; review 2 would keep it in the footer. | Keep the existing footer-only quip. Do not place it in the hero, metadata, navigation, or primary CTA copy. [VERIFIED: review inputs; `src/lib/site.ts`, `src/components/SiteFooter.tsx`] |
| Status presentation | Both reject milestone arithmetic on the home page; review 2 sketches progress bars. | Replace the home milestone count with a concise experimental-status block and a Roadmap link. Do not hand-maintain capability percentages or “working” claims unless they derive from an auditable source; those decay faster than the current route content. [VERIFIED: review inputs; `src/app/(home)/content.md`, `src/app/roadmap/page.tsx`, `src/content/roadmap.ts`] |

## Normative Fact Check for Proposed Copy

| Proposed claim or example | Verdict | Safe implementation copy / consequence |
|---|---|---|
| “Linear resources. Flow-sensitive borrow checking. No lifetime annotations.” | Confirmed. Native handles must be consumed exactly once on every execution path; borrow scopes are flow-sensitive; lifetime annotation syntax does not exist. [VERIFIED: `MANIFESTO.md`, “No Lifetime Annotations” and “Linear Types for Resource Management”] | Safe hero support copy: “Linear resources and flow-sensitive borrow checking, without lifetime annotations.” |
| “No safety escape hatches”; no `#[allow]`, warning levels, or suppressions. | Confirmed. The specification states that diagnostics are errors or silence and forbids `#[allow]`, lint configuration, and warning severity. [VERIFIED: `MANIFESTO.md`, “Errors Only, Never Warnings”] | Safe primary proposition: “Systems programming without safety escape hatches.” |
| “No GC / no reference counting / no exceptions / no macros / no operator overloading / no async / no trait objects.” | Confirmed as explicit anti-principles. [VERIFIED: `MANIFESTO.md`, “Anti-Principles (Things Cinnabar Will Never Have)”] | Use only a small curated subset in the first home tranche; place the complete anti-feature list in later learning/reference content. |
| “No user-reachable panics”; runtime failures become values where dynamic. | Confirmed with the specification’s narrower guarantee: valid compiled programs run without crashing or panicking, while dynamic division and indexing surface `Result` errors. [VERIFIED: `MANIFESTO.md`, “The Crucible Rule” and “Runtime Guarantees”] | Use “No user-reachable panics” only with adjacent explanation or link; do not imply that every input is statically decidable. |
| “Strict tail recursion” / bounded stack. | Confirmed only for self-recursive calls in strict tail position; the compiler rejects non-tail self-recursion and relies on LLVM tail-call elimination. [VERIFIED: `MANIFESTO.md`, “Runtime Guarantees”] | Phrase precisely: “Self-recursion is required to be in tail position.” Do not claim every recursive algorithm is accepted or that arbitrary recursion is stack-safe. |
| “Native static binaries / LLVM / static musl.” | Confirmed for the documented compiler and native surface; the installation route identifies LLVM 21 and a static musl libc, while the manifesto describes static musl linkage where libc is needed. [VERIFIED: `src/app/install/content.md`, `MANIFESTO.md`, “Native Surface: Operating System”] | Use as a supporting capability, not as the opening proposition. |
| Proposed sketch `val vec = vec_new[I64]()?` plus `if failed`. | Rejected as publishable Cinnabar example. The normative propagation form is `try`; `if` branches on an expression, and the review sketch does not provide the native declarations/imports needed for a complete program. [VERIFIED: `MANIFESTO.md`, “Explicit Over Implicit, Always”, “Control Flow”; `tests/fixtures/spec.cnb`] | Do not place this sketch in code UI. Source the hero’s rejected program from the self-contained `tests/fixtures/explain_leak.cnb` fixture. |
| Proposed `error E0442A`. | Unverified. The normative specification does not establish this code, and the current site diagnostic data renders `Error:` without an error code. [VERIFIED: `MANIFESTO.md`; `src/content/diagnostic.ts`; `src/components/PlaygroundDiagnostics.tsx`] | Do not publish an invented diagnostic code. Let the actual WASM result supply its own message and location. |
| “The real lexer, resolver, typechecker, and borrow checker run client-side.” | Confirmed with one important boundary: the WASM client exposes frontend checking through borrow checking; it does not link, execute, or send the submitted source to a server. [VERIFIED: `src/lib/cinnabar-wasm-client.ts`, `src/app/playground/page.tsx`, `src/generated/cinnabar-wasm/README.md`] | Use the current playground wording or a shortened equivalent; do not claim LLVM code generation or execution happens in the browser. |

## Recommended Scope Split

### Tranche 1 - Homepage and existing-route navigation

**Goal:** A first-time systems programmer can see Cinnabar’s differentiator, witness a real rejection, and reach the full playground in one interaction without being required to read installation or architecture material first.

**In scope**

1. Reorder the existing navigation to place `/playground/` first; retain all existing routes and labels in this tranche. [VERIFIED: `src/lib/site.ts`]
2. Replace the hero’s installation-terminal emphasis with a value-led headline, supporting claim, primary Playground action, secondary Manifesto action, and tertiary install/source links. [VERIFIED: `src/app/(home)/page.tsx`, `src/app/(home)/content.md`]
3. Render a compact, live, fixture-backed rejected example in the hero using the existing WASM/editor/diagnostics path. Use the self-contained resource-leak fixture, not invented pseudocode or a second diagnostic renderer. [VERIFIED: `tests/fixtures/explain_leak.cnb`, `src/components/PlaygroundEditor.tsx`, `src/components/PlaygroundDiagnostics.tsx`]
4. Replace the eight equal home highlights with three primary promises and a lightweight factual supporting strip. Move detailed feature material to links and dedicated routes. [VERIFIED: `src/app/(home)/content.md`, `src/content/highlights.ts`]
5. Make the homepage code-example entry point resource ownership first rather than tail recursion first; retain tail recursion as a secondary example. [VERIFIED: `src/content/samples.ts`, `src/components/SampleExplorer.tsx`]
6. Remove the full pipeline and manifest sections from the homepage, retaining concise Architecture and Reference links. [VERIFIED: `src/app/(home)/page.tsx`]
7. Replace “six of eight milestones” with an experimental-status statement and Roadmap handoff; avoid un-sourced percentage/progress-bar claims. [VERIFIED: `src/app/(home)/content.md`, `src/app/roadmap/page.tsx`]

**Explicitly out of scope**

- New Learn, Language, Docs, or Contributing routes.
- A true language-reference rewrite.
- Architecture chapter routes.
- New packages, analytics, a server/API, or a second compiler/WASM bundle.
- A visual rebrand, gradients/glass effects, mascot/artwork expansion, or a new design system. [VERIFIED: `site/package.json`, `src/app/globals.css`, review inputs]

### Tranche 2 - Documentation information architecture

**Goal:** Separate evaluation/onboarding material from contributor operations, make route labels match their contents, and provide a progressive learning path without duplicating the normative source documents.

**In scope after a separate content audit**

1. Reduce `/install/` to requirements, Nix setup, compiler build, verification, first project, and editor/LSP setup. [VERIFIED: review input; `src/app/install/page.tsx`, `src/app/install/content.md`]
2. Move Docker Desktop, worktrees, Compose volumes, VS Code container setup, and the contributor gate explanation to `/contributing/development/`. [VERIFIED: review input; `src/app/install/page.tsx`, `src/app/install/content.md`]
3. Rename the current `/reference/` presentation to “CLI Reference” while keeping its existing URL as a stable permalink; build a genuine language reference only once its sections are sourced and reviewed. [VERIFIED: `src/app/reference/page.tsx`, `src/app/reference/content.md`]
4. Introduce Learn content only after selecting an authoritative source for each page: introduction, why Cinnabar, linear types, borrowing, error handling, and first program. [ASSUMED] The current route content does not yet provide an audited source map for this new educational content.
5. Replace the Architecture page’s embedded full-document disclosure with chapter-level navigation and a direct raw-document link, while preserving a single source of truth for technical claims. [VERIFIED: `src/app/architecture/page.tsx`, `src/lib/repo-docs.ts`, review input]

## Tranche 1 Implementation Map

| Change | Primary files | Supporting files | Required behavior |
|---|---|---|---|
| Navigation order | `src/lib/site.ts` | `NavLinks.tsx`, `MobileMenu.tsx`, `SiteFooter.tsx`, `app/sitemap.ts` | Reorder the shared `NAV` data only; all three navigation surfaces and sitemap continue to use the same route source. [VERIFIED: `src/lib/site.ts`, `src/components/NavLinks.tsx`, `src/components/MobileMenu.tsx`, `src/components/SiteFooter.tsx`, `src/app/sitemap.ts`] |
| Hero hierarchy | `src/app/(home)/page.tsx` | `src/app/(home)/content.md`, `src/lib/site.ts` | Keep the wordmark as the sole home `h1`; put value proposition and action hierarchy before setup instructions. [VERIFIED: `src/app/(home)/page.tsx`, `tests/e2e/a11y.spec.ts`] |
| Live hero proof | `src/components/PlaygroundEditor.tsx` | `src/components/PlaygroundDiagnostics.tsx`, `src/lib/cinnabar-wasm-client.ts`, `src/content/playground-samples.ts`, new server-only fixture reader if needed | Add a compact/embedded mode or equivalent prop-driven composition to the existing workbench; it must show the real checker’s output for fixture-backed input and link to full `/playground/`. [VERIFIED: `src/components/PlaygroundEditor.tsx`, `src/components/PlaygroundDiagnostics.tsx`, `src/lib/cinnabar-wasm-client.ts`, `tests/fixtures/explain_leak.cnb`] |
| Fixture provenance | New `src/lib/repo-fixtures.ts` or a narrowly extended `src/lib/repo-docs.ts` | `src/app/(home)/page.tsx`, tests | Read only an allowlisted repository fixture at static build time and pass its text into the client editor. Do not duplicate a manually transcribed rejection fixture in a TypeScript string. [VERIFIED: `src/lib/repo-docs.ts`, `tests/fixtures/explain_leak.cnb`, `site/next.config.ts`] |
| Three-promise content | `src/app/(home)/content.md` | `src/content/highlights.ts`, `src/app/(home)/page.tsx`, `tests/unit/content-bindings.test.ts` | Replace the eight `@highlights` items and icon mapping together. Keep claims traceable to `MANIFESTO.md`; avoid an equal-weight grid that recreates the current hierarchy. [VERIFIED: `src/app/(home)/content.md`, `src/content/highlights.ts`, `tests/unit/content-bindings.test.ts`, `MANIFESTO.md`] |
| Resource-first sample | `src/content/samples.ts` | `src/components/SampleExplorer.tsx`, `src/app/(home)/content.md`, `tests/e2e/samples.spec.ts` | Make the linear-resource sample selected first, or add an explicit resource-focused first panel; keep keyboard tabs and source attribution intact. [VERIFIED: `src/content/samples.ts`, `src/components/SampleExplorer.tsx`, `tests/e2e/samples.spec.ts`] |
| Remove homepage duplication | `src/app/(home)/page.tsx` | `src/app/(home)/content.md`, `src/content/pipeline.ts`, `src/content/samples.ts` | Delete home-only pipeline/manifest composition and their stale copy blocks only after dedicated links remain. Do not delete the Architecture or CLI Reference content they point to. [VERIFIED: `src/app/(home)/page.tsx`, `src/app/architecture/page.tsx`, `src/app/reference/page.tsx`] |
| Status handoff | `src/app/(home)/content.md` | `src/app/(home)/page.tsx`, `src/app/roadmap/page.tsx`, `src/content/roadmap.ts` | State experimental/early-development status and link to Roadmap. Do not use milestone arithmetic or manually drawn capability progress bars as a source of truth. [VERIFIED: `src/app/(home)/content.md`, `src/lib/site.ts`, `src/app/roadmap/page.tsx`] |
| Metadata | `src/app/(home)/page.tsx` | `src/app/layout.tsx`, `src/lib/site.ts`, `src/app/og-image/route.tsx` | Update title/description/OG copy to the approved value proposition so search/social presentation does not retain the old README-first hierarchy. [VERIFIED: `src/app/(home)/page.tsx`, `src/app/layout.tsx`, `tests/e2e/metadata.spec.ts`] |

## Recommended Homepage Information Flow

```text
Header: Playground · Manifesto · Install · Reference · Architecture · Roadmap
  |
Hero: value proposition + primary “Try in Playground” action
  |
Compact live fixture-backed rejection (browser WASM, no execution)
  |
Three primary promises: ownership / no escape hatches / predictable runtime behavior
  |
Resource-first code sample with valid/rejected learning handoff
  |
Experimental status + Roadmap / Installation / Architecture links
```

The flow changes only the homepage’s explanatory order. The full Playground, Manifesto, CLI Reference, Architecture, Install, and Roadmap remain the depth destinations. [VERIFIED: `src/lib/site.ts`, `src/app/(home)/page.tsx`]

## Architecture Patterns

### Reuse the single live-checker path

`PlaygroundEditor` already owns source state, asynchronous request ordering, WASM preloading, sample selection, and rendering `PlaygroundDiagnostics`. Home should configure that component rather than making another hook around `checkSource`, another import of the WASM module, or a second diagnostic-to-DOM translation. [VERIFIED: `src/components/PlaygroundEditor.tsx`, `src/lib/cinnabar-wasm-client.ts`, `src/components/PlaygroundDiagnostics.tsx`]

**Recommended shape:** retain one editor component and add a documented embedded mode with explicit initial source/sample inputs and the smallest UI necessary for the hero. The full playground keeps the tab list and all six starter samples. The home page provides a clearly labeled link to the full playground. [ASSUMED] The existing component can be parameterized without creating an accessibility regression; implementation must confirm this against the current Next.js documentation and end-to-end suite.

### Keep prose in route-adjacent content files

Home prose belongs in `src/app/(home)/content.md`; JSX composes sections and TypeScript carries structural pairings such as icons and sample order. The parser and `content-bindings` tests already fail loudly if those two layers drift. [VERIFIED: `src/lib/page-content.ts`, `site/README.md`, `tests/unit/content-bindings.test.ts`]

### Keep normative claims linked to source, not a site-only copy

The current manifesto/roadmap/architecture routes read repository markdown at build time specifically to prevent stale copies. New learning copy should cite or derive from the manifesto and fixture corpus, with an explicit source audit before it becomes a separate route. [VERIFIED: `src/lib/repo-docs.ts`, `site/README.md`, `MANIFESTO.md`]

## Don't Hand-Roll

| Problem | Do not build | Use instead | Why |
|---|---|---|---|
| Live compiler proof | A homepage-specific checker hook or second WASM loader | `PlaygroundEditor`, `checkSource`, and `PlaygroundDiagnostics` | The existing path already handles preload, stale async responses, checker load failures, and diagnostic locations. [VERIFIED: `src/components/PlaygroundEditor.tsx`, `src/lib/cinnabar-wasm-client.ts`, `src/components/PlaygroundDiagnostics.tsx`] |
| Rejected language example | Pseudocode, fabricated source spans, or made-up diagnostic identifiers | `tests/fixtures/explain_leak.cnb` delivered through the real checker | The fixture is complete and intentionally rejects a one-path resource leak; actual output remains owned by the compiler. [VERIFIED: `tests/fixtures/explain_leak.cnb`] |
| Navigation copies | Separate desktop, mobile, and footer link lists | Shared `NAV` data | Existing shared data keeps labels, paths, icons, blurbs, active state, footer, and sitemap in sync. [VERIFIED: `src/lib/site.ts`, `src/components/NavLinks.tsx`, `src/components/MobileMenu.tsx`, `src/components/SiteFooter.tsx`] |
| Technical source documents | Copied manifesto/architecture markdown in site content | `readRepoDoc` and source-document links | Repository markdown remains the source of truth and repo-relative links are rewritten at build time. [VERIFIED: `src/lib/repo-docs.ts`, `tests/e2e/content.spec.ts`] |
| Visual polish | A parallel card/gradient/glass system | Existing CSS tokens, windows, rule grids, and terminal palette | Existing theme/contrast, responsive, motion, and diagnostic-rail tests protect these design constraints. [VERIFIED: `src/app/globals.css`, `tests/unit/palette.test.ts`, `tests/e2e/visual.spec.ts`, `tests/e2e/diagnostic.spec.ts`] |

## Common Pitfalls and Mitigations

### Claiming more than the browser actually runs

The current checker stops after borrow checking; it does not link, execute, or run LLVM code generation in the browser. Hero copy must say “frontend through borrow checking” or reuse equivalent existing copy. [VERIFIED: `src/lib/cinnabar-wasm-client.ts`, `src/app/playground/page.tsx`]

### Letting a manually copied fixture drift

The current static home samples are display excerpts, while playground samples are complete programs. A new hero rejection must be fixture-backed and covered by a test that proves its live report is erroneous; otherwise a compiler grammar change can leave the home proof invalid for the wrong reason or clean unexpectedly. [VERIFIED: `src/content/samples.ts`, `src/content/playground-samples.ts`, `tests/e2e/playground.spec.ts`, `tests/fixtures/explain_leak.cnb`]

### Reintroducing duplicate diagnostic presentation

The current static home transcript is explicitly “Illustrative · not captured compiler output.” Moving it upward without changing its provenance would make a reader infer that it is an exact live compiler result. The hero’s central diagnostic should come from `PlaygroundDiagnostics`; retain, relocate, or remove the illustrative rail component only with its label and dedicated rail tests updated deliberately. [VERIFIED: `src/app/(home)/content.md`, `src/content/diagnostic.ts`, `src/components/DiagnosticTranscript.tsx`, `tests/e2e/diagnostic.spec.ts`]

### Breaking the editor’s line-number geometry

The playground deliberately disables wrapping because its custom gutter has one row per logical source line. A compact hero mode must retain horizontal scrolling for long lines or change both the editor and gutter design together. [VERIFIED: `src/components/PlaygroundEditor.tsx`]

### Breaking static-export route coverage in later docs IA

The site exports static directories with trailing slashes, while current tests assume `ROUTES` equals home plus `NAV`. Adding non-navigation pages later requires separating “all exported routes” from “primary navigation” before sitemap, metadata, visual, accessibility, and navigation tests can remain accurate. [VERIFIED: `site/next.config.ts`, `src/lib/site.ts`, `tests/unit/site.test.ts`, `tests/e2e/routes.ts`, `tests/e2e/navigation.spec.ts`, `tests/e2e/metadata.spec.ts`]

### Hiding content behind motion or losing keyboard semantics

The site safeguards reveal content for no-JavaScript and reduced-motion users, while the homepage sample tabs implement keyboard navigation. Any new embedded editor/sample control must preserve these constraints and one-`h1` page structure. [VERIFIED: `src/app/globals.css`, `src/components/Reveal.tsx`, `src/components/SampleExplorer.tsx`, `tests/e2e/motion.spec.ts`, `tests/e2e/a11y.spec.ts`, `tests/e2e/samples.spec.ts`]

## Validation Architecture

### Existing test framework

| Property | Value |
|---|---|
| Unit framework | Vitest 3.2.4, configured for `tests/unit/**/*.test.ts`. [VERIFIED: `site/package.json`, `site/vitest.config.ts`] |
| Browser framework | Playwright 1.61.1, serving the static `out` export. [VERIFIED: `site/package.json`, `site/playwright.config.ts`] |
| Accessibility | axe-core Playwright checks WCAG 2.0/2.1 A and AA tags for each listed route. [VERIFIED: `tests/e2e/a11y.spec.ts`] |
| Visual coverage | Full-page dark/light route snapshots plus a mobile-home snapshot; snapshots are intentionally not committed. [VERIFIED: `tests/e2e/visual.spec.ts`, `site/README.md`] |
| Site commands for implementation | `npm run lint`, `npm run typecheck`, `npm test`, `npm run build`, `npm run test:e2e`, `npm run test:links`, `npm run test:lighthouse`. [VERIFIED: `site/package.json`, `site/README.md`] |

### Tranche 1 requirements to test map

| Requirement | Test change | Acceptance criterion |
|---|---|---|
| Playground is first in desktop and mobile navigation | Update `tests/unit/site.test.ts` and `tests/e2e/navigation.spec.ts`. | Shared `NAV` order is asserted; every header/mobile/footer link still resolves and marks the current section. [VERIFIED: `src/lib/site.ts`, `tests/unit/site.test.ts`, `tests/e2e/navigation.spec.ts`] |
| Hero provides a real rejection | Add a focused home-playground assertion, preferably in a new `tests/e2e/home-playground.spec.ts`. | On `/`, the embedded fixture reaches a real `Error` state; following its CTA opens `/playground/`; no assertion hard-codes an invented error ID. [VERIFIED: `tests/e2e/playground.spec.ts`, `tests/fixtures/explain_leak.cnb`] |
| Full playground remains intact | Extend `tests/e2e/playground.spec.ts` only as needed. | The standalone page still starts clean, reports invalid edits, resets samples, and retains hover behavior. [VERIFIED: `tests/e2e/playground.spec.ts`] |
| Resource-first example retains tabs accessibility | Update `tests/e2e/samples.spec.ts`. | The default resource-focused tab is selected; arrows/Home/End and tab semantics remain correct. [VERIFIED: `src/components/SampleExplorer.tsx`, `tests/e2e/samples.spec.ts`] |
| Home content bindings remain complete | Update `tests/unit/content-bindings.test.ts`. | Every retained promise/icon and sample summary has a matching content block; removed keys have no orphan mapping. [VERIFIED: `tests/unit/content-bindings.test.ts`, `src/content/highlights.ts`] |
| One accessible, responsive hero | Update/extend `tests/e2e/a11y.spec.ts` and `tests/e2e/navigation.spec.ts`. | Axe remains clean, exactly one `h1` remains, primary CTA has a descriptive accessible name, and 390px pages have no horizontal overflow. [VERIFIED: `tests/e2e/a11y.spec.ts`, `tests/e2e/navigation.spec.ts`] |
| Visual system remains intentional | Update `tests/e2e/visual.spec.ts`; inspect dark, light, and mobile renderings. | New composition is captured in visual baselines without changing theme-token, code-surface, or diagnostic-contrast behavior. [VERIFIED: `tests/e2e/visual.spec.ts`, `tests/e2e/theme.spec.ts`, `src/app/globals.css`] |
| Illustrative diagnostic is not silently misrepresented | Update `tests/e2e/diagnostic.spec.ts` if `DiagnosticTranscript` moves or is removed. | Either its new location retains rail coverage and explicit illustrative labeling, or the test/component are removed together after the live hero test replaces its user-facing responsibility. [VERIFIED: `tests/e2e/diagnostic.spec.ts`, `src/components/DiagnosticTranscript.tsx`] |
| Metadata and static export stay correct | Update `tests/e2e/metadata.spec.ts` as hero metadata changes. | Each existing route retains title, description, canonical URL, social image, sitemap entry, and robots/sitemap relationship. [VERIFIED: `tests/e2e/metadata.spec.ts`, `src/app/sitemap.ts`, `src/app/robots.ts`] |

### Implementation verification order

1. Run the required repository gate once after the completed code unit: `nix develop --command ./pre_commit_check.sh`. [VERIFIED: `AGENTS.md`]
2. From `site/`, run lint, typecheck, Vitest, static build, Playwright, links, and Lighthouse in the documented dependency order; Playwright serves the built `out` directory. [VERIFIED: `site/README.md`, `site/package.json`, `site/playwright.config.ts`]
3. Manually inspect the first viewport at desktop and 390px in both themes, with normal and reduced motion, to confirm the proposition, CTA hierarchy, fixture error, and no overflow are understandable without scrolling. [ASSUMED] Manual editorial review is required because automated tests cannot judge hierarchy or clarity.

## Package and Environment Assessment

No new dependency is recommended for either tranche. The current site already contains Next.js, React, CodeMirror, WASM glue, Vitest, Playwright, Axe, link checking, and Lighthouse support. Therefore no package legitimacy audit or package installation task is required. [VERIFIED: `site/package.json`, `src/components/PlaygroundEditor.tsx`, `src/lib/cinnabar-wasm-client.ts`]

The browser proof depends on the committed generated WASM glue plus `public/wasm/cinnabar_wasm_bg.wasm`; Netlify does not regenerate it. Any future compiler/WASM update must regenerate and commit the site assets with the compiler change, but the homepage/layout work itself should not change that bundle. [VERIFIED: `src/generated/cinnabar-wasm/README.md`, `public/wasm/cinnabar_wasm_bg.wasm`]

## Assumptions Log

| # | Claim | Section | Risk if wrong |
|---|---|---|---|
| A1 | The existing editor can gain a compact embedded mode without needing a new component or breaking its current CodeMirror/accessibility behavior. | Architecture Patterns | A small component extraction may be needed; do not duplicate checker state. |
| A2 | Later Learn pages will need new audited source mapping rather than merely rearranging current copy. | Tranche 2 | The scope may expand into language-documentation authorship and require user review of educational wording. |
| A3 | Manual visual/editorial review is necessary in addition to the automated suite. | Validation Architecture | A technically passing build could still preserve a weak first-viewport hierarchy. |

## Resolved Implementation Decision

### D-17: compact editable hero source
**RESOLVED:** Tranche 1 uses compact editable source in the embedded existing
CodeMirror instance through the existing `PlaygroundEditor` -> `checkSource` ->
`PlaygroundDiagnostics` checker path. Every source edit rechecks through the
existing WASM path, and the full Playground CTA remains visible. The planned
implementation adds no read-only prop or mode. [VERIFIED:
`PLAN.md:83`, `PLAN.md:108-119`, `PLAN.md:253-266`]

Read-only may be proposed only in a separately approved plan revision after the
implemented editable composition has received reasonable optimization and still
cannot meet the keyboard, 390px overflow/viewport, or Lighthouse acceptance
criteria. A failed gate does not authorize an automatic mode switch or a dormant
read-only configuration. [VERIFIED: `PLAN.md:83`, `PLAN.md:115-119`,
`PLAN.md:654-661`]

## Open Questions

1. **What is the approved home status vocabulary beyond “early development”?**
   - What we know: The shared status badge is “early development,” and the Roadmap is the existing detailed status destination. [VERIFIED: `src/lib/site.ts`, `src/app/roadmap/page.tsx`]
   - What is unclear: Whether labels such as “Compiler: working” are sufficiently source-backed and stable for public copy.
   - Recommendation: Do not add capability percentages in tranche 1; use the existing experimental status and link to Roadmap until a maintained status source is defined.

2. **Which source governs a future “Why linear types?” teaching page?**
   - What we know: The manifesto is normative, and `explain_leak.cnb`/`linear_branch_consume.cnb` demonstrate rejected and accepted path behavior. [VERIFIED: `MANIFESTO.md`, `tests/fixtures/explain_leak.cnb`, `tests/fixtures/repro/linear_branch_consume.cnb`]
   - What is unclear: The desired level of tutorial prose, diagrams, and interactive comparison.
   - Recommendation: Treat it as a later, separately reviewed Learn-page specification rather than inserting it into the homepage tranche.

## Sources

### Primary (HIGH confidence)

- `MANIFESTO.md` - normative language rules and anti-principles.
- `tests/fixtures/spec.cnb` - full reference fixture read end to end.
- `tests/fixtures/explain_leak.cnb` and `tests/fixtures/repro/linear_branch_consume.cnb` - self-contained rejected/accepted linear-path examples.
- `site/src/app/(home)/page.tsx`, `site/src/app/(home)/content.md`, `site/src/lib/site.ts` - current homepage and navigation structure.
- `site/src/components/PlaygroundEditor.tsx`, `site/src/lib/cinnabar-wasm-client.ts`, `site/src/app/playground/page.tsx` - current live checker behavior.
- `site/src/app/install/page.tsx`, `site/src/app/reference/page.tsx`, `site/src/app/architecture/page.tsx` and adjacent `content.md` files - current documentation boundaries.
- `site/tests/e2e/*.spec.ts` and `site/tests/unit/*.test.ts` listed in the validation map - current regression coverage.

### Secondary (MEDIUM confidence)

- The two supplied website reviews - product/IA recommendations, assessed against the codebase and manifesto rather than treated as sources of language truth.

## Metadata

**Confidence breakdown:**

- Current stack and test coverage: HIGH - read directly from site source and configuration.
- Language-claim fact check: HIGH - read directly from `MANIFESTO.md` and the reference fixture.
- First-tranche architecture: HIGH - reuses established components and data flow.
- Later documentation IA: MEDIUM - direction is clear, but educational source mapping and final route taxonomy remain product decisions.

**Valid until:** Current-code findings should be revisited if the website or compiler/WASM interface changes; editorial recommendations remain valid until the project chooses a different public audience.
