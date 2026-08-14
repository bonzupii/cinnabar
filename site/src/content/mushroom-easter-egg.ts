/*
 * The foraging puzzle behind the Konami-code easter egg (MushroomEasterEgg):
 * template pieces and pure helpers, kept apart from the DOM wiring the same
 * way lib/konami.ts and lib/logo-easter-egg.ts keep their matching logic out
 * of their components.
 *
 * Three mushrooms. Each can be foraged (bound with a `val` from a fallible
 * `find`) and then eaten (consumed). Every move appends real Cinnabar to a
 * growing `main()`, and the component runs the assembled program through the
 * live check() after each one. The puzzle's rule is the borrow checker's
 * own, not scripted copy: every `match find(...)` block carries an
 * `Err => return 1` arm — a real control-flow path — and any mushroom held
 * but not yet eaten leaks on that path. So forage-all-then-eat-all is
 * rejected ("linear value 'mushroom1' must be consumed before returning",
 * pointing at the next forage's error arm), while fully dispatching one
 * mushroom before starting the next checks clean. Eating a mushroom twice is
 * the same "use of moved value" the old two-scene egg demonstrated, still
 * reachable by clicking an eaten tile again.
 *
 * Every outcome above was verified against the real check() — the same wasm
 * build this site ships — before being committed, the discipline
 * playground-samples.ts applies to its entries. The egg calls check() live
 * rather than showing a canned transcript, so an edit here changes what
 * visitors watch the checker do: re-verify against check() before changing a
 * character.
 */

/** The synthetic path the egg's windows are titled with. */
export const MUSHROOM_ENTRY_PATH = "mushroom.cnb";

/**
 * The three tiles. Each carries a fixed 1-based number: tile N always binds
 * `mushroomN`, whatever order it's foraged in.
 */
export const MUSHROOM_IDS = [1, 2, 3] as const;

export type MushroomId = (typeof MUSHROOM_IDS)[number];

/** One click: forage picks a mushroom up, eat consumes it. */
export type MushroomMove = {
  kind: "forage" | "eat";
  mushroom: MushroomId;
};

/** What a tile looks like, derived from the move log alone. */
export type MushroomState = "growing" | "held" | "eaten";

/** Everything before the first move: module, imports, the open `main()`. */
export const MUSHROOM_PREAMBLE = `pub mod Forage
  pub nat type Mushroom

  pub type Error
    pub NoneFound
  end

  pub nat fun find(species: &[U8]) impure Result(Mushroom, Error)
  pub nat fun eat(mushroom: Mushroom) impure Unit
end

use Forage.find
use Forage.eat

pub const SPECIES: &[U8] = "Cantharellus cinnabarinus"

pub fun main() impure I64`;

/** Everything after the last move. */
export const MUSHROOM_SUFFIX = `  return 0
end`;

/**
 * The block a forage move appends. `find` is fallible, so every forage
 * brings its own `Err => return 1` path into `main` — which is exactly what
 * makes the move order matter to the checker.
 */
export function forageBlock(mushroom: MushroomId): string {
  return `  val mushroom${mushroom} = match find(SPECIES)
    Ok(value) => value
    Err(Forage.NoneFound) => return 1
  end`;
}

/** The line an eat move appends. */
export function eatLine(mushroom: MushroomId): string {
  return `  eat(mushroom${mushroom})`;
}

/**
 * The full program: preamble, then each move's block or line in the order
 * the moves were made, then the suffix. Zero moves parses fine but draws a
 * real "unused constant 'SPECIES'" error — verified live — which the egg
 * leans on as the opening nudge: forage something and SPECIES gets used.
 */
export function assembleMushroomProgram(moves: readonly MushroomMove[]): string {
  const middle = moves.map((move) =>
    move.kind === "forage" ? forageBlock(move.mushroom) : eatLine(move.mushroom),
  );
  return [MUSHROOM_PREAMBLE, ...middle, MUSHROOM_SUFFIX].join("\n");
}

/**
 * A tile's current look. Eaten wins over held so a double-eaten mushroom
 * still reads as eaten; the UI only ever appends forage-before-eat per tile,
 * but the helper stays total over any log undo can produce.
 */
export function mushroomState(
  moves: readonly MushroomMove[],
  mushroom: MushroomId,
): MushroomState {
  let foraged = false;
  let eaten = false;
  for (const move of moves) {
    if (move.mushroom !== mushroom) continue;
    if (move.kind === "forage") foraged = true;
    else eaten = true;
  }
  if (eaten) return "eaten";
  return foraged ? "held" : "growing";
}

/**
 * The move-log half of winning: every mushroom has exactly one forage and
 * exactly one eat. The other half — the live report coming back with zero
 * diagnostics — stays with the checker, and only both together count as
 * solved. "Exactly" is load-bearing: a double eat un-completes the log even
 * though the tile still shows eaten.
 */
export function isPuzzleComplete(moves: readonly MushroomMove[]): boolean {
  return MUSHROOM_IDS.every((id) => {
    let forages = 0;
    let eats = 0;
    for (const move of moves) {
      if (move.mushroom !== id) continue;
      if (move.kind === "forage") forages += 1;
      else eats += 1;
    }
    return forages === 1 && eats === 1;
  });
}

/**
 * The `mushroomN` binding names present in the assembled program, for
 * CodeBlock's linear-handle underlines. A name exists once its forage block
 * does, eaten or not.
 */
export function mushroomHandles(moves: readonly MushroomMove[]): string[] {
  const foraged = new Set<MushroomId>();
  for (const move of moves) {
    if (move.kind === "forage") foraged.add(move.mushroom);
  }
  return MUSHROOM_IDS.filter((id) => foraged.has(id)).map((id) => `mushroom${id}`);
}
