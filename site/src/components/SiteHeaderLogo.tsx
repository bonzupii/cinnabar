"use client";

import Link from "next/link";
import { useRef } from "react";
import CinnabarMark from "@/components/brand/CinnabarMark";
import Wordmark from "@/components/brand/Wordmark";
import {
  hasTriggeringClickBurst,
  LOGO_CLICK_THRESHOLD,
  OPEN_MUSHROOM_EGG_EVENT,
} from "@/lib/logo-easter-egg";

/*
 * The header's home link, split out of SiteHeader (a server component) only
 * because it now also hides the easter egg's second trigger: spam-click the
 * logo and MushroomEasterEgg opens, the "tap the build number" gag. Markup,
 * classes and aria-label are exactly what SiteHeader rendered inline before
 * the split — the click counting is the sole addition.
 *
 * The counter lives in a ref, not state: crossing the threshold triggers a
 * window event, never a render. Timestamps are pruned to the threshold's
 * length, mirroring the rolling key buffer in MushroomEasterEgg; the pure
 * burst check itself lives in lib/logo-easter-egg.ts. Navigation is left
 * alone — the link still goes home on every click, egg or no egg, so the
 * trigger costs a normal visitor nothing.
 */
export default function SiteHeaderLogo() {
  const clickTimesRef = useRef<number[]>([]);

  const onClick = () => {
    const clicks = clickTimesRef.current;
    const now = Date.now();
    clicks.push(now);
    if (clicks.length > LOGO_CLICK_THRESHOLD) {
      clicks.splice(0, clicks.length - LOGO_CLICK_THRESHOLD);
    }
    if (hasTriggeringClickBurst(clicks, now)) {
      clicks.length = 0;
      window.dispatchEvent(new CustomEvent(OPEN_MUSHROOM_EGG_EVENT));
    }
  };

  return (
    <Link
      href="/"
      aria-label="Cinnabar — home"
      className="flex items-center focus-visible:outline-offset-4"
      onClick={onClick}
    >
      {/*
        The wrapping span, not a className on CinnabarMark directly: its
        <svg> sets `display: block` inline (CinnabarMark.tsx), which a
        Tailwind class on the same element can't win against -- an
        inline style always beats an external stylesheet rule of equal
        or lower priority, `!important` aside.
      */}
      <span className="min-[400px]:hidden">
        <CinnabarMark size={26} letter="var(--text)" />
      </span>
      <Wordmark size={26} letter="var(--text)" className="hidden min-[400px]:block" />
    </Link>
  );
}
