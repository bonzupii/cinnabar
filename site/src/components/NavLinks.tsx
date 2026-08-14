"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { isActiveRoute, NAV } from "@/lib/site";

/*
 * Plate 12 sets the nav in bold uppercase with 0.1em tracking, and marks the
 * current section with a 2px vermilion underline — the header's only use of
 * the accent besides the mark.
 */
export default function NavLinks() {
  const pathname = usePathname();

  return (
    <>
      {NAV.map((item) => {
        const active = isActiveRoute(pathname, item.href);
        return (
          <Link
            key={item.href}
            href={item.href}
            aria-current={active ? "page" : undefined}
            className={`panel-hover border-b-2 pb-[3px] text-[13px] font-bold tracking-[0.1em] uppercase ${
              active
                ? "border-cinnabar text-text"
                : "text-secondary hover:text-text border-transparent"
            }`}
          >
            {item.label}
          </Link>
        );
      })}
    </>
  );
}
