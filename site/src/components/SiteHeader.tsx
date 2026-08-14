import Link from "next/link";
import Wordmark from "@/components/brand/Wordmark";
import CinnabarMark from "@/components/brand/CinnabarMark";
import { GitHubIcon } from "@/components/brand/icons";
import NavLinks from "@/components/NavLinks";
import MobileMenu from "@/components/MobileMenu";
import ThemeToggle from "@/components/ThemeToggle";
import { REPO_URL } from "@/lib/site";

/*
 * Modelled on the docs header of plate 12: mark, wordmark, uppercase nav, and
 * the version chip pinned to the right. A hairline underneath, nothing else —
 * the header carries no accent except the mark's own block and the underline
 * on the current section.
 */
export default function SiteHeader() {
  return (
    <header className="border-hairline bg-ground/95 sticky top-0 z-50 border-b backdrop-blur-sm">
      <a
        href="#main-content"
        className="bg-cinnabar text-on-cinnabar sr-only px-4 py-2 text-sm font-bold tracking-widest uppercase focus:not-sr-only focus:fixed focus:top-4 focus:left-6 focus:z-100"
      >
        Skip to content
      </a>

      <div className="mx-auto flex h-16 max-w-[1400px] items-center gap-8 px-6 sm:px-10">
        {/*
          Below 400px only the mark shows, and the mark is decorative, so the
          link would otherwise have no accessible name at all on a small phone.
        */}
        <Link
          href="/"
          aria-label="Cinnabar — home"
          className="flex items-center gap-3 focus-visible:outline-offset-4"
        >
          <CinnabarMark size={26} letter="var(--text)" />
          <Wordmark size={18} letter="var(--text)" className="hidden min-[400px]:block" />
        </Link>

        <nav aria-label="Primary" className="hidden items-center gap-8 lg:flex">
          <NavLinks />
        </nav>

        <div className="ml-auto flex items-center gap-3">
          <span className="border-hairline text-label hidden border px-3 py-1.5 font-mono text-xs md:inline-block">
            0.1.0-dev
          </span>
          <a
            href={REPO_URL}
            target="_blank"
            rel="noopener noreferrer"
            aria-label="Cinnabar on GitHub"
            className="border-hairline-strong text-text hover:border-text hover:bg-panel panel-hover pressable hidden h-9 items-center gap-2 border px-3 text-xs font-bold tracking-widest uppercase sm:inline-flex"
          >
            <GitHubIcon size={14} />
            GitHub
          </a>
          <ThemeToggle />
          <MobileMenu />
        </div>
      </div>
    </header>
  );
}
