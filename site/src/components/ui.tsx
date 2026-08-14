import Link from "next/link";
import type { ComponentType, ReactNode } from "react";
import Markdown, { InlineMarkdown } from "@/components/Markdown";

/*
 * The small, repeated pieces.
 *
 * Every one of these existed as copy-pasted class strings across the pages —
 * the same button, the same panel, the same source note — which is exactly how
 * a design system drifts. They are gathered here so the brand rules they
 * encode (uppercase 0.1em tracking on actions, hairline grids, one accent per
 * view) are stated once.
 */

/* --------------------------------------------------------------- actions -- */

type ButtonVariant = "primary" | "secondary" | "ghost";

const BUTTON_BASE =
  "panel-hover pressable inline-flex items-center gap-2.5 text-sm font-bold tracking-[0.1em] uppercase";

const BUTTON_VARIANT: Record<ButtonVariant, string> = {
  primary: "bg-cinnabar text-on-cinnabar hover:bg-cinnabar-deep px-7 py-3.5",
  // The fill on hover is what makes a bordered button feel like a surface
  // rather than an outline; the border going to full text colour alone read as
  // a hairline flicker.
  secondary:
    "border-hairline-strong text-text hover:border-text hover:bg-panel border px-7 py-3.5",
  ghost: "text-secondary hover:text-text hover:bg-panel px-2 py-3.5",
};

/**
 * What an icon component has to accept to be usable here.
 *
 * `onAccent` is part of it because the primary variant sets it: the plate 07
 * icons mark their meaningful part in vermilion, which vanishes on a vermilion
 * fill.
 */
type ActionIcon = ComponentType<{
  size?: number;
  className?: string;
  onAccent?: boolean;
}>;

type ActionProps = {
  href: string;
  children: ReactNode;
  variant?: ButtonVariant;
  icon?: ActionIcon;
  /** Set for links leaving the site; adds the usual target and rel. */
  external?: boolean;
  className?: string;
};

/** A call to action. Internal links route through next/link. */
export function Action({
  href,
  children,
  variant = "secondary",
  icon: Icon,
  external,
  className,
}: ActionProps) {
  const classes = `${BUTTON_BASE} ${BUTTON_VARIANT[variant]}${className ? ` ${className}` : ""}`;
  const content = (
    <>
      {/*
        Only the primary variant paints the accent, so only it needs the icon's
        accented detail moved off vermilion. Deciding it here rather than at
        each call site means a caller cannot forget.
      */}
      {Icon ? <Icon size={16} onAccent={variant === "primary"} /> : null}
      {children}
    </>
  );

  if (external) {
    return (
      <a href={href} target="_blank" rel="noopener noreferrer" className={classes}>
        {content}
      </a>
    );
  }
  return (
    <Link href={href} className={classes}>
      {content}
    </Link>
  );
}

/** An inline forward link, set in the accent — "Read the walkthrough →". */
export function ArrowLink({
  href,
  children,
  external,
  className,
}: {
  href: string;
  children: ReactNode;
  external?: boolean;
  className?: string;
}) {
  const classes = `text-cinnabar-text hover:text-text panel-hover group inline-flex w-fit items-center gap-1.5 text-[13px] font-bold tracking-widest uppercase${
    className ? ` ${className}` : ""
  }`;
  /*
   * The arrow advances a couple of pixels under the cursor. It is the one
   * gesture on the site that says something about direction rather than about
   * state, which is why it earns movement where the other hovers only change
   * colour. `motion-reduce:` removes the travel outright rather than shortening
   * it.
   */
  const content = (
    <>
      {children}
      <span
        aria-hidden="true"
        className="inline-block transition-transform duration-150 ease-out group-hover:translate-x-[3px] motion-reduce:transition-none motion-reduce:group-hover:translate-x-0"
      >
        →
      </span>
    </>
  );

  if (external) {
    return (
      <a href={href} target="_blank" rel="noopener noreferrer" className={classes}>
        {content}
      </a>
    );
  }
  return (
    <Link href={href} className={classes}>
      {content}
    </Link>
  );
}

/* ------------------------------------------------------------------ text -- */

/** Document-scale prose, filling its column up to a readable measure. */
export function Prose({
  children,
  className,
}: {
  children: string;
  className?: string;
}) {
  return (
    <div className={`max-w-[86ch] [&_p:first-child]:mt-0 ${className ?? ""}`}>
      <Markdown>{children}</Markdown>
    </div>
  );
}

/** A page lede: markdown that inherits a larger size from this wrapper. */
export function Lede({ children }: { children: string }) {
  return (
    <div className="text-secondary text-[18px] leading-[1.55] tracking-[-0.01em] text-pretty sm:text-[21px] [&_code]:font-mono [&_code]:text-[0.9em]">
      <InlineMarkdown>{children}</InlineMarkdown>
    </div>
  );
}

/**
 * The note that says which repository file a page is rendered from.
 *
 * Repeated on the three document pages, always in the same form: a vermilion
 * rule, mono text, and one link back to the source.
 */
export function SourceNote({
  children,
  className,
}: {
  children: string;
  className?: string;
}) {
  return (
    <div
      className={`border-cinnabar text-bright border-l-2 pl-6 font-mono text-[13px] leading-[1.8] [&_a]:text-cinnabar-text [&_a]:underline [&_a]:underline-offset-[3px] [&_a]:decoration-cinnabar-text/40 [&_a:hover]:decoration-cinnabar-text ${
        className ?? ""
      }`}
    >
      <InlineMarkdown>{children}</InlineMarkdown>
    </div>
  );
}

/** Inline code outside a markdown document. */
export function Code({ children }: { children: ReactNode }) {
  return (
    <code className="border-hairline bg-panel text-bright border px-[5px] py-[2px] font-mono text-[0.875em] wrap-break-word">
      {children}
    </code>
  );
}

/* --------------------------------------------------------------- surfaces -- */

/** A cell in one of the board's hairline grids. */
export function Panel({
  children,
  className,
  interactive,
  as: Tag = "div",
}: {
  children: ReactNode;
  className?: string;
  /** Adds the hover treatment, for cells that are links or contain one. */
  interactive?: boolean;
  as?: "div" | "li";
}) {
  return (
    <Tag
      className={`bg-panel flex flex-col ${
        interactive ? "hover:bg-panel-raised panel-interactive" : ""
      } ${className ?? ""}`}
    >
      {children}
    </Tag>
  );
}

/**
 * A bordered callout — the board's device for a statement that carries more
 * weight than the prose around it.
 */
export function Callout({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={`border-hairline bg-panel flex flex-col gap-5 border p-8 sm:p-12 ${
        className ?? ""
      }`}
    >
      {children}
    </div>
  );
}

/** A count with its label, as used by the roadmap's status summary. */
export function Stat({ value, label }: { value: ReactNode; label: string }) {
  return (
    <Panel className="gap-2 p-6">
      <span className="text-text text-[32px] leading-none font-bold tracking-[-0.03em]">
        {value}
      </span>
      <span className="text-label font-mono text-[10px] tracking-[0.16em] uppercase">
        {label}
      </span>
    </Panel>
  );
}

/**
 * A list marked with the board's square bullet.
 *
 * `accent` uses vermilion squares, for a short list that is making a point;
 * the default uses the hairline colour, for an ordinary list.
 */
export function MarkedList({
  items,
  accent,
  className,
}: {
  items: readonly string[];
  accent?: boolean;
  className?: string;
}) {
  return (
    <ul className={`flex list-none flex-col gap-2.5 pl-0 ${className ?? ""}`}>
      {items.map((item) => (
        <li
          key={item}
          className={`text-secondary relative pl-6 text-[14.5px] leading-[1.6] before:absolute before:top-[0.55em] before:left-0 before:h-[6px] before:w-[6px] before:content-[''] ${
            accent ? "before:bg-cinnabar" : "before:bg-hairline-strong"
          }`}
        >
          {item}
        </li>
      ))}
    </ul>
  );
}
