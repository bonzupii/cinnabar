"use client";

import { useSyncExternalStore } from "react";
import { THEME_STORAGE_KEY, type Theme } from "@/lib/theme";

/*
 * Light/dark toggle.
 *
 * Three states, not two. With no stored choice the page follows the system
 * preference (and falls back to dark, which is the brand's default); choosing
 * a theme stamps `data-theme` on <html> and persists it, which wins over the
 * system in both directions.
 *
 * The DOM attribute is the single source of truth rather than React state, so
 * the value the toggle reports can never disagree with the value the CSS is
 * actually using — including on the first paint, which the inline script in
 * the layout has already applied before this component mounts.
 */

function subscribe(onChange: () => void) {
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
  });

  // With no explicit choice stored, the system preference is the theme, so a
  // change to it is a change to what this control reports.
  const media = window.matchMedia("(prefers-color-scheme: light)");
  media.addEventListener("change", onChange);

  return () => {
    observer.disconnect();
    media.removeEventListener("change", onChange);
  };
}

/** The theme actually in effect, whether chosen explicitly or inherited. */
function getSnapshot(): Theme {
  const chosen = document.documentElement.getAttribute("data-theme");
  if (chosen === "light" || chosen === "dark") return chosen;
  return window.matchMedia("(prefers-color-scheme: light)").matches
    ? "light"
    : "dark";
}

function getServerSnapshot(): Theme {
  // The markup is prerendered; dark is the documented default.
  return "dark";
}

export default function ThemeToggle() {
  const theme = useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);

  function toggle() {
    const next = getSnapshot() === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", next);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch {
      // Storage can be unavailable (private mode, blocked cookies). The theme
      // still applies for this page view; it just will not be remembered.
    }
  }

  return (
    <button
      type="button"
      onClick={toggle}
      aria-label="Switch between light and dark theme"
      aria-pressed={theme === "dark"}
      className="border-hairline-strong text-text hover:border-text hover:bg-panel panel-hover pressable inline-flex h-9 w-9 items-center justify-center border"
    >
      <span className="theme-ic-dark items-center">
        <svg
          width="15"
          height="15"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.6"
          aria-hidden="true"
        >
          {/* A moon, cut from straight strokes and one arc — the icon set
              allows a curve only where the form demands it. */}
          <path d="M20 14.2A8 8 0 1 1 9.8 4 6.4 6.4 0 0 0 20 14.2z" />
        </svg>
      </span>
      <span className="theme-ic-light items-center">
        <svg
          width="15"
          height="15"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.6"
          aria-hidden="true"
        >
          {/* The spore-ring geometry from the mark studies: a centre and
              twelve radial marks, reduced to eight. */}
          <rect x="8.4" y="8.4" width="7.2" height="7.2" />
          <path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M18.7 5.3l-2.1 2.1M7.4 16.6l-2.1 2.1" />
        </svg>
      </span>
    </button>
  );
}
