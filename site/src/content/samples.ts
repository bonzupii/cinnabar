/*
 * Code samples.
 *
 * Every sample here is copied verbatim from the repository's known-good
 * fixture corpus, exactly as README.md presents them — none is hand-assembled
 * for the site. The `source` field names the fixture so a reader can check.
 */

export type Sample = {
  id: string;
  label: string;
  source: string;
  summary: string;
  code: string;
  /** Bindings the typechecker marks linear, for plate 09's dotted rule. */
  linearHandles?: readonly string[];
};

export const SAMPLES: readonly Sample[] = [
  {
    id: "hanoi",
    label: "Tail recursion",
    source: "tests/fixtures/repro/hanoi.cnb",
    summary:
      "Structs and strict tail position. hanoi_acc calls itself as the direct value of a return, which is the only self-recursive call the typechecker accepts; LLVM turns it into a jump at -O2.",
    code: `pub const DISKS: I64 = 8

pub type MoveCount
  pub moves: I64
end

fun hanoi_acc(n: I64, acc: I64) I64
  if n <= 0
    return acc
  end
  return hanoi_acc(n - 1, acc + acc + 1)
end

fun hanoi_moves(disks: I64) I64
  return hanoi_acc(disks, 0)
end

fun hanoi(n: I64) MoveCount
  return MoveCount(moves: hanoi_moves(n))
end

pub fun main() I64
  val result = hanoi(DISKS)
  return result.moves
end`,
  },
  {
    id: "vec",
    label: "Linear handles",
    source: "tests/fixtures/repro/vec_test.cnb",
    summary:
      "vec is a native handle, so it carries a consumption obligation. Both the error path and the success path have to discharge it — hence the fail_vec helper, which frees before returning.",
    linearHandles: ["vec"],
    code: `use Collections.vec_new
use Collections.vec_push
use Collections.vec_view
use Collections.vec_free

const BAD_NEW: I64 = 1
const BAD_PUSH: I64 = 2

fun fail_vec<T>(vec: Collections.Vec(T)) impure I64
  vec_free(vec)
  return BAD_PUSH
end

fun fill_squares(vec: &mut Collections.Vec(I64)) impure Result(Unit, Collections.Error)
  var i: I64 = 0
  while i < 5
    try vec_push(vec, i * i)
    i = i + 1
  end
  return Ok(Unit)
end

pub fun main() impure I64
  val vec = match vec_new[I64]()
    Ok(v) => v
    Err(error) => return BAD_NEW
  end

  val fill_result = fill_squares(&mut vec)
  match fill_result
    Ok(Unit) => Unit
    Err(error) => return fail_vec(vec)
  end

  val view = vec_view(&vec)
  vec_free(vec)          # linear handle consumed exactly once
  return 0
end`,
  },
  {
    id: "slice",
    label: "Slices and patterns",
    source: "tests/fixtures/repro/slice_test.cnb",
    summary:
      "Array rest-patterns and a tail-recursive fold. Match is exhaustive: every variant, array length and rest pattern has to be covered, and there is no catch-all arm to cover them with.",
    code: `pub mod Slice
  pub nat fun len<T>(view: &[T]) Usize
end

use Slice.len as slice_len

fun slice_sum_acc(view: &[U8], acc: Usize) Usize
  match view
    [] => return acc
    [first, rest @ ..] => return slice_sum_acc(rest, acc + Usize.from(first))
  end
end

fun slice_sum(view: &[U8]) Usize
  return slice_sum_acc(view, 0)
end

pub const MAGIC_BYTE_0: U8 = 0x0D
pub const MAGIC_BYTE_1: U8 = 0xF0
pub const MAGIC_BYTE_2: U8 = 0xAD
pub const MAGIC_BYTE_3: U8 = 0x0B
pub const EXPECTED_SUM: Usize = 437

fun array_as_slice() Usize
  val bytes: [U8; 4] = [MAGIC_BYTE_0, MAGIC_BYTE_1, MAGIC_BYTE_2, MAGIC_BYTE_3]
  return slice_sum(&bytes)
end

pub fun main() I64
  if array_as_slice() == EXPECTED_SUM
    return 0
  end
  return 1
end`,
  },
] as const;

/** The manifest, which is Cinnabar source rather than a configuration format. */
export const MANIFEST_SAMPLE = `pub const NAME: &[U8] = "my_project"
pub const ENTRY: &[U8] = "main.cnb"
pub const TESTS: &[U8] = "tests"`;
