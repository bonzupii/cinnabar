Operations that can fail return a value describing both outcomes.

<!-- @lede -->

Cinnabar has no exceptions and no user-reachable panic path. `Result` and
`Option` keep failure visible in the type, while exhaustive `match` and `try`
make the chosen control flow visible in source.

## Match when outcomes need different work

```cinnabar
val value = match numerator / denominator
  Ok(result) => result
  Err(error) => return 0
end
```

Division, modulo, dynamic indexing, and allocation expose failure rather than
trapping. Constant invalid operations can be rejected at compile time.

## Propagate when this layer cannot recover

`try expression` returns early with the compatible `Err` or `None` case. It is
still explicit in the function body and valid only when the function’s return
type can carry that failure.

An unhandled `Result` or `Option` is an error, not a warning that can be ignored.
