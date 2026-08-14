import { ogContentType, ogSize, renderOgImage } from "@/lib/og-template";

export const dynamic = "force-static";
export const size = ogSize;
export const contentType = ogContentType;
export const alt = "Cinnabar social card — the normative language specification.";

export default function OpengraphImage() {
  return renderOgImage({
    eyebrow: "Normative specification",
    title: "The Cinnabar Manifesto",
    description:
      "Twelve core principles, the authoritative language surface, and the anti-principles the language will never have.",
  });
}
