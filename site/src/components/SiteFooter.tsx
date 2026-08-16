import Link from "next/link";
import Wordmark from "@/components/brand/Wordmark";
import {
  DiagnosticIcon,
  DocIcon,
  GitHubIcon,
  NAV_ICONS,
} from "@/components/brand/icons";
import { NAV, QUIP, REPO_URL } from "@/lib/site";
import { CONTAINER, ICON } from "@/lib/constants";

/*
 * The closing strip of plate 14: the wordmark set in one colour against a
 * metadata line. Nothing here takes the accent except the marks inside the
 * icons, which carry it by construction.
 *
 * Every link is marked with the same icon the section uses elsewhere, so the
 * footer reads as a map of the site rather than as a list of words.
 */

const LINK =
  "text-secondary hover:text-text panel-hover flex items-center gap-2.5 text-sm";

const HEADING =
  "text-label mb-1 font-mono text-[10px] tracking-[0.16em] uppercase";

export default function SiteFooter() {
  return (
    <footer className="border-hairline border-t">
      <div className={`${CONTAINER} py-14`}>
        <div className="flex flex-col gap-12 lg:flex-row lg:justify-between">
          <div className="flex flex-col gap-5">
            <Wordmark size={28} variant="mono" letter="var(--label)" />
            <p className="text-secondary max-w-[34ch] font-mono text-[13px] leading-relaxed">
              {QUIP}
            </p>
            <p className="text-label font-mono text-[11px] tracking-[0.08em]">
              LLVM 21 · musl · Apache-2.0 WITH LLVM-exception
            </p>
          </div>

          <div className="flex gap-16">
            <nav aria-label="Footer" className="flex flex-col gap-3">
              <h2 className={HEADING}>Documentation</h2>
              {NAV.map((item) => {
                const Icon = NAV_ICONS[item.icon];
                return (
                  <Link key={item.href} href={item.href} className={LINK}>
                    <Icon size={ICON.inline} />
                    {item.label}
                  </Link>
                );
              })}
              <Link href="/manifesto/" className={LINK}>
                <DocIcon size={ICON.inline} />
                Manifesto
              </Link>
              <Link href="/contributing/development/" className={LINK}>
                <DiagnosticIcon size={ICON.inline} />
                Contributing
              </Link>
            </nav>

            <div className="flex flex-col gap-3">
              <h2 className={HEADING}>Project</h2>
              <a
                href={REPO_URL}
                target="_blank"
                rel="noopener noreferrer"
                className={LINK}
              >
                <GitHubIcon size={ICON.inline} />
                Repository
              </a>
              <a
                href={`${REPO_URL}/issues`}
                target="_blank"
                rel="noopener noreferrer"
                className={LINK}
              >
                <DiagnosticIcon size={ICON.inline} />
                Issues
              </a>
              <a
                href={`${REPO_URL}/blob/main/LICENSE`}
                target="_blank"
                rel="noopener noreferrer"
                className={LINK}
              >
                <DocIcon size={ICON.inline} />
                License
              </a>
            </div>
          </div>
        </div>

        <div className="border-hairline mt-14 flex flex-col gap-3 border-t pt-8 sm:flex-row sm:items-center sm:justify-between">
          <span className="text-label font-mono text-[11px] tracking-[0.16em] uppercase">
            Cinnabar · early development
          </span>
          <span className="text-label font-mono text-[11px]">
            github.com/bonzupii/cinnabar
          </span>
        </div>
      </div>
    </footer>
  );
}
