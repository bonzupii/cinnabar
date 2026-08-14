import { ogContentType, ogSize, renderOgImage } from "@/lib/og-template";

export const dynamic = "force-static";
export const size = ogSize;
export const contentType = ogContentType;
export const alt = "Cinnabar social card — the getting-started guide.";

export default function OpengraphImage() {
  return renderOgImage({
    eyebrow: "Getting started",
    title: "Build the compiler.",
    description:
      "LLVM 21 via a Nix flake, a static musl libc, the language server, and the repository's verification gate.",
  });
}
