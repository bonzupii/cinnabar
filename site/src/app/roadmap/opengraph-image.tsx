import { ogContentType, ogSize, renderOgImage } from "@/lib/og-template";

export const dynamic = "force-static";
export const size = ogSize;
export const contentType = ogContentType;
export const alt = "Cinnabar social card — the project status.";

export default function OpengraphImage() {
  return renderOgImage({
    eyebrow: "Project status",
    title: "Resolved, and planned.",
    description:
      "Eight milestones and their status, and why self-hosting is a completeness test rather than a gate.",
  });
}
