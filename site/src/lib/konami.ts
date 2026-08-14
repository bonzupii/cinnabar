/*
 * The Konami sequence and its matcher, kept apart from any DOM wiring.
 *
 * Codes are `KeyboardEvent.code` values, not `.key`: `.key` follows the
 * active keyboard layout — B is somewhere else on Dvorak, and an IME can
 * rewrite it entirely — while `.code` names the physical key, which is what
 * the ritual actually is. The matcher is pure so recognition can be tested
 * without synthesising keyboard events: the listener in MushroomEasterEgg
 * keeps a rolling buffer of recent codes and asks this one question of it.
 */

export const KONAMI_SEQUENCE = [
  "ArrowUp",
  "ArrowUp",
  "ArrowDown",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "ArrowLeft",
  "ArrowRight",
  "KeyB",
  "KeyA",
] as const;

/** Whether `buffer` currently ends with the complete sequence, in order. */
export function endsWithKonami(buffer: readonly string[]): boolean {
  if (buffer.length < KONAMI_SEQUENCE.length) return false;
  const offset = buffer.length - KONAMI_SEQUENCE.length;
  return KONAMI_SEQUENCE.every((code, index) => buffer[offset + index] === code);
}
