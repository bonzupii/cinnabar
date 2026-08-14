import type { ComponentType } from "react";

/*
 * The section rule the board uses at the top of each plate: an uppercase name
 * over a hairline, with an optional mono note pushed to the right.
 *
 * The board numbers its plates because it is a board — a single continuous
 * document a reader scrolls through in order. The site is not, so the numbers
 * are dropped and an icon from the plate 07 set carries the section instead.
 */
export default function SectionHeading({
  title,
  note,
  icon: Icon,
  id,
  as: Tag = "h2",
}: {
  title: string;
  note?: string;
  icon?: ComponentType<{ size?: number; className?: string }>;
  id?: string;
  as?: "h2" | "h3";
}) {
  return (
    <div
      id={id}
      className="border-hairline flex scroll-mt-24 items-center gap-4 border-b pb-5"
    >
      {Icon ? <Icon size={20} className="text-cinnabar-text" /> : null}
      <Tag className="text-text text-xs font-bold tracking-[0.2em] uppercase">
        {title}
      </Tag>
      {note ? (
        <span className="text-label ml-auto hidden font-mono text-[11px] sm:block">
          {note}
        </span>
      ) : null}
    </div>
  );
}
