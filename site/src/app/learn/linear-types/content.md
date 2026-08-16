A linear value represents an obligation: ownership must reach one consuming
operation on every path out of its scope.

<!-- @lede -->

Native resources such as memory blocks, vectors, strings, and hash maps are
linear. They cannot be copied, silently dropped, or consumed twice.

## Follow the value, not a convention

The declared type is the source of truth. If a branch transfers a handle, the
other branch must also discharge its obligation before the function returns.
This catches leaks and double frees before code generation.

```cinnabar
pub fun release(block: Memory.Block) impure Unit
  Memory.deallocate(block)
  return Unit
end
```

After `deallocate`, `block` is consumed. Reading or consuming it again is an
error. Returning without consuming it is also an error.

## Borrow when ownership should stay put

A function can inspect or temporarily mutate a resource through a borrow. The
owner remains responsible for the final consuming operation.
