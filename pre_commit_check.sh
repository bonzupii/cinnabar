#!/usr/bin/env bash
set -euo pipefail

echo "CLI help works"
cargo run --quiet -- --help > /dev/null

echo "No args fails"
if cargo run --quiet > /dev/null 2>&1; then
  echo "no args incorrectly accepted" >&2
  exit 1
fi

echo "Valid constructor test fixture parses and resolves"
cargo run --quiet -- tests/fixtures/constructor_parse.cnb

echo "AST dump contains Const nodes"
cargo run --quiet -- tests/fixtures/constructor_parse.cnb --dump-ast | grep -q 'Const('

echo "AST dump contains Path nodes"
cargo run --quiet -- tests/fixtures/constructor_parse.cnb --dump-ast | grep -q 'Path('

echo "pub use re-export parses and preserves is_pub: true"
cargo run --quiet -- tests/fixtures/pub_use.cnb --dump-ast | grep -q 'is_pub: true'

echo "Invalid native modifier on const is rejected"
if cargo run --quiet -- tests/fixtures/invalid_native_const.cnb > /dev/null 2>&1; then
  echo "invalid native const incorrectly accepted" >&2
  exit 1
fi

echo "Mixed struct field and enum variant type is rejected"
if cargo run --quiet -- tests/fixtures/invalid_mixed_type.cnb > /dev/null 2>&1; then
  echo "invalid mixed type declaration incorrectly accepted" >&2
  exit 1
fi

echo "Local pub val is rejected"
if cargo run --quiet -- tests/fixtures/pub_local_val.cnb > /dev/null 2>&1; then
  echo "local pub val incorrectly accepted" >&2
  exit 1
fi

echo "Unknown variable reference is rejected"
if cargo run --quiet -- tests/fixtures/unknown_var.cnb > /dev/null 2>&1; then
  echo "unknown variable reference incorrectly accepted" >&2
  exit 1
fi

echo "Reference specification (spec.cnb) parses and resolves cleanly"
cargo run --quiet -- tests/fixtures/spec.cnb

echo "AST dump for spec.cnb works"
cargo run --quiet -- tests/fixtures/spec.cnb --dump-ast > /dev/null

echo "Nested block comment is rejected"
if cargo run --quiet -- tests/fixtures/nested_block_comment.cnb > /dev/null 2>&1; then
  echo "nested block comment incorrectly accepted" >&2
  exit 1
fi

echo "Missing file is rejected"
if cargo run --quiet -- tests/fixtures/does_not_exist.cnb > /dev/null 2>&1; then
  echo "missing file incorrectly accepted" >&2
  exit 1
fi

echo "All pre-commit checks passed."
