# Cinnabar site

The public site for [Cinnabar](https://github.com/bonzupii/cinnabar), built with
Next.js 16 and exported as static files.

The design is not invented here. Every colour, typeface, mark and layout device
comes from the brand board in
[`.planning/brand/designs/Cinnabar Brand.dc.html`](.planning/brand/designs/), and
the source comments cite the plate each decision comes from.

```bash
npm install
npm run dev          # http://localhost:3000
npm run build        # static export into out/
```

## Where the words live

Each route keeps its prose in a `content.md` beside its `page.tsx`, so changing
wording never means reading JSX. A page is rarely one continuous document, so
the file is divided into named blocks:

```markdown
<!-- @lede -->
Markdown for the lede.

<!-- @closing -->
Markdown for the closing section.
```

The page pulls them by name (`content.block("lede")`). A missing block fails the
build rather than rendering an empty section. Text before the first marker is
the `body` block, which is all a prose-only page needs.

A block that carries a repeated thing — a capability, a CLI flag, a table row —
is divided again by `###` headings, and `content.items(name)` returns those in
file order. The heading is the name of the thing and the paragraph under it is
its description, which is the point: a flag and what it does cannot drift apart
when they are the same paragraph.

```markdown
<!-- @project-rows -->

### cinnabar build [PATH] [--target host]

Compiles the project the manifest describes.
```

`content.item(name, slug)` fetches one by the **slug of its heading**, and
`content.list(name)` reads a block's top-level bullets as plain strings.

### What stays in TypeScript

`src/content/*.ts` keeps structure, never prose: which entries exist, what order
they appear in, which icon each is drawn with, which anchor each links to. The
two halves are bound by slug, so rewording a heading breaks the build instead of
silently pairing the new title with the wrong icon —
`tests/unit/content-bindings.test.ts` turns that into a one-second unit failure
naming the block and the key, rather than a build failure naming whichever
component happened to read it.

Three things deliberately stayed as data rather than moving to markdown:

- **`USAGE`** in `src/content/cli.ts` is a literal transcription of what
  `--help` prints. It is a fixed-width block whose line breaks are the content.
- **`TEST_PROFILES`** is a matrix, not a list of descriptions. Its cells are
  read across rows as well as down columns, which markdown headings cannot
  express.
- **`src/content/diagnostic.ts`** is a diagnostic transcript tagged span by span
  with the role each part plays, and its column alignment is load-bearing. It is
  data structured for a renderer, not copy anyone edits for tone.

The home page lives in a `(home)` route group so it has a folder of its own like
every other route; a route group does not appear in the URL.

### The repository's own documents

`/manifesto`, `/roadmap` and `/architecture` additionally render `MANIFESTO.md`,
`ROADMAP.md` and `ARCHITECTURE.md` from the repository root, read at build time
by `src/lib/repo-docs.ts`. They are not copied here — `MANIFESTO.md` is
normative, and a stale copy of a normative spec is worse than no copy.
Repo-relative links inside those documents are rewritten: a document with its
own page here links to that page, everything else links into GitHub.

## Live GitHub data

The roadmap page carries a five-row feed of the most recent commits on `main`.
It is on that page and nowhere else: the roadmap is where a reader goes to ask
whether the project is alive, and one instance spends one of a reader's sixty
unauthenticated GitHub requests rather than two.

It is built in two layers, in `src/lib/github.ts` and
`src/components/ActivityFeed.tsx`:

1. **The build fetches the list and prerenders it into the HTML.** That is what
   makes the section correct with JavaScript off, for a crawler, and when the
   API cannot be reached at all. It is accurate at deploy and goes stale
   afterwards.
2. **The browser fetches it again on mount and swaps it in.** That is the live
   half. GitHub's REST API sends `Access-Control-Allow-Origin: *`, so this works
   from a static host with no server and no secret.

There is no Netlify Function. The unauthenticated limit is 60 requests per hour
**per originating IP**, so the budget is the reader's and a normal visit spends
one of sixty; the result is cached in `sessionStorage` for ten minutes, so
browsing several pages spends nothing more. A function would raise the ceiling
to 5,000/hour, but only by introducing a token to store and rotate, a runtime
dependency where there is currently none, and a second thing that can be down —
and layer 1 already covers the case it would exist to prevent.

**What a reader sees when it fails.** Nothing changes. Every failure — offline,
DNS, blocked by an extension, 403 rate limit, a captive portal's HTML, a
malformed entry — comes back from `fetchCommits` as an empty list, and an empty
list means "keep what the build gave you". There is no spinner, no error, and no
retry: there is already something correct on screen. If the *build* could not
reach GitHub either, the section renders a static panel of prose and a link to
the commit log, at the same reserved height. The rows are a fixed 44px with the
subject clipped rather than wrapped — a commit message is arbitrary text, and a
feed whose height depends on how verbose the last commit was cannot reserve its
space — so nothing below the section moves when data lands.

Two things worth knowing before changing it:

- **Dates are absolute (`2026-08-14`), not relative.** A relative date computed
  at build is wrong by the time anyone reads it, and one computed in the browser
  disagrees with the prerendered HTML — a hydration mismatch and a visible
  reflow on every load.
- **The build's fetch asks for `next: { revalidate }`, not `cache: "no-store"`.**
  An uncached fetch inside a page makes that page dynamic, and under
  `output: "export"` Next refuses to render it (`NEXT_STATIC_GEN_BAILOUT`) —
  which `fetchCommits` catches, so the only symptom is a feed that is silently
  always empty. That happened once. The page now logs a build-time warning when
  the list comes back empty, which is the tripwire for it happening again.

There is no releases panel and no star count, because `bonzupii/cinnabar` has no
releases, no tags and no stars. A panel that can only ever render "0" is worse
than no panel.

## Two deliberate departures from the board

Both are recorded in `src/app/globals.css` and pinned by
`tests/unit/palette.test.ts`.

**There is a light theme.** Plate 05 says "the screen system stays dark", so this
is a departure, made at the language owner's direction. It is not invented: it
uses plate 05's own light-surface set (`#F2EEEA`, `#E4DED8`, `#C4351D`,
`#16130F`), which the board scopes to print, extended with the greys needed to
carry text. Dark remains the base; light arrives via the system preference or an
explicit choice, and an explicit choice wins in both directions. **Code surfaces
keep a dark ground in both themes** — plate 09's theme is called "Cinnabar Dark"
and plate 14 forbids adding colours to it, so there is no light variant to
invent, and the syntax palette's measured contrast ratios hold whichever theme
the page is in.

**Three greys are lifted for text.** The board sets 10–11px labels in `#6E6763`
and `#7C7570`, which measure 3.47:1 and 4.25:1 on the ground — under WCAG AA's
4.5:1. `--label` and `--cinnabar-text` carry text; the brand tokens themselves
are unchanged and still describe rules, fills and the mark. The syntax theme's
keyword accent stays at the board's exact `#E0442A`, which passes at 4.61:1 on
the code ground.

## Canonical Tailwind classes, enforced

The palette is registered in an `@theme inline` block, so `bg-hairline-strong`
emits exactly what `bg-[color:var(--hairline-strong)]` emits. The long form kept
coming back — reported, fixed, and reintroduced by the next rewrite of the
component — so it is now a build error rather than an editor hint. Two things
enforce it, and they do not overlap:

- **`eslint-plugin-better-tailwindcss`**, rule `enforce-canonical-classes`. It
  implements Tailwind's own canonical suggestions, which also catch an arbitrary
  value equal to a scale value (`tracking-[-0.025em]` for `tracking-tight`) and
  a negative offset written longhand. `collapse` is off: rewriting four sides
  into a shorthand is a different judgement and would churn classes that are
  written per side on purpose.
- **`tests/unit/tailwind-tokens.test.ts`**, for the two things the plugin does
  not do. It does not resolve a token that aliases another custom property — it
  rewrites `bg-[color:var(--hairline-strong)]` to `bg-(--hairline-strong)`,
  which is shorter but still goes around the token — and it only sees classes
  inside a `className`, while this project keeps some class strings in named
  constants. The test parses the token table out of `globals.css` rather than
  listing it, so a token added to the theme is covered without the test
  changing.

Use registered tokens. `bg-[color:var(--hairline-strong)]` and
`bg-(--hairline-strong)` both fail.

## Components worth knowing about

**A `.rule-grid` must be covered by its children.** The board's signature layout
device paints `--hairline` as the *container's* background and separates its
children by a 1px gap, so every rule a reader sees is the container showing
through. That works only while the children cover it: anywhere they do not
reach is not a 1px rule but a block of flat grey. It has now been got wrong
three times, from three different causes:

- A window stretched to the height of a taller column, whose bar and body only
  add up to their own content. `WindowBody` is `flex-1`, which is right there —
  a terminal's ground reaching the frame edge is what a terminal looks like.
- The arena card stack on `/architecture/`, stretched the same way. The fix is
  the opposite one — `self-start`, so it is not stretched at all — because a
  card's height is its own content's, and growing these three would tie how
  tall a card is to how long the prose in the *next column* happens to run.
- The home page's badge strip. `w-fit` is `fit-content`, which clamps to the
  space available, so on a phone the strip is as wide as the section while its
  badges have wrapped onto a second line and left its tail bare. The badges
  `grow`, and flex distributes free space per line, so a short line is filled
  by the badges on it and a full line is unchanged.

A `.rule-grid` placed in a grid or flex row is the thing to look at.
`tests/e2e/rule-grid.spec.ts` measures every one of them on every route, at
desktop and at 390px, and fails when a container's inner edge sits more than a
pixel or so outside its children.

**`Disclosure` is a native `<details>`,** not a button and a piece of state. The
content is in the DOM whether the section is open or not, so a crawler and a
reader without JavaScript get the whole document; the open/closed state, the
Enter/Space handling and the expanded/collapsed announcement are the browser's.
It is also what makes the roadmap's in-page anchors work: the rendered
`ROADMAP.md` sits inside a closed disclosure, and browsers expand a closed
`<details>` when a fragment resolves into it, so a capability card's `#milestone-…`
link still lands. `tests/e2e/disclosure.spec.ts` renders the real component in a
subprocess and loads its markup into a browser, because Playwright compiles JSX
in a spec with its own component-testing factory, which `react-dom/server`
cannot render.

**`AsciiDiagram` recognises a figure by its characters, never by its text.**
`ARCHITECTURE.md` draws the compiler pipeline as an untagged fenced block of
box-drawing characters; rendered as a terminal window it read as program output,
which is exactly what it is not. `isAsciiDiagram` returns true when the block
contains a character from the box-drawing, arrow or geometric-arrowhead ranges —
none of which occurs in prose, in a shell transcript or in Cinnabar source — and
`parseFlow` recovers the labels of a single-column top-to-bottom flow so it can
be redrawn properly. Both degrade rather than break: if the block grows a
branch, a legend or a second column, `parseFlow` declines and the art is
presented as written; if it stops being a diagram, `isAsciiDiagram` declines and
it is an ordinary fenced block again. No path renders nothing, and no path
depends on the current wording upstream.

**The diagnostic's rails are drawn, not typed.** This was got wrong three times,
so it is recorded here as well as in `src/components/DiagnosticTranscript.tsx`:
IBM Plex Mono has no box-drawing glyphs. Every `│ ─ ┬ ╭` is served by a fallback
face at *that* face's advance rather than the mono cell — measured on the built
page at 16px, the mono cell is 9.60px, `│ ─ ┬` arrive at 11.34px and `╭ ╰ ╯` at
16px. So the rail breaks at every corner and every column after a box character
falls out of step with the source line above it. It was never line height and
never stem geometry, and no font-feature setting fixes a glyph the font does not
have. Each box character is now laid out as an empty cell exactly one `ch` wide
with the stroke painted as a CSS border inside it. The strokes are `aria-hidden`,
which is also an improvement: a screen reader announcing "box drawings light
vertical" fifteen times is noise.

**Entrance reveals must not hide content.** A scroll reveal needs a hidden state,
and motion's `initial` prop renders that into the HTML — so without JavaScript
every revealed section would be permanently invisible. The hidden state is
therefore CSS gated on a `js` class that the pre-paint script adds, and `Reveal`
only decides *when* to reveal (`useInView` from `motion`). That also means no
flash on load, because the class is in place before the first paint.
`tests/e2e/motion.spec.ts` asserts the served HTML contains no `opacity: 0`, and
everything reverts under `prefers-reduced-motion`.

**The theme key lives in a plain module.** `src/lib/theme.ts` is imported by both
the pre-paint script and the toggle. It was briefly exported from the toggle
itself, which carries `"use client"` — importing a value out of a client module
from server code yields a client reference rather than the string, so the
generated script read `localStorage.getItem(undefined)` and silently never
restored a theme.

## Social images

Next has an `opengraph-image.tsx` file convention, but it requires that exact
filename in every segment, emits it at a path with no extension, and appends a
content hash to it inside a route group. This site serves the image from a route
handler at `src/app/<route>/og-image/route.tsx` instead: the handler is written
once in `src/lib/og-image.tsx` and reused, and the URL — `/og-image`,
`/roadmap/og-image` — is one this code chooses.

The copy each image renders is exported as `og` from that route's `page.tsx`,
beside the page's other metadata, and the page points at the image with
`ogImageMetadata`. A route's title and description live in one place.

A route handler's output still has no file extension, so a static host has
nothing to infer a type from and serves it as a generic download, which every
social scraper rejects. `netlify.toml` therefore sets
`Content-Type: image/png` on `/og-image` and `/*/og-image`.
`tests/unit/netlify-config.test.ts` asserts those rules exist, since the local
test server has no `netlify.toml` to read, and `tests/e2e/metadata.spec.ts`
asserts the built images are valid PNGs.

**Fonts for those images come from npm.** `src/lib/og-fonts.ts` reads the static
`.woff` faces out of `@fontsource`. It locates them by walking up `node_modules`
rather than with `require.resolve`, because the bundler owns that function:
pointed at a `.woff` it fails the build with "Unknown module type", and pointed
at a package.json it returns an internal module id rather than a path. Satori
reads ttf, otf and woff but not woff2, which is why the larger file is the right
one here.

## Brand raster assets

```bash
npm run assets       # generate, then verify
```

`scripts/generate-brand-assets.tsx` writes eight PNGs into `public/brand/` — the
mark at 512 and 1024, the wordmark at 1024×300 and a 1280×640 social banner,
each on the dark ground and the light one. The site itself never loads them:
every mark on a page is the SVG component, which is sharper and themes itself.
They exist for everything that cannot use a React component — a README, GitHub's
social card, a slide — and that audience is also why both grounds are shipped
rather than one with a guess about the reader's theme.

Nothing in the script redraws the mark: the geometry is imported from
`CinnabarMark`, the wordmark lock-up is the same `OgWordmark` the social cards
set, and the banner is `renderOgImage` at GitHub's canvas. It reads only from
`node_modules` and this repository, so it works offline and is byte-identical
run to run. `npm run assets:verify` (`scripts/verify-png.mjs`) then checks each
file is a real PNG at its declared size. `npm run assets` is a `prebuild` step,
so a normal build regenerates and re-verifies them.

## Quality gates

```bash
npm run lint         # eslint, including the React Compiler rules
npm run typecheck    # tsc --noEmit
npm test             # vitest — pure functions: highlighters, link rewriting,
                     #   palette contrast, TOC, content bindings, GitHub feed
npm run build        # the static export
npm run test:e2e     # playwright — visual, a11y (axe), navigation, motion,
                     #   hairline-grid coverage, graceful degradation of the
                     #   commit feed
npm run test:links   # linkinator over the built export
npm run test:lighthouse
```

`npm run test:e2e` serves `out/`, so run `npm run build` first.

**The visual baselines are not committed.** They are ~21 MB in total, which is
more than this repository should carry for a site, so `**/*-snapshots/` is
ignored. The cost is real and worth stating plainly: on a machine with no
baselines the visual specs write fresh ones and assert *"this rendered"* rather
than *"this did not change"*. They only catch a regression on a machine that
already has a baseline from before it. Everything a screenshot would otherwise
pin — contrast in both themes, window chrome, layout overflow, focus behaviour —
is asserted explicitly by the other specs, which do run everywhere. Regenerate
with `npx playwright test --update-snapshots`.

The visual specs block `api.github.com`, so a baseline is pinned to the list the
build prerendered rather than changing whenever someone pushes.

## Deploying

Deploys are manual. There is no build hook and no deploy-on-push; `netlify.toml`
only describes how a build would run if one were triggered.

```bash
npx netlify login
npx netlify link --name cinnabarlang
npm run deploy          # netlify deploy --prod
npm run deploy:preview  # a draft URL, without touching production
```

Netlify CLI 27 **builds by default** — there is no `--build` flag any more,
`--no-build` is the opt-out — which is why the one line of configuration below
is what makes `npm run deploy` work at all.

`netlify.toml` sets `NETLIFY_NEXT_PLUGIN_SKIP = "true"`, and it is not optional.
Netlify auto-installs `@netlify/plugin-nextjs` on any project it detects Next in.
That runtime exists to wire up a server — SSR handlers, the ISR cache, the image
CDN — and this site is `output: "export"`: there is no server, `publish` is the
static `out/` directory, and the plugin's `onPostBuild` step has no server build
to publish, so it fails the deploy with "Failed publishing static content".
Skipping it is the documented remedy for a static export.
`tests/unit/netlify-config.test.ts` pins the setting.
