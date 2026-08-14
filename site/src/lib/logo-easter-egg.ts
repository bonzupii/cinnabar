/*
 * The logo-click trigger for the mushroom easter egg, kept apart from any DOM
 * wiring — the same split lib/konami.ts keeps for the keyboard trigger. The
 * component (SiteHeaderLogo) records click times and asks this one question;
 * the pure function can then be tested without synthesising click events.
 *
 * Threshold and window: five clicks inside 1.5 seconds. Five is the mouse-era
 * reading of the "tap the build number seven times" ritual — deliberate
 * spamming reaches it in under a second, while an accidental double- or even
 * triple-click stays two short. The 1.5s window is roomy enough that nobody
 * has to race, but short enough that five *navigation* clicks spread over a
 * browsing session never accumulate into a trigger.
 */

export const LOGO_CLICK_THRESHOLD = 5;
export const LOGO_CLICK_WINDOW_MS = 1500;

/**
 * The event SiteHeaderLogo dispatches on window when the burst lands, and
 * MushroomEasterEgg listens for. A plain CustomEvent, no payload: the two
 * components share only this name, not each other's internals.
 */
export const OPEN_MUSHROOM_EGG_EVENT = "cinnabar:open-mushroom-egg";

/**
 * Whether the run of click timestamps crosses the threshold: true when at
 * least LOGO_CLICK_THRESHOLD clicks (the current one included) happened
 * within the last LOGO_CLICK_WINDOW_MS.
 *
 * The window boundary is inclusive: a click exactly LOGO_CLICK_WINDOW_MS
 * before `now` still counts. Chosen so the constant reads literally — "within
 * 1500ms" includes the 1500th — and pinned by a unit test either way.
 */
export function hasTriggeringClickBurst(timestamps: readonly number[], now: number): boolean {
  let recent = 0;
  for (const timestamp of timestamps) {
    if (now - timestamp <= LOGO_CLICK_WINDOW_MS) recent += 1;
  }
  return recent >= LOGO_CLICK_THRESHOLD;
}
