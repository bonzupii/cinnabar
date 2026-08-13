#!/usr/bin/env bash
set -euo pipefail

LOG_FILE="./pre_commit.log"

# Initialize/clear log file
> "$LOG_FILE"

# ANSI color formatting
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Strips ANSI/CSI escape sequences (color, cursor movement, etc.) from
# captured command output before it reaches the log file. This does not
# touch the compiler or any tool's own color output on a real terminal —
# it only cleans what gets written to disk.
strip_ansi() {
  sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g; s/\x1b\][^\x07]*\x07//g'
}

run_check() {
  local description="$1"
  shift
  echo -e "${BLUE}[CHECK]${NC} ${description}..."
  echo -e "\n=== [CHECK] ${description} ===" >> "$LOG_FILE"
  local tmp_out
  tmp_out=$(mktemp)
  if "$@" > "$tmp_out" 2>&1; then
    strip_ansi < "$tmp_out" >> "$LOG_FILE"
    rm -f "$tmp_out"
    echo -e "${GREEN}[PASS]${NC} ${description}"
    echo "=== [PASS] ${description} ===" >> "$LOG_FILE"
  else
    strip_ansi < "$tmp_out" >> "$LOG_FILE"
    rm -f "$tmp_out"
    echo -e "${RED}[FAIL]${NC} ${description}" >&2
    echo "=== [FAIL] ${description} ===" >> "$LOG_FILE"
    exit 1
  fi
}

run_check_ast() {
  local description="$1"
  shift
  echo -e "${BLUE}[CHECK]${NC} ${description}..."
  echo -e "\n=== [CHECK] ${description} (AST dump excluded from log) ===" >> "$LOG_FILE"
  local tmp_out
  tmp_out=$(mktemp)
  if "$@" > "$tmp_out" 2>&1; then
    echo "(AST dump verified successfully - raw AST omitted from log)" >> "$LOG_FILE"
    rm -f "$tmp_out"
    echo -e "${GREEN}[PASS]${NC} ${description}"
    echo "=== [PASS] ${description} ===" >> "$LOG_FILE"
  else
    strip_ansi < "$tmp_out" >> "$LOG_FILE"
    rm -f "$tmp_out"
    echo -e "${RED}[FAIL]${NC} ${description}" >&2
    echo "=== [FAIL] ${description} ===" >> "$LOG_FILE"
    exit 1
  fi
}

expect_failure() {
  local description="$1"
  shift
  echo -e "${BLUE}[CHECK]${NC} ${description} (expecting rejection)..."
  echo -e "\n=== [CHECK] ${description} (expecting rejection) ===" >> "$LOG_FILE"
  local tmp_out
  tmp_out=$(mktemp)
  if "$@" > "$tmp_out" 2>&1; then
    strip_ansi < "$tmp_out" >> "$LOG_FILE"
    rm -f "$tmp_out"
    echo -e "${RED}[FAIL]${NC} ${description} was incorrectly accepted!" >&2
    echo "=== [FAIL] ${description} was incorrectly accepted! ===" >> "$LOG_FILE"
    exit 1
  else
    strip_ansi < "$tmp_out" >> "$LOG_FILE"
    rm -f "$tmp_out"
    echo -e "${GREEN}[PASS]${NC} ${description}"
    echo "=== [PASS] ${description} ===" >> "$LOG_FILE"
  fi
}

echo "=================================================="
echo "==== Cinnabar Toolchain Pre-Commit Test Suite ===="
echo "=================================================="
echo ""

# 1. Code Quality & Lint Gates
run_check "Cargo check" cargo check --quiet
run_check "Cargo clippy (zero warnings policy)" cargo clippy --quiet -- -D warnings
run_check "Semgrep (AGENTS.md policy: discard patterns, dummy spans, hardcoded registries)" semgrep --config .semgrep.yml --error --quiet .
run_check "Cargo unit test suite" cargo test --quiet

# 2. CLI Invocation Checks
run_check "CLI help display" cargo run --quiet -- --help
expect_failure "CLI with no arguments" cargo run --quiet

# 3. Positive Test Fixtures
run_check "Constructor fixture parses, resolves, and type-checks" cargo run --quiet -- tests/fixtures/constructor_parse.cnb
run_check "pub use re-export parses, resolves, and type-checks" cargo run --quiet -- tests/fixtures/pub_use.cnb
run_check "Multi-file external module import parses, loads, resolves, and type-checks" cargo run --quiet -- tests/fixtures/multi_file/main.cnb
run_check "Reference specification (spec.cnb) compiles to a binary" cargo run --quiet -- tests/fixtures/spec.cnb
run_check "Reference specification binary executes and passes every self-check (exit 0)" tests/fixtures/spec

# AST Dump Verification (AST pattern checked, raw AST omitted from log)
run_check_ast "AST dump for constructor_parse.cnb contains Const nodes" bash -c "cargo run --quiet -- tests/fixtures/constructor_parse.cnb --dump-ast | grep -q 'Const('"
run_check_ast "AST dump for constructor_parse.cnb contains Path nodes" bash -c "cargo run --quiet -- tests/fixtures/constructor_parse.cnb --dump-ast | grep -q 'Path('"
run_check_ast "AST dump for pub_use.cnb preserves is_pub: true" bash -c "cargo run --quiet -- tests/fixtures/pub_use.cnb --dump-ast | grep -q 'is_pub: true'"
run_check_ast "AST dump for spec.cnb works" cargo run --quiet -- tests/fixtures/spec.cnb --dump-ast

# 4. Negative Test Fixtures (All Diagnostic Errors Logged to pre_commit.log)
expect_failure "Rejecting invalid native modifier on const" cargo run --quiet -- tests/fixtures/invalid_native_const.cnb
expect_failure "Rejecting invalid native modifiers on non-native items" cargo run --quiet -- tests/fixtures/invalid_native_modifiers.cnb
expect_failure "Rejecting mixed struct field and enum variant type" cargo run --quiet -- tests/fixtures/invalid_mixed_type.cnb
expect_failure "Rejecting pub modifier on local val/var (pub_local_val.cnb)" cargo run --quiet -- tests/fixtures/pub_local_val.cnb
expect_failure "Rejecting pub modifier inside local scope (invalid_pub_local.cnb)" cargo run --quiet -- tests/fixtures/invalid_pub_local.cnb
expect_failure "Rejecting assignment to immutable val" cargo run --quiet -- tests/fixtures/immutable_assign.cnb
expect_failure "Rejecting unknown variable reference" cargo run --quiet -- tests/fixtures/unknown_var.cnb
expect_failure "Rejecting qualified path struct initialization" cargo run --quiet -- tests/fixtures/invalid_qualified_struct_init.cnb
expect_failure "Rejecting casing rule violations" cargo run --quiet -- tests/fixtures/invalid_casing.cnb
expect_failure "Rejecting invalid hex literals" cargo run --quiet -- tests/fixtures/invalid_hex_literal.cnb
expect_failure "Rejecting nested block comment (lexer)" cargo run --quiet -- tests/fixtures/nested_block_comment.cnb
expect_failure "Rejecting nested block comment (standalone)" cargo run --quiet -- tests/fixtures/invalid_nested_block_comment.cnb
expect_failure "Rejecting comprehensive resolver & typechecker error suite" cargo run --quiet -- tests/fixtures/invalid_resolver_and_typechecker.cnb
expect_failure "Rejecting missing input file" cargo run --quiet -- tests/fixtures/does_not_exist.cnb

# Rejection cases the cargo suites assert in detail, invoked here too so the
# gate itself fails if any of them stops being rejected at all.
expect_failure "Rejecting type-checking suite unreachable behind the resolver bundle" cargo run --quiet -- tests/fixtures/invalid_typechecker.cnb
expect_failure "Rejecting unused declarations, public and private" cargo run --quiet -- tests/fixtures/dead_code.cnb
expect_failure "Rejecting discard patterns in every position" cargo run --quiet -- tests/fixtures/09_discard_patterns.cnb
expect_failure "Rejecting a call to a non-public item from outside its module" cargo run --quiet -- tests/fixtures/08_dropped_pub.cnb
expect_failure "Rejecting an unconsumed linear handle" cargo run --quiet -- tests/fixtures/explain_leak.cnb
expect_failure "Rejecting an unhandled Result" cargo run --quiet -- tests/fixtures/mushling_unhandled_result.cnb
expect_failure "Rejecting an unresolved function name" cargo run --quiet -- tests/fixtures/suggest_unresolved_fn.cnb
expect_failure "Rejecting an unresolved type name" cargo run --quiet -- tests/fixtures/suggest_unresolved_type.cnb
expect_failure "Rejecting an ambiguous unresolved name" cargo run --quiet -- tests/fixtures/suggest_ambiguous.cnb

# 5. Cleanup: remove compiled fixture binaries (keep the cargo cache)
rm -f tests/fixtures/spec tests/fixtures/constructor_parse tests/fixtures/pub_use tests/fixtures/multi_file/main

echo ""
echo -e "${GREEN}=================================================="
echo "====== All pre-commit checks passed cleanly ======"
echo -e "==================================================${NC}"
