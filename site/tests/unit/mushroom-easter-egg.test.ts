import { describe, expect, it } from "vitest";
import {
  assembleMushroomProgram,
  eatLine,
  forageBlock,
  isPuzzleComplete,
  MUSHROOM_PREAMBLE,
  MUSHROOM_SUFFIX,
  mushroomHandles,
  mushroomState,
  type MushroomMove,
} from "@/content/mushroom-easter-egg";

// Move logs are spelled out inline: unlike the konami/logo tests there is no
// tuned constant to derive them from -- the log IS the input, and each test's
// point is which exact sequence it feeds the helpers.

/** Shorthand: `f(2)` is a forage of mushroom 2, `e(2)` eats it. */
const f = (mushroom: 1 | 2 | 3): MushroomMove => ({ kind: "forage", mushroom });
const e = (mushroom: 1 | 2 | 3): MushroomMove => ({ kind: "eat", mushroom });

describe("assembleMushroomProgram", () => {
  it("assembles preamble and suffix only for the empty log, with no blank middle lines", () => {
    // Zero moves is a real program the component checks live (it draws the
    // "unused constant 'SPECIES'" nudge), so its shape matters: the suffix
    // must directly follow the preamble, not sit across an empty line.
    expect(assembleMushroomProgram([])).toBe(`${MUSHROOM_PREAMBLE}\n${MUSHROOM_SUFFIX}`);
  });

  it("assembles a single forage as exactly the block forageBlock returns", () => {
    expect(assembleMushroomProgram([f(2)])).toBe(
      [MUSHROOM_PREAMBLE, forageBlock(2), MUSHROOM_SUFFIX].join("\n"),
    );
  });

  it("keeps moves in the order they were made, not grouped by mushroom or kind", () => {
    // The order is the whole puzzle -- the checker's verdict hangs on where
    // each forage's error arm falls relative to still-held mushrooms, so the
    // assembler must never sort or group what the visitor actually did.
    expect(assembleMushroomProgram([f(2), e(2), f(1)])).toBe(
      [MUSHROOM_PREAMBLE, forageBlock(2), eatLine(2), forageBlock(1), MUSHROOM_SUFFIX].join(
        "\n",
      ),
    );
  });
});

describe("mushroomState", () => {
  it("reports growing before any move touches the mushroom", () => {
    expect(mushroomState([], 1)).toBe("growing");
    // Other mushrooms' moves are not this tile's business.
    expect(mushroomState([f(2), e(2)], 1)).toBe("growing");
  });

  it("reports held after a forage", () => {
    expect(mushroomState([f(1)], 1)).toBe("held");
  });

  it("reports eaten after forage then eat", () => {
    expect(mushroomState([f(1), e(1)], 1)).toBe("eaten");
  });

  it("still reports eaten after a double eat -- eaten wins over held", () => {
    // The doc comment's rule: the helper stays total over any log undo can
    // produce, and once any eat is in the log the tile reads eaten no matter
    // how many forages sit alongside it.
    expect(mushroomState([f(1), e(1), e(1)], 1)).toBe("eaten");
  });
});

describe("isPuzzleComplete", () => {
  it("is false for the empty log", () => {
    expect(isPuzzleComplete([])).toBe(false);
  });

  it("is false while only some mushrooms are dispatched", () => {
    expect(isPuzzleComplete([f(1), e(1)])).toBe(false);
    expect(isPuzzleComplete([f(1), e(1), f(2), e(2), f(3)])).toBe(false);
  });

  it("is true once every mushroom has exactly one forage and one eat", () => {
    expect(isPuzzleComplete([f(1), e(1), f(2), e(2), f(3), e(3)])).toBe(true);
    // Completeness is the log's half of winning; order is the checker's.
    // Forage-all-then-eat-all is a complete log too -- it just fails the
    // checker's half, which is not this helper's call to make.
    expect(isPuzzleComplete([f(1), f(2), f(3), e(1), e(2), e(3)])).toBe(true);
  });

  it("is false again once a mushroom has been eaten twice -- exactly means exactly", () => {
    // Every mushroom has been touched, but mushroom 1 carries two eats. The
    // doc comment calls "exactly" load-bearing: a double eat un-completes the
    // log even though the tile still shows eaten.
    expect(isPuzzleComplete([f(1), e(1), f(2), e(2), f(3), e(3), e(1)])).toBe(false);
  });
});

describe("mushroomHandles", () => {
  it("names nothing before any forage", () => {
    expect(mushroomHandles([])).toEqual([]);
    // An eat alone (reachable only through pathological logs, but the helper
    // is total) does not conjure a binding that was never foraged.
    expect(mushroomHandles([e(1)])).toEqual([]);
  });

  it("names only foraged mushrooms", () => {
    expect(mushroomHandles([f(2)])).toEqual(["mushroom2"]);
  });

  it("orders names by ascending id regardless of forage order", () => {
    expect(mushroomHandles([f(3), f(1)])).toEqual(["mushroom1", "mushroom3"]);
  });

  it("keeps a handle after its mushroom is eaten -- the binding name is still in the program", () => {
    expect(mushroomHandles([f(2), e(2), f(1), e(1)])).toEqual(["mushroom1", "mushroom2"]);
  });
});
