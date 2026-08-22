//! The self-tail-recursive fixtures and their expected exit codes.
//!
//! Shared by the repro harness's every-opt-level run and the sanitizer
//! gate's memcheck run; a subset of `EXPECT_OK` kept in its own module so
//! only suites that scan it include it.
//!
//! **Invariants:**
//! - Every entry is also in `EXPECT_OK` with the same exit code.

pub(crate) const EXPECT_RECURSION: &[(&str, i32)] = &[
    ("tail_rec", 64),
    ("mem_probe", 0),
    ("hanoi", 255),
    ("rec_test", 120),
    ("fib", 155),
];
