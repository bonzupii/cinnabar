import { ogContentType, ogSize, renderOgImage } from "@/lib/og-template";

export const dynamic = "force-static";
export const size = ogSize;
export const contentType = ogContentType;
export const alt = "Cinnabar — a statically-typed systems language with Austral-style linear typing.";

export default function OpengraphImage() {
  return renderOgImage({
    eyebrow: "Systems language",
    title: "Consumed exactly once.",
    description:
      "A statically-typed systems language with Austral-style linear typing. No garbage collector, no lifetime annotations, no reachable panics.",
  });
}
