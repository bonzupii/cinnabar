import { ImageResponse } from "next/og";
import { readFile } from "node:fs/promises";
import path from "node:path";
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
 * Schibsted Grotesk ships only as a variable font upstream, and Satori
 * resolves a variable font to its default instance — which would set the
 * wordmark at 400 instead of 800. The static instances beside this file were
 * cut from it with fontTools for that reason.
 */

const GROUND = "#100E0D";
const TEXT = "#EDE9E6";
const SECONDARY = "#A29B96";
const MUTE = "#6E6763";
const HAIRLINE = "#302C2A";
const CINNABAR = "#E0442A";

export const ogSize = { width: 1200, height: 630 };
export const ogContentType = "image/png";

async function loadFonts() {
  const dir = path.join(process.cwd(), "src", "app", "fonts");
  const [grotesk800, grotesk700, grotesk400, mono500] = await Promise.all([
    readFile(path.join(dir, "SchibstedGrotesk-800.ttf")),
    readFile(path.join(dir, "SchibstedGrotesk-700.ttf")),
    readFile(path.join(dir, "SchibstedGrotesk-400.ttf")),
    readFile(path.join(dir, "IBMPlexMono-Medium.ttf")),
  ]);
  return [
    { name: "Schibsted Grotesk", data: grotesk800, weight: 800 as const, style: "normal" as const },
    { name: "Schibsted Grotesk", data: grotesk700, weight: 700 as const, style: "normal" as const },
    { name: "Schibsted Grotesk", data: grotesk400, weight: 400 as const, style: "normal" as const },
    { name: "IBM Plex Mono", data: mono500, weight: 500 as const, style: "normal" as const },
  ];
}

/**
 * The wordmark: the drawn C followed by INNABAR.
 *
 * Plate 03's inline metrics are in em; here they are resolved against the cap
 * size so Satori — which has no em-relative sizing for an inline SVG — places
 * the letter on the same baseline the type sits on.
 */
function Wordmark({ cap }: { cap: number }) {
  const height = cap * 0.705;
  const width = cap * 0.6698;
  return (
    <div style={{ display: "flex", alignItems: "center" }}>
      <svg width={width} height={height} viewBox={MARK_VIEWBOX_INLINE}>
        <polygon points={MARK_LETTER_POINTS} fill={TEXT} />
        <rect {...MARK_BLOCK} fill={CINNABAR} />
      </svg>
      <span
        style={{
          fontFamily: "Schibsted Grotesk",
          fontWeight: 800,
          fontSize: cap,
          lineHeight: 1,
          letterSpacing: cap * -0.035,
          color: TEXT,
          marginLeft: cap * 0.031,
        }}
      >
        INNABAR
      </span>
    </div>
  );
}

export async function renderOgImage({
  eyebrow,
  title,
  description,
}: {
  eyebrow: string;
  title: string;
  description: string;
}) {
  const fonts = await loadFonts();

  return new ImageResponse(
    (
      <div
        style={{
          width: ogSize.width,
          height: ogSize.height,
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
          <Wordmark cap={52} />
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
    ),
    { ...ogSize, fonts },
  );
}
