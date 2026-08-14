"use client";

import { useInView } from "motion/react";
import { useRef, type ReactNode } from "react";

/*
 * Entrance motion.
 *
 * The brand's motion principle is restraint: 120-200ms, no bounce, no spin.
 * These reveals are a short fade with a few pixels of travel, played once when
 * the element first scrolls into view — enough to give the hairline grids a
 * sense of assembly without becoming an effect.
 *
 * A scroll reveal needs a hidden state before the element is revealed, and
 * where that hidden state comes from is the whole design problem:
 *
 * - Rendering it in the markup (what motion's `initial` prop does) ships
 *   `opacity: 0` in the HTML. Without JavaScript every revealed section is
 *   then permanently invisible.
 * - Applying it at hydration means above-the-fold content paints, disappears
 *   and fades back in — a visible flash on every load.
 *
 * So the hidden state is applied by CSS, gated on a `js` class that the same
 * pre-paint script that restores the theme puts on <html>. No JavaScript means
 * the class is never added and everything is simply visible; with JavaScript
 * the class is present before the first paint, so nothing flashes. This
 * component only decides *when* to reveal — `useInView` from motion — and the
 * transition itself is CSS, which is also what makes the reduced-motion case
 * exact rather than merely fast.
 */

type RevealProps = {
  children: ReactNode;
  /** Seconds of stagger, for revealing a row of cards in sequence. */
  delay?: number;
  className?: string;
  as?: "div" | "section" | "li" | "article";
};

export default function Reveal({
  children,
  delay = 0,
  className,
  as: Component = "div",
}: RevealProps) {
  const ref = useRef<HTMLElement>(null);
  const inView = useInView(ref, {
    once: true,
    amount: 0.15,
    margin: "0px 0px -80px 0px",
  });

  return (
    <Component
      // The ref types differ per tag; the element type is the same either way.
      ref={ref as React.Ref<never>}
      className={`reveal${inView ? " is-in" : ""}${className ? ` ${className}` : ""}`}
      style={delay > 0 ? { transitionDelay: `${delay}s` } : undefined}
    >
      {children}
    </Component>
  );
}
