//! Fixtures whose contract is more than an exit code.
//!
//! Kept apart from `repro_corpus` so a suite that only needs the
//! expected-success list does not pull in a table it never reads. One table
//! per file, each included where it is used.

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

