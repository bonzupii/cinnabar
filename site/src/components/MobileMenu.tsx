"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { GitHubIcon, NAV_ICONS } from "@/components/brand/icons";
import { isActiveRoute, NAV, REPO_URL } from "@/lib/site";

/*
 * A modal navigation panel for narrow viewports.
 *
 * Focus is moved into the dialog on open and returned to the trigger on close;
 * Tab is wrapped inside the panel while it is open, so the page behind it is
 * never reachable by keyboard without closing first.
 *
 * The overlay is portalled to <body>. It cannot render in place: this
 * component lives inside the site header, and the header carries a
 * backdrop-filter, which makes it a containing block for `position: fixed`
 * descendants. Rendered in place the overlay resolves `inset-0` against the
 * 64px header instead of the viewport and collapses to zero height.
 */
export default function MobileMenu() {
  const [open, setOpen] = useState(false);
  const pathname = usePathname();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  // The panel is dismissed where it is acted on, rather than by reacting to
  // the route afterwards: following a link is what closes it, so the close
  // belongs on the link.
  const close = () => setOpen(false);

  useEffect(() => {
    if (!open) return;

    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";

    const panel = panelRef.current;
    panel?.querySelector<HTMLElement>("a, button")?.focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close();
        triggerRef.current?.focus();
        return;
      }
      if (event.key !== "Tab" || !panel) return;

      const focusable = panel.querySelectorAll<HTMLElement>("a[href], button");
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];

      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.body.style.overflow = previousOverflow;
    };
  }, [open]);

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        aria-label={open ? "Close menu" : "Open menu"}
        className="border-hairline-strong text-text hover:border-text hover:bg-panel panel-hover pressable inline-flex h-9 w-9 items-center justify-center border lg:hidden"
      >
        <svg
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          aria-hidden="true"
        >
          {open ? (
            <>
              <line x1="5" y1="5" x2="19" y2="19" />
              <line x1="19" y1="5" x2="5" y2="19" />
            </>
          ) : (
            <>
              <line x1="3" y1="7" x2="21" y2="7" />
              <line x1="3" y1="12" x2="21" y2="12" />
              <line x1="3" y1="17" x2="21" y2="17" />
            </>
          )}
        </svg>
      </button>

      {open
        ? createPortal(
            <div className="fixed inset-x-0 top-16 bottom-0 z-40 lg:hidden">
              <div
                aria-hidden="true"
                data-testid="mobile-menu-backdrop"
                onClick={close}
                className="bg-ground/80 absolute inset-0"
              />
              <div
                ref={panelRef}
                role="dialog"
                aria-modal="true"
                aria-label="Site navigation"
                className="border-hairline bg-panel relative max-h-full overflow-y-auto border-b"
              >
                <nav aria-label="Primary mobile" className="flex flex-col">
                  {NAV.map((item) => {
                    const active = isActiveRoute(pathname, item.href);
                    const Icon = NAV_ICONS[item.icon];
                    return (
                      <Link
                        key={item.href}
                        href={item.href}
                        aria-current={active ? "page" : undefined}
                        onClick={close}
                        className="border-hairline hover:bg-panel-raised panel-hover border-b px-6 py-4"
                      >
                        <span
                          className={`flex items-center gap-2.5 text-sm font-bold tracking-[0.1em] uppercase ${
                            active ? "text-cinnabar-text" : "text-text"
                          }`}
                        >
                          <Icon size={16} />
                          {item.label}
                        </span>
                        <span className="text-label mt-1 block pl-[26px] font-mono text-xs">
                          {item.blurb}
                        </span>
                      </Link>
                    );
                  })}
                  <a
                    href={REPO_URL}
                    target="_blank"
                    rel="noopener noreferrer"
                    onClick={close}
                    className="text-text hover:bg-panel-raised panel-hover flex items-center gap-2 px-6 py-4 text-sm font-bold tracking-[0.1em] uppercase"
                  >
                    <GitHubIcon size={14} />
                    GitHub
                  </a>
                </nav>
              </div>
            </div>,
            document.body,
          )
        : null}
    </>
  );
}
