/*
 * Theme storage, shared by the pre-paint script and the toggle.
 *
 * This must be a plain module. The key previously lived in ThemeToggle, which
 * carries "use client": importing a value out of a client module from server
 * code yields a client reference rather than the string, so the generated
 * script read `localStorage.getItem(undefined)` and silently never restored a
 * theme. Keeping the constant here means both sides read the same literal.
 */

export const THEME_STORAGE_KEY = "cinnabar-theme";

export type Theme = "light" | "dark";

/**
 * Applies a stored theme choice before the first paint, and marks the document
 * as JavaScript-capable.
 *
 * Rendered as an inline script at the top of <body>, so it runs before any of
 * the page below it is painted. It only stamps an explicit stored choice —
 * with none stored the CSS media query decides, so there is nothing to do and
 * nothing to flash.
 *
 * The `js` class is what gates the entrance reveals in globals.css: their
 * hidden state must never reach a reader without JavaScript, and must be in
 * place before the first paint for a reader with it.
 */
export const THEME_INIT_SCRIPT = `(function(){var r=document.documentElement;r.classList.add("js");try{var t=localStorage.getItem("${THEME_STORAGE_KEY}");if(t==="light"||t==="dark"){r.setAttribute("data-theme",t)}}catch(e){}})();`;
