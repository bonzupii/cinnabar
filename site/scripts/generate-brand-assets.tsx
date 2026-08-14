import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { ImageResponse } from "next/og";
import {
  MARK_BLOCK,
  MARK_LETTER_POINTS,
  MARK_VIEWBOX_STANDALONE,
} from "@/components/brand/CinnabarMark";
import {
  BRAND_THEMES,
  type BrandPalette,
  type BrandTheme,
} from "@/lib/constants";
import { loadOgFonts } from "@/lib/og-fonts";
import { OgWordmark, renderOgImage } from "@/lib/og-template";
import { DESCRIPTION } from "@/lib/site";

/*
 * Raster brand assets.
 *
 * The site itself never loads these: every mark on a page is the SVG
 * component, which is sharper and themes itself. They exist for everything
 * outside the site that cannot use an SVG React component — a README, GitHub's
 * social card, a slide, someone writing about the language. That audience also
 * explains the two grounds: a README is read in whichever theme its reader
 * chose, so both are shipped rather than one with a guess.
 *
 * Nothing here redraws the mark. The geometry is imported from the component
 * that owns it, the wordmark lock-up is the same `OgWordmark` the social cards
 * set, and the banner is `renderOgImage` at GitHub's canvas — so a change to
 * the brand reaches the PNGs by rebuilding rather than by being reapplied.
 *
 * Run with `npm run assets`. It reads only from node_modules and from this
 * repository, so it works offline and produces the same bytes every time.
 */

const OUTPUT_DIR = path.join(process.cwd(), "public", "brand");

/**
 * The mark's share of its canvas.
 *
 * Plate 02 keeps a block's width of clear space around the mark; the block is
 * 0.34 cap, so a square canvas that leaves it on all four sides puts the mark
 * at a little under two thirds of the width.
 */
const MARK_SCALE = 0.62;

/** The banner's copy. The description is plate 11's, from lib/site.ts. */
const BANNER = {
  eyebrow: "Systems language",
  title: "A zero-trust systems language.",
  description: DESCRIPTION,
};

/** GitHub's social preview canvas. */
const BANNER_SIZE = { width: 1280, height: 640 };

/** The wordmark lock-up's canvas, and the cap size that centres in it. */
const WORDMARK_CANVAS = { width: 1024, height: 300 };
const WORDMARK_CAP = 148;

type Asset = { name: string; render: () => Promise<ImageResponse> };

/** The mark alone, centred on its ground. */
function markAsset(size: number, palette: BrandPalette) {
  return async () => {
    const inner = Math.round(size * MARK_SCALE);
    return new ImageResponse(
      (
        <div
          style={{
            width: size,
            height: size,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: palette.ground,
          }}
        >
          <svg width={inner} height={inner} viewBox={MARK_VIEWBOX_STANDALONE}>
            <polygon points={MARK_LETTER_POINTS} fill={palette.text} />
            <rect {...MARK_BLOCK} fill={palette.cinnabar} />
          </svg>
        </div>
      ),
      { width: size, height: size, fonts: await loadOgFonts() },
    );
  };
}

/** The full lock-up: the drawn C and INNABAR, centred on its ground. */
function wordmarkAsset(palette: BrandPalette) {
  return async () =>
    new ImageResponse(
      (
        <div
          style={{
            ...WORDMARK_CANVAS,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: palette.ground,
          }}
        >
          <OgWordmark cap={WORDMARK_CAP} palette={palette} />
        </div>
      ),
      { ...WORDMARK_CANVAS, fonts: await loadOgFonts() },
    );
}

/** Plate 11's banner, at the size GitHub crops its social preview to. */
function bannerAsset(theme: BrandTheme) {
  return async () => renderOgImage({ ...BANNER, theme, size: BANNER_SIZE });
}

/** Every file this script produces, in both themes. */
function assets(): Asset[] {
  const list: Asset[] = [];
  for (const theme of ["dark", "light"] as const) {
    const palette = BRAND_THEMES[theme];
    for (const size of [512, 1024]) {
      list.push({
        name: `cinnabar-mark-${size}-${theme}.png`,
        render: markAsset(size, palette),
      });
    }
    list.push({
      name: `cinnabar-wordmark-${WORDMARK_CANVAS.width}-${theme}.png`,
      render: wordmarkAsset(palette),
    });
    list.push({
      name: `cinnabar-banner-${BANNER_SIZE.width}x${BANNER_SIZE.height}-${theme}.png`,
      render: bannerAsset(theme),
    });
  }
  return list;
}

async function main() {
  await mkdir(OUTPUT_DIR, { recursive: true });

  for (const asset of assets()) {
    const response = await asset.render();
    const bytes = Buffer.from(await response.arrayBuffer());
    const file = path.join(OUTPUT_DIR, asset.name);
    await writeFile(file, bytes);
    console.log(`wrote public/brand/${asset.name} — ${bytes.length} bytes`);
  }
}

// Not top-level await: the package is not `type: "module"`, so this file is
// transformed to CommonJS and top-level await is not available there.
main().catch((error) => {
  console.error(error);
  process.exit(1);
});
