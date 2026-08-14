import { SAMPLES } from "@/content/samples";

/*
 * Starter programs for the playground -- deliberately not the same text as
 * `SAMPLES` in `content/samples.ts`.
 *
 * Those are excerpts trimmed for readability on the static homepage:
 * `SampleExplorer`/`CodeBlock` only ever display them, never check them, so
 * nothing there required them to be complete, standalone, compilable
 * programs. Loading one into a live checker and immediately showing a wall
 * of unresolved-import errors (the "Linear handles" excerpt omits its
 * `Collections` module declaration) is a bad first impression for a feature
 * whose whole point is showing the checker at work -- and completing that
 * excerpt turns up a second, genuine rejection: `fail_vec<T>` frees a
 * `Collections.Vec(T)` inside a still-generic function body, which the
 * conservative-linearity rule for unresolved type parameters (Manifesto
 * principle 7) rejects even though every real call site instantiates
 * `T = I64`.
 *
 * "Tail recursion" and "Slices and patterns" are self-contained as written,
 * so those two are reused verbatim. "Linear handles" is a fresh example
 * instead, over `Memory.Block` rather than `Collections.Vec` -- linear
 * consumption without the generic-parameter subtlety. Every entry here was
 * run through the real check() and confirmed to report zero diagnostics
 * before being committed.
 */

const MEMORY_BLOCK_SAMPLE = `pub mod Memory
  pub nat type Block

  pub type Error
    pub AllocationFailed(Usize)
    pub AccessOutOfBounds(Usize, Usize)
  end

  pub nat fun allocate(size: Usize) impure Result(Block, Error)
  pub nat fun deallocate(block: Block) impure Unit
  pub nat fun write_u8(block: &Block, offset: Usize, value: U8) impure Result(Unit, Error)
  pub nat fun read_u8(block: &Block, offset: Usize) impure Result(U8, Error)
end

use Memory.allocate
use Memory.deallocate
use Memory.write_u8
use Memory.read_u8

pub const SIZE: Usize = 16
pub const BYTE: U8 = 0x2A

pub fun main() impure I64
  val block = match allocate(SIZE)
    Ok(value) => value
    Err(error) => return 1
  end

  match write_u8(&block, 0, BYTE)
    Ok(Unit) => Unit
    Err(error) => Unit
  end

  val result = match read_u8(&block, 0)
    Ok(value) => value
    Err(error) => 0
  end

  deallocate(block)
  return I64.from(result)
end`;

export type PlaygroundSample = {
  id: string;
  label: string;
  code: string;
};

export const PLAYGROUND_SAMPLES: readonly PlaygroundSample[] = [
  { id: SAMPLES[0].id, label: SAMPLES[0].label, code: SAMPLES[0].code },
  { id: "memory", label: "Linear handles", code: MEMORY_BLOCK_SAMPLE },
  { id: SAMPLES[2].id, label: SAMPLES[2].label, code: SAMPLES[2].code },
];
