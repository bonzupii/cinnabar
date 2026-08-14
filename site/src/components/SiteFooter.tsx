import Link from "next/link";
import Wordmark from "@/components/brand/Wordmark";
import { GitHubIcon } from "@/components/brand/icons";
import { NAV, QUIP, REPO_URL } from "@/lib/site";

/*
 * The closing strip of plate 14: the wordmark set in the mute grey, one
 * colour, against a metadata line. Nothing here takes the accent.
 */
export default function SiteFooter() {
  return (
    <footer className="border-hairline border-t">
      <div className="mx-auto max-w-[1360px] px-6 py-14 sm:px-10">
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
              <h2 className="text-label mb-1 font-mono text-[10px] tracking-[0.16em] uppercase">
                Documentation
              </h2>
              {NAV.map((item) => (
                <Link
                  key={item.href}
                  href={item.href}
                  className="text-secondary hover:text-text panel-hover text-sm"
                >
                  {item.label}
                </Link>
              ))}
            </nav>

            <div className="flex flex-col gap-3">
              <h2 className="text-label mb-1 font-mono text-[10px] tracking-[0.16em] uppercase">
                Project
              </h2>
              <a
                href={REPO_URL}
                target="_blank"
                rel="noopener noreferrer"
                className="text-secondary hover:text-text panel-hover flex items-center gap-2 text-sm"
              >
                <GitHubIcon size={13} />
                Repository
              </a>
              <a
                href={`${REPO_URL}/issues`}
                target="_blank"
                rel="noopener noreferrer"
                className="text-secondary hover:text-text panel-hover text-sm"
              >
                Issues
              </a>
              <a
                href={`${REPO_URL}/blob/main/LICENSE`}
                target="_blank"
                rel="noopener noreferrer"
                className="text-secondary hover:text-text panel-hover text-sm"
              >
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
