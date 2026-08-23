import { createNextOg } from "metaplate/next";
import {
  BRAND_THEMES,
  OG_SIZE,
  WORDMARK_METRICS,
  type BrandPalette,
  type BrandTheme,
} from "@/lib/constants";
import { loadOgFonts } from "@/lib/og-fonts";
import {
  MARK_BLOCK,
  MARK_LETTER_POINTS,
  MARK_VIEWBOX_INLINE,
} from "@/components/brand/CinnabarMark";

/*
 * Social images, built to plate 11's mark-left banner: the wordmark lock-up
 * top-left, the metadata strip top-right, the title and description in the
 * lower half, and the quip against the repo URL at the foot.
 *
 * The faces come from @fontsource at build time; see lib/og-fonts.ts for why
 * they are not vendored here.
 *
 * The palette and the canvas are parameters rather than constants because
 * scripts/generate-brand-assets.tsx renders the same banner as a downloadable
 * PNG, at GitHub's social size and on both grounds. Everything else about the
 * layout is shared, which is the point — there is one banner design, not two
 * that have to be kept in step.
 */

export const ogSize = OG_SIZE;

export type CanvasSize = { width: number; height: number };

export type OgRenderCopy = {
  eyebrow: string;
  title: string;
  description: string;
  alt?: string;
  theme?: BrandTheme;
  size?: CanvasSize;
};

/**
 * The wordmark: the drawn C followed by INNABAR.
 *
 * Plate 03's inline metrics are in em; here they are resolved against the cap
 * size so Satori — which has no em-relative sizing for an inline SVG — places
 * the letter on the same baseline the type sits on.
 */
export function OgWordmark({
  cap,
  palette,
}: {
  cap: number;
  palette: BrandPalette;
}) {
  const height = cap * WORDMARK_METRICS.capHeight;
  const width = cap * WORDMARK_METRICS.width;
  return (
    <div style={{ display: "flex", alignItems: "center" }}>
      <svg width={width} height={height} viewBox={MARK_VIEWBOX_INLINE}>
        <polygon points={MARK_LETTER_POINTS} fill={palette.text} />
        <rect {...MARK_BLOCK} fill={palette.cinnabar} />
      </svg>
      <span
        style={{
          fontFamily: "Schibsted Grotesk",
          fontWeight: 800,
          fontSize: cap,
          lineHeight: 1,
          letterSpacing: cap * -0.035,
          color: palette.text,
          marginLeft: cap * WORDMARK_METRICS.sidebearing,
        }}
      >
        INNABAR
      </span>
    </div>
  );
}

function OgTemplate({
  eyebrow,
  title,
  description,
  theme = "dark",
  size = OG_SIZE,
}: OgRenderCopy) {
  const palette = BRAND_THEMES[theme];
  const {
    ground: GROUND,
    text: TEXT,
    secondary: SECONDARY,
    mute: MUTE,
    hairline: HAIRLINE,
    cinnabar: CINNABAR,
  } = palette;

  return (
      <div
        style={{
          width: size.width,
          height: size.height,
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          background: GROUND,
          padding: "58px 72px",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <OgWordmark cap={52} palette={palette} />
          <span
            style={{
              fontFamily: "IBM Plex Mono",
              fontWeight: 500,
              fontSize: 15,
              letterSpacing: 2.6,
              textTransform: "uppercase",
              color: MUTE,
            }}
          >
            Apache-2.0 · LLVM 21 · musl
          </span>
        </div>

        <div style={{ display: "flex", flexDirection: "column" }}>
          <span
            style={{
              fontFamily: "IBM Plex Mono",
              fontWeight: 500,
              fontSize: 16,
              letterSpacing: 2.4,
              textTransform: "uppercase",
              color: CINNABAR,
              marginBottom: 22,
            }}
          >
            {eyebrow}
          </span>
          <span
            style={{
              display: "flex",
              fontFamily: "Schibsted Grotesk",
              fontWeight: 800,
              fontSize: 64,
              lineHeight: 1.04,
              letterSpacing: -2.2,
              color: TEXT,
              maxWidth: 980,
            }}
          >
            {title}
          </span>
          <span
            style={{
              display: "flex",
              fontFamily: "Schibsted Grotesk",
              fontWeight: 400,
              fontSize: 24,
              lineHeight: 1.42,
              letterSpacing: -0.3,
              color: SECONDARY,
              marginTop: 22,
              maxWidth: 900,
            }}
          >
            {description}
          </span>
        </div>

        <div style={{ display: "flex", flexDirection: "column" }}>
          <div
            style={{ display: "flex", height: 1, background: HAIRLINE, marginBottom: 20 }}
          />
          <div style={{ display: "flex", justifyContent: "space-between" }}>
            <span
              style={{
                fontFamily: "IBM Plex Mono",
                fontWeight: 500,
                fontSize: 17,
                color: CINNABAR,
              }}
            >
              Probably better than Rust.
            </span>
            <span
              style={{
                fontFamily: "IBM Plex Mono",
                fontWeight: 500,
                fontSize: 15,
                color: MUTE,
              }}
            >
              github.com/bonzupii/cinnabar
            </span>
          </div>
        </div>
      </div>
  );
}

export const og = createNextOg<OgRenderCopy>({
  alt: (copy) => copy.alt ?? copy.title,
  fonts: loadOgFonts,
  component: OgTemplate,
});

/** Renders route cards and custom-sized downloadable brand banners alike. */
export function renderOgImage(copy: OgRenderCopy) {
  const size = copy.size ?? OG_SIZE;
  return og.render(copy, { size });
}
