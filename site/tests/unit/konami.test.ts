import { describe, expect, it } from "vitest";
import { endsWithKonami, KONAMI_SEQUENCE } from "@/lib/konami";

// ArrowUp ArrowUp ArrowDown ArrowDown ArrowLeft ArrowRight ArrowLeft
// ArrowRight KeyB KeyA -- spelled from the constant so these tests follow the
// sequence itself rather than restating it.
const FULL = [...KONAMI_SEQUENCE];

describe("endsWithKonami", () => {
  it("rejects an empty buffer", () => {
    expect(endsWithKonami([])).toBe(false);
  });

  it("rejects a buffer shorter than the sequence", () => {
    // A correct prefix, one key short of the full ritual.
    expect(endsWithKonami(FULL.slice(0, -1))).toBe(false);
  });

  it("accepts a buffer that is exactly the full sequence", () => {
    expect(endsWithKonami(FULL)).toBe(true);
  });

  it("accepts the sequence with unrelated noise before it", () => {
    expect(endsWithKonami(["KeyQ", "Space", "Enter", ...FULL])).toBe(true);
  });

  it("rejects a sequence broken partway through", () => {
    // Correct up to the two ArrowDowns, then a stray key, then the rest --
    // every code from the sequence is present, but not consecutively.
    const broken = [...FULL.slice(0, 4), "KeyX", ...FULL.slice(4)];
    expect(endsWithKonami(broken)).toBe(false);
  });

  it("rejects the full sequence with trailing keys after it -- only the tail counts", () => {
    // The complete sequence is in the buffer, but the visitor kept typing:
    // the buffer no longer *ends* with the sequence, so it must not match.
    expect(endsWithKonami([...FULL, "KeyB", "KeyA"])).toBe(false);
    expect(endsWithKonami([...FULL, "ArrowUp"])).toBe(false);
  });
});
