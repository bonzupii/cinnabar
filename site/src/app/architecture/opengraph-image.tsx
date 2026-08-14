import { ogContentType, ogSize, renderOgImage } from "@/lib/og-template";

export const dynamic = "force-static";
export const size = ogSize;
export const contentType = ogContentType;
export const alt = "Cinnabar social card — the compiler internals.";

export default function OpengraphImage() {
  return renderOgImage({
    eyebrow: "Compiler internals",
    title: "One fixed pipeline.",
    description:
      "Seven stages over a flat node arena, where every fact is computed exactly once and attached for later stages to read.",
  });
}
