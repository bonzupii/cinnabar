import type { ComponentType } from "react";
import { Lede } from "@/components/ui";

/*
 * The page header.
 *
 * The board numbers its plates, but a plate index is meaningless once each
 * section is its own page, so the number is replaced by an icon from the
 * plate 07 set and the section name.
 */
export default function PageHeader({
  section,
  note,
  title,
  lede,
  icon: Icon,
}: {
  /** Uppercase section name. */
  section: string;
  /** Optional mono note, right-aligned on the rule. */
  note?: string;
  title: string;
  /** Markdown, normally the route's `@lede` block. */
  lede?: string;
  icon?: ComponentType<{ size?: number; className?: string }>;
}) {
  return (
    <div className="mx-auto max-w-[1400px] px-6 pt-14 sm:px-10 sm:pt-20">
      <div className="border-hairline flex items-center gap-4 border-b pb-5">
        {Icon ? <Icon size={20} className="text-cinnabar-text" /> : null}
        <span className="text-text text-xs font-bold tracking-[0.2em] uppercase">
          {section}
        </span>
        {note ? (
          <span className="text-label ml-auto hidden font-mono text-[11px] sm:block">
            {note}
          </span>
        ) : null}
      </div>

      <h1 className="text-text mt-11 max-w-[22ch] text-[40px] leading-[1.05] font-bold tracking-[-0.03em] text-balance sm:text-[58px]">
        {title}
      </h1>
      {lede ? (
        <div className="mt-7 max-w-[80ch]">
          <Lede>{lede}</Lede>
        </div>
      ) : null}
    </div>
  );
}

/** Small uppercase mono label in vermilion — the board's eyebrow. */
export function Eyebrow({ children }: { children: React.ReactNode }) {
  return (
    <span className="text-cinnabar-text font-mono text-[10px] tracking-[0.16em] uppercase">
      {children}
    </span>
  );
}
