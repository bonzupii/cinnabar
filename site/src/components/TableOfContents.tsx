"use client";

import { useEffect, useState } from "react";
import type { TocEntry } from "@/lib/markdown-toc";

/*
 * A sticky contents rail for the long documents.
 *
 * The active entry is tracked with an IntersectionObserver keyed to a band
 * across the upper part of the viewport, rather than by listening to scroll:
 * it fires only when a heading actually crosses the band, so there is no
 * per-frame work and no layout thrashing while the reader scrolls.
 */
export default function TableOfContents({
  entries,
  label = "On this page",
}: {
  entries: readonly TocEntry[];
  label?: string;
}) {
  const [activeSlug, setActiveSlug] = useState<string | null>(
    entries[0]?.slug ?? null,
  );

  useEffect(() => {
    if (entries.length === 0) return;

    const headings = entries
      .map((entry) => document.getElementById(entry.slug))
      .filter((element): element is HTMLElement => element !== null);
    if (headings.length === 0) return;

    // Track which headings are inside the band; the topmost one wins, so a
    // section stays marked while its body is being read.
    const visible = new Set<string>();

    const observer = new IntersectionObserver(
      (records) => {
        for (const record of records) {
          if (record.isIntersecting) visible.add(record.target.id);
          else visible.delete(record.target.id);
        }
        const firstVisible = headings.find((heading) => visible.has(heading.id));
        if (firstVisible) {
          setActiveSlug(firstVisible.id);
          return;
        }
        // Nothing in the band — between two headings. Keep the last one that
        // scrolled past the top instead of clearing the marker.
        const passed = headings.filter(
          (heading) => heading.getBoundingClientRect().top < 120,
        );
        if (passed.length > 0) setActiveSlug(passed[passed.length - 1].id);
      },
      { rootMargin: "-88px 0px -70% 0px", threshold: 0 },
    );

    for (const heading of headings) observer.observe(heading);
    return () => observer.disconnect();
  }, [entries]);

  if (entries.length === 0) return null;

  return (
    <nav aria-label={label} className="lg:sticky lg:top-24">
      <h2 className="text-label border-hairline mb-4 border-b pb-3 font-mono text-[10px] tracking-[0.16em] uppercase">
        {label}
      </h2>
      <ul className="flex max-h-[calc(100vh-11rem)] list-none flex-col gap-0.5 overflow-y-auto pr-2">
        {entries.map((entry) => {
          const active = entry.slug === activeSlug;
          return (
            <li key={entry.slug}>
              <a
                href={`#${entry.slug}`}
                aria-current={active ? "true" : undefined}
                className={`panel-hover block border-l-2 py-1.5 text-[13px] leading-snug ${
                  entry.depth >= 3 ? "pl-5" : "pl-3"
                } ${
                  active
                    ? "border-cinnabar text-text"
                    : // The rail is a column of quiet grey; on hover the entry
                      // takes its own rule, so the cursor picks one line out of
                      // the list rather than merely brightening it.
                      "text-label hover:text-text hover:border-hairline-strong border-transparent"
                }`}
              >
                {entry.text}
              </a>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
