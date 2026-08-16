import { BuildIcon, CheckIcon, DocIcon, LinearIcon } from "@/components/brand/icons";
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
 * consumption without the generic-parameter subtlety. "Result & errors",
 * "Traits & impl" and "Enums & matching" are new, written directly for the
 * playground rather than adapted from a homepage excerpt. Every entry here
 * was run through the real check() and confirmed to report zero diagnostics
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

const ERROR_HANDLING_SAMPLE = `fun safe_divide(a: I64, b: I64) I64
  val result = match a / b
    Ok(value) => value
    Err(error) => 0
  end
  return result
end

pub fun main() I64
  val ok = safe_divide(10, 2)
  val zero = safe_divide(10, 0)
  return ok - 5 + zero
end`;

const TRAITS_SAMPLE = `pub type Header
  pub kind: U32
  pub flags: U32
end

pub type Tag
  pub value: U32
end

pub trait Checksum
  pub fun checksum(value: &Self) U32
end

pub impl Checksum for Header
  pub fun checksum(value: &Header) U32
    return (value.kind << 16) ^ value.flags
  end
end

pub impl Checksum for Tag
  pub fun checksum(value: &Tag) U32
    return value.value ^ 255
  end
end

fun checksum_value<T: Checksum>(value: &T) U32
  return Checksum.checksum(value)
end

pub fun main() I64
  val header = Header(kind: 1, flags: 2)
  val tag = Tag(value: 42)
  return I64.from(checksum_value(&header) + checksum_value(&tag))
end`;

const ENUMS_SAMPLE = `pub type Shape
  pub Circle(I64)
  pub Square(I64)
  pub Rectangle(I64, I64)
end

fun area(shape: Shape) I64
  match shape
    Circle(radius) => return radius * radius * 3
    Square(side) => return side * side
    Rectangle(w, h) => return w * h
  end
end

pub fun main() I64
  return area(Circle(2)) + area(Square(3)) + area(Rectangle(4, 5))
end`;

type PlaygroundSampleIcon = typeof LinearIcon;

export type PlaygroundSample = {
  id: string;
  label: string;
  code: string;
  icon: PlaygroundSampleIcon;
  /** Shown under the tabs for whichever sample is loaded. */
  summary: string;
};

export const PLAYGROUND_SAMPLES: readonly PlaygroundSample[] = [
  {
    id: "hanoi",
    label: "Tail recursion",
    code: SAMPLES.find((sample) => sample.id === "hanoi")?.code ?? "",
    icon: SAMPLES.find((sample) => sample.id === "hanoi")?.icon ?? BuildIcon,
    summary:
      "Every self-recursive call in tail position lowers to a jump — no runtime stack guard, no stack-overflow crash, however deep the recursion goes.",
  },
  {
    id: "memory",
    label: "Linear handles",
    code: MEMORY_BLOCK_SAMPLE,
    icon: LinearIcon,
    summary:
      "A Memory.Block handle must be consumed exactly once on every path out of scope — allocated, written, read, then freed, checked at compile time with no garbage collector.",
  },
  {
    id: "slice",
    label: "Slices and patterns",
    code: SAMPLES.find((sample) => sample.id === "slice")?.code ?? "",
    icon: SAMPLES.find((sample) => sample.id === "slice")?.icon ?? BuildIcon,
    summary:
      "Array and slice patterns destructure with a rest binding (rest @ ..), and a tail-recursive fold walks a slice without ever indexing into it.",
  },
  {
    id: "errors",
    label: "Result & errors",
    code: ERROR_HANDLING_SAMPLE,
    icon: CheckIcon,
    summary:
      "Division and modulo return Result instead of trapping — there is no runtime panic to catch, only a value to match on.",
  },
  {
    id: "traits",
    label: "Traits & impl",
    code: TRAITS_SAMPLE,
    icon: DocIcon,
    summary:
      "A trait declares a shared interface; two impl blocks provide it for unrelated types, and a bounded generic function dispatches through the trait alone.",
  },
  {
    id: "enums",
    label: "Enums & matching",
    code: ENUMS_SAMPLE,
    icon: BuildIcon,
    summary:
      "An enum with payloads (Circle(I64), Rectangle(I64, I64)) is destructured exhaustively by match, one arm per shape.",
  },
];
