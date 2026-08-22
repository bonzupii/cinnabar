//! Fixtures whose contract is more than an exit code.
//!
//! Kept apart from `repro_corpus` so suites needing only the expected-success
//! list do not pull in a table they never read.

/// A fixture run under `run_with_streams`: its stdin and `argv` inputs, and
/// the exact bytes it must write to each output descriptor.
pub(crate) struct StreamCase {
    pub(crate) name: &'static str,
    pub(crate) args: &'static [&'static str],
    pub(crate) stdin: &'static [u8],
    pub(crate) stdout: &'static [u8],
    pub(crate) stderr: &'static [u8],
    pub(crate) exit: i32,
}

// Not reduced by the test profile; each case is its behaviour's only assertion.
pub(crate) const STREAM_CASES: &[StreamCase] = &[
    // The two standard-output writes abut; neither `eprint` reaches stdout,
    // though all four calls interleave in program order.
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

