//! The expected-success fixture corpus.
//!
//! One table, shared by every suite that runs it. `repro_harness` runs these
//! for their exit codes; `sanitizer_gate` runs them again under a memory
//! checker. A second copy would drift, and the two suites would quietly stop
//! covering the same programs — which is exactly the kind of hand-maintained
//! duplicate AGENTS.md calls a standing correctness bug.
//!
//! **Invariants:**
//! - This table is the single definition of the expected-success corpus.
//!   A suite that needs a subset samples from it; it does not keep its own
//!   list.
//! - Each entry carries the exit code its fixture must produce, so adding a
//!   fixture means stating what it does rather than only that it compiles.

/// Fixture stem under `tests/fixtures/repro/`, and the exit code it must
/// produce.
pub(crate) const EXPECT_OK: &[(&str, i32)] = &[
    ("hello", 0),
    ("net_primitives", 0),
    ("liveness_many_bindings", 100),
    ("mini", 0),
    ("array_test", 0),
    ("borrow_index", 0),
    ("enum_array_index", 0),
    ("idx10d_mut_disjoint", 30),
    ("idx10e_same_expr_disjoint", 30),
    ("rec_test", 120),
    ("tail_rec", 64),
    ("mem_probe", 0),
    ("mem_byte_access", 0),
    ("hanoi", 255),
    ("head", 10),
    ("enum_test", 0),
    ("mem2", 0),
    ("vm2", 1),
    ("vm3", 1),
    ("vm4", 1),
    ("vm6", 1),
    ("vm7", 5),
    ("vm8", 1),
    ("vm9", 5),
    ("vm11", 4),
    ("vm", 120),
    ("continue_test", 9),
    ("jump_test", 3),
    ("jump2", 3),
    ("jump3", 3),
    ("jump4", 1),
    ("nested_continue_test", 109),
    ("elif_test", 1),
    ("elif_chain", 3),
    ("modulo", 42),
    ("div_runtime", 7),
    ("int_min_neg1", 0),
    ("shift_mask", 0),
    ("int_width_grid", 0),
    ("int_literal_context", 0),
    ("string_literal", 0),
    ("string_print", 0),
    ("string_static_borrow", 0),
    ("file_roundtrip", 0),
    ("runtime_io", 0),
    ("empty_block", 0),
    ("utf8_validation", 0),
    ("multiline_const", 30),
    ("fib", 155),
    ("linear_branch_consume", 0),
    ("linear_loop_consume", 0),
    ("linear_field_reinit", 0),
    ("linear_ref_swap", 0),
    ("linear_field_consume", 0),
    ("linear_ref_nonlinear_read", 14),
    ("ret_borrow_shared_twice", 0),
    ("ret_borrow_single_origin", 0),
    ("slice_test", 0),
    ("vec_pop_drain", 0),
    ("hash_map_remove_drain", 0),
];

/// A fixture whose contract is more than an exit code: what it is given on
/// standard input and in `argv`, and exactly what it must write to each
/// output descriptor.
///
/// `run_binary` gives every other fixture a null standard input and discards
/// both output streams. That is deliberate — a fixture's result must not
/// depend on the terminal the suite happened to run from — but it leaves
/// everything a program writes invisible. A `print` that wrote to standard
/// error, a `print_line` that dropped its terminator, a `read_line` that
/// kept the newline it consumed, or an argument table that stopped after
/// `argv[0]` would each leave every exit code in the suite unchanged.
///
/// These cases run under `run_with_streams` instead, which supplies exactly
/// the bytes named here and reads both descriptors back separately. The
/// input is still a definite state every run agrees on; it is simply a
/// richer one than "nothing".
pub(crate) struct StreamCase {
    pub(crate) name: &'static str,
    pub(crate) args: &'static [&'static str],
    pub(crate) stdin: &'static [u8],
    pub(crate) stdout: &'static [u8],
    pub(crate) stderr: &'static [u8],
    pub(crate) exit: i32,
}

// Not reduced by the test profile, unlike the expected-success corpus.
// There are four, each is the only assertion of the behaviour it covers,
// and a profile that dropped one would silently restore the blind spot the
// whole table exists to remove.
pub(crate) const STREAM_CASES: &[StreamCase] = &[
    // `print` adds nothing and `print_line` adds exactly one terminator, so
    // the two standard-output writes abut. Neither `eprint` reaches
    // standard output, though the four calls interleave in program order.
    StreamCase {
        name: "terminal_streams",
        args: &[],
        stdin: b"",
        stdout: b"out-aout-b\n",
        stderr: b"err-aerr-b",
        exit: 0,
    },
    // A line excludes its terminator, a blank line is a line rather than
    // end of input, and bytes followed by end of input are a final line.
    StreamCase {
        name: "terminal_lines",
        args: &[],
        stdin: b"alpha\n\nbeta",
        stdout: b"terminator excluded\nblank line is a line\nunterminated final line\nend of input\n",
        stderr: b"",
        exit: 0,
    },
    // A `String` holds well-formed UTF-8 whatever built it, so a line that
    // is not well-formed is a rejection rather than a `String`.
    StreamCase {
        name: "terminal_invalid_utf8",
        args: &[],
        stdin: b"\xff\n",
        stdout: b"malformed input rejected\n",
        stderr: b"",
        exit: 0,
    },
    // Two arguments of different lengths, so a table that stops after
    // `argv[0]` or reads one element twice changes the answer.
    StreamCase {
        name: "runtime_argv",
        args: &["alpha", "beta"],
        stdin: b"",
        stdout: b"arguments carried through\n",
        stderr: b"",
        exit: 0,
    },
];

