import type { Metadata } from "next";
import PageHeader from "@/components/PageHeader";
import PlaygroundEditor from "@/components/PlaygroundEditor";
import { DiagnosticIcon } from "@/components/brand/icons";
import { ogImageMetadata } from "@/lib/og-image";

/**
 * `PageHeader`'s own `lede` prop caps at 80ch, the reading width every other
 * page's opening paragraph uses. This page's intro reads better at the
 * width its own content area already commits to (the editor and its six
 * samples are wide), so it's rendered here instead, inside the same
 * `max-w-[1400px]` container as everything else on the page.
 */
const LEDE =
  "This runs the compiler's real front end — lexing through borrow checking — compiled to WebAssembly, in your browser. Nothing is linked, executed, or sent to a server: it can only tell you whether what you typed is well-formed, the same way it would tell the compiler.";

/** Social image copy, rendered by ./og-image/route.tsx. */
export const og = {
  eyebrow: "Try it",
  title: "Checked as you type.",
  description:
    "The same lexer, resolver, typechecker and borrow checker the compiler runs, compiled to WebAssembly and running in your browser. No server, no execution of what you write.",
  alt: "Cinnabar social card — the in-browser playground.",
};

export const metadata: Metadata = {
  title: "Playground",
  description:
    "Type Cinnabar and see real compiler diagnostics — lexing, resolution, type checking and borrow checking, run entirely in your browser with no server in between.",
  alternates: { canonical: "/playground/" },
  ...ogImageMetadata("/playground/", og),
};

export default function PlaygroundPage() {
  return (
    <article className="pb-28">
      <PageHeader section="Playground" title="Checked as you type." icon={DiagnosticIcon} />

      <div className="mx-auto max-w-[1400px] px-6 pt-14 sm:px-10">
        <p className="text-secondary mb-10 text-[16px] leading-[1.7] text-pretty">{LEDE}</p>
        <PlaygroundEditor />
      </div>
    </article>
  );
}
