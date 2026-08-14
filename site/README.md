# Cinnabar site

The public site for [Cinnabar](https://github.com/bonzupii/cinnabar), built with
Next.js 16 and exported as static files.

The design is not invented here. Every colour, typeface, mark and layout device
comes from the brand board in
[`.planning/brand/designs/Cinnabar Brand.dc.html`](.planning/brand/designs/), and
the source comments cite the plate each decision comes from.

## Editing content

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

The home page lives in a `(home)` route group so it has a folder of its own
like every other route; a route group does not appear in the URL.

Structured content that is rendered as components rather than prose — the CLI
tables, the language highlights, the milestone list, the code samples — lives in
`src/content/*.ts`, because a typed array is a better shape for a table than
markdown is.

`/manifesto`, `/roadmap` and `/architecture` additionally render `MANIFESTO.md`,
`ROADMAP.md` and `ARCHITECTURE.md` from the repository root, read at build time
by `src/lib/repo-docs.ts`. They are not copied here — `MANIFESTO.md` is
normative, and a stale copy of a normative spec is worse than no copy.
Repo-relative links inside those documents are rewritten: a document with its
own page here links to that page, everything else links into GitHub.

## Two deliberate departures from the board

Both are recorded in `src/app/globals.css` and pinned by
`tests/unit/palette.test.ts`.

**There is a light theme.** Plate 05 says "the screen system stays dark", so
this is a departure, made at the language owner's direction. It is not invented:
it uses plate 05's own light-surface set (`#F2EEEA`, `#E4DED8`, `#C4351D`,
`#16130F`), which the board scopes to print, extended with the greys needed to
carry text. Dark remains the base; light arrives via the system preference or an
explicit choice. Code blocks stay dark in both themes — plate 09's theme is
called "Cinnabar Dark" and plate 14 forbids adding colours to it, so there is no
light variant to invent.

**Three greys are lifted for text.** The board sets 10–11px labels in `#6E6763`
and `#7C7570`, which measure 3.47:1 and 4.25:1 on the ground — under WCAG AA's
4.5:1. `--label` and `--cinnabar-text` carry text; the brand tokens themselves
are unchanged and still describe rules, fills and the mark. The syntax theme's
keyword accent stays at the board's exact `#E0442A`, which passes at 4.61:1 on
the code ground.

## Commands

```bash
npm install
npm run dev          # http://localhost:3000
npm run build        # static export into out/
```

Quality gates:

```bash
npm run lint         # eslint, including the React Compiler rules
npm run typecheck    # tsc --noEmit
npm test             # vitest — highlighters, link rewriting, palette contrast, TOC
npm run test:e2e     # playwright — visual (both themes), a11y (axe), navigation, motion
npm run test:links   # linkinator over the built export
npm run test:lighthouse
```

`npm run test:e2e` serves `out/`, so run `npm run build` first. Refresh visual
baselines with `npm run test:e2e:update` and read the diff before committing it.

## Deploying

Deploys are manual. There is no build hook and no deploy-on-push.

```bash
npx netlify login
npx netlify link --name cinnabarlang
npm run deploy
```

`npm run deploy` runs `netlify deploy --build --prod`; `npm run deploy:preview`
gives a draft URL without touching production.

## Notes on four non-obvious choices

**Social images need a content-type header.** Next's `opengraph-image`
convention emits an extension-less file (`out/opengraph-image`), which a static
host serves as a generic download — and every social scraper then rejects it.
`netlify.toml` restores `image/png` for those paths; `tests/unit/netlify-config.test.ts`
asserts the rules exist, since the local test server has no `netlify.toml` to
read, and `scripts/verify-png.mjs` checks the built files are real PNGs.

The copy each image renders is exported as `og` from that route's `page.tsx`,
beside the page's other metadata, and imported by `opengraph-image.tsx` — Next
requires the image to be its own file, but a route's title and description
should not live in two places. The home page's image is at the root segment
rather than inside `(home)`: Next appends a content hash to a metadata image
declared inside a route group, which would break the stable path the header
rule targets.

**Fonts for those images come from npm.** `src/lib/og-fonts.ts` reads the
static `.woff` faces out of `@fontsource`. It locates them by walking up
`node_modules` rather than with `require.resolve`, because the bundler owns
that function: pointed at a `.woff` it fails the build with "Unknown module
type", and pointed at a package.json it returns an internal module id rather
than a path. Satori reads ttf, otf and woff but not woff2, which is why the
larger file is the right one here.

**Entrance reveals must not hide content.** A scroll reveal needs a hidden state,
and motion's `initial` prop renders that into the HTML — so without JavaScript
every revealed section would be permanently invisible. The hidden state is
therefore CSS gated on a `js` class that the pre-paint script adds, and
`Reveal` only decides *when* to reveal (`useInView` from `motion`). That also
means no flash on load, because the class is in place before the first paint.
`tests/e2e/motion.spec.ts` asserts the served HTML contains no `opacity: 0`.

**The theme key lives in a plain module.** `src/lib/theme.ts` is imported by both
the pre-paint script and the toggle. It was briefly exported from the toggle
itself, which carries `"use client"` — importing a value out of a client module
from server code yields a client reference rather than the string, so the
generated script read `localStorage.getItem(undefined)` and silently never
restored a theme.
