import { ogContentType, ogSize, renderOgImage } from "@/lib/og-template";

export const dynamic = "force-static";
export const size = ogSize;
export const contentType = ogContentType;
export const alt = "Cinnabar social card — the CLI reference.";

export default function OpengraphImage() {
  return renderOgImage({
    eyebrow: "CLI reference",
    title: "Two ways to invoke it.",
    description:
      "Every flag for compiling a file, every project subcommand, the build.cnb manifest, and the test layout.",
  });
}
