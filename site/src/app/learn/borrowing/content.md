Borrows give temporary access without transferring the owner’s consumption
obligation.

<!-- @lede -->

`&T` is shared access and `&mut T` is exclusive access. The compiler derives
where each borrow remains live from the program’s control flow.

## Shared and exclusive access

Any number of shared borrows may coexist while no mutable borrow is active. A
mutable borrow excludes every other borrow of the same value until its last use.

```cinnabar
fun fill(values: &mut Collections.Vec(I64)) impure Unit
  # mutation is explicit at the boundary
  return Unit
end
```

There is no dereference operator. Field access, calls, and patterns express the
operation while the compiler manages indirection.

## No lifetime annotations

Scopes are flow-sensitive rather than named in an API. If a returned reference
could come from more than one input and the source is ambiguous, the function is
rejected; the solution is to restructure the API so ownership is clear.
