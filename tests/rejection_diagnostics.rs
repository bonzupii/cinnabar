//! Bundled rejection fixtures, asserted diagnostic by diagnostic.
//!
//! Every other check on a rejected fixture asks only whether the compiler
//! exited non-zero. For a fixture carrying a single rejection that is
//! enough: drop the diagnostic and the fixture compiles, which the exit
//! code catches immediately.
//!
//! It is not enough for a fixture that bundles several independent
//! rejections. Stop reporting one of them and the fixture still fails on
//! its siblings, so the exit code is unchanged and the suite stays green
//! while a whole class of program has quietly become accepted. The
//! fixtures below are exactly those bundles.
//!
//! The comparison is against the ordered list of messages, not a set of
//! substrings that must appear somewhere. Order catches a reshuffle, and
//! the length catches both a lost diagnostic and an unintended new one.
//! `invalid_native_modifiers.cnb` shows why nothing weaker would do: its
//! five rejections all carry the same message, so only the count
//! distinguishes five reported sites from four.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Bundle {
    path: &'static str,
    /// Every diagnostic the fixture is meant to produce, in the order the
    /// compiler reports them.
    diagnostics: &'static [&'static str],
}

const BUNDLES: &[Bundle] = &[
    // One `nat` on each item kind that may not carry it. The message is
    // the same every time, so the count is the whole assertion: it is what
    // separates "every disallowed position is rejected" from "the first
    // one is".
    Bundle {
        path: "tests/fixtures/invalid_native_modifiers.cnb",
        diagnostics: &[
            "native modifier is only allowed on fun and type",
            "native modifier is only allowed on fun and type",
            "native modifier is only allowed on fun and type",
            "native modifier is only allowed on fun and type",
            "native modifier is only allowed on fun and type",
        ],
    },
    // Resolution runs before type checking and this fixture fails there,
    // so these four are everything it reports. The type-checking cases
    // further down the file are unreachable behind them; see the note in
    // `bundles_report_every_diagnostic_they_carry`.
    Bundle {
        path: "tests/fixtures/invalid_resolver_and_typechecker.cnb",
        diagnostics: &[
            "duplicate symbol 'DUPLICATE_CONST'",
            "cannot redeclare builtin 'Result'",
            "cannot redeclare builtin 'Option'",
            "cannot resolve import 'NonExistentModule.some_func'",
        ],
    },
    // One violation per casing rule the language enforces, so losing any
    // one of the three would leave that rule unenforced and unnoticed.
    Bundle {
        path: "tests/fixtures/invalid_casing.cnb",
        diagnostics: &[
            "'camelCaseFunction' violates casing rule: expected snake_case",
            "'Bad_Const_Name' violates casing rule: expected SCREAMING_SNAKE_CASE",
            "'bad_type_name' violates casing rule: expected PascalCase",
        ],
    },
    // The malformed literal, and then the character the lexer resumed on.
    // The second is the recovery behaving as intended rather than an
    // afterthought: a lexer that swallowed the rest of the line would
    // report only the first.
    Bundle {
        path: "tests/fixtures/invalid_hex_literal.cnb",
        diagnostics: &["expected hexadecimal digits after 0x", "unexpected character"],
    },
    // Division and modulo are folded by separate paths, so one going
    // unchecked is invisible while the other still rejects.
    Bundle {
        path: "tests/fixtures/repro/const_div_zero_cascade.cnb",
        diagnostics: &["division by zero in constant", "modulo by zero in constant"],
    },
    // The type-checking cases the bundle above can never reach, in a file
    // the resolver accepts. Some of these are consequences of others — the
    // `Option(?)` pair follows from `try` on an integer, and the inference
    // failure from the unusable Result — and they are listed because the
    // compiler reports them, not because the fixture set out to cause them.
    Bundle {
        path: "tests/fixtures/invalid_typechecker.cnb",
        diagnostics: &[
            "constant initializer type mismatch: expected 'I64', found 'Bool'",
            "cannot assign 'Bool' to 'I64'",
            "cannot assign 'Bool' to 'I64'",
            "cannot assign to 'immutable_val': assignment requires var",
            "cannot assign 'Bool' to 'I64'",
            "while condition must be Bool",
            "binary operator '+' requires integer operands",
            "logical operator '&&' requires Bool operands",
            "unary '-' requires an integer operand",
            "unary '!' requires a Bool operand",
            "return type mismatch: expected 'I64', found 'Bool'",
            "return with no value in a function returning 'I64'",
            "try on Result requires the enclosing function to return Result",
            "try on Option requires the enclosing function to return Option",
            "try requires a Result or Option operand",
            "constructed value type mismatch: expected 'Option(I64)', found 'Option(?)'",
            "return type mismatch: expected 'Option(I64)', found 'Option(?)'",
            "non-exhaustive match on 'I64': add a binding arm",
            "return type mismatch: expected 'I64', found 'Unit'",
            "match arms have different types: 'I64' and 'Bool'",
            "non-exhaustive match on 'I64': add a binding arm",
            "if condition must be Bool",
            "field 'x' has type 'Bool', expected 'I64'",
            "cannot infer type parameter 'E'",
        ],
    },
];

/// Removes the colour sequences ariadne writes, so a diagnostic can be
/// matched by its text. Ariadne emits only the `ESC [ … letter` form, which
/// ends at the first ASCII letter.
fn strip_ansi(text: &str) -> String {
    let mut out = String::new();
    let mut escaped = false;
    for ch in text.chars() {
        if escaped {
            if ch.is_ascii_alphabetic() {
                escaped = false;
            }
            continue;
        }
        if ch == '\u{1b}' {
            escaped = true;
            continue;
        }
        out.push(ch);
    }
    out
}

/// The messages the compiler reported for `path`, in order.
///
/// Both streams are read: a diagnostic belongs on standard error, and
/// reading only that stream would let one that escaped to standard output
/// register as a diagnostic that was never reported at all.
fn diagnostics_of(path: &Path) -> Vec<String> {
    let output = match Command::new(env!("CARGO_BIN_EXE_cinnabar")).arg(path).output() {
        Ok(output) => output,
        Err(err) => {
            assert!(false, "cannot run the compiler on {}: {}", path.display(), err);
            return Vec::new();
        }
    };
    assert!(
        !output.status.success(),
        "{} was accepted; a bundle of rejections must still be rejected",
        path.display()
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    strip_ansi(&combined)
        .lines()
        .filter_map(|line| line.strip_prefix("Error: ").map(str::to_string))
        .collect()
}

#[test]
fn bundles_report_every_diagnostic_they_carry() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut idx = 0usize;
    while idx < BUNDLES.len() {
        let bundle = match BUNDLES.get(idx) {
            Some(bundle) => bundle,
            None => break,
        };
        let path = PathBuf::from(root).join(bundle.path);
        let reported = diagnostics_of(&path);
        let expected: Vec<String> = bundle.diagnostics.iter().map(|text| text.to_string()).collect();
        assert_eq!(
            reported, expected,
            "{} reported a different set of diagnostics than it carries",
            bundle.path
        );
        idx += 1;
    }
}

/// A bundle whose rejections span two stages reports only the earlier one,
/// which is why the type-checking cases needed a file of their own.
///
/// The pipeline stops at the first stage that fails.
/// `invalid_resolver_and_typechecker.cnb` carries twenty-four numbered
/// cases and reports four, all from the resolver; the nineteen
/// type-checking ones sit behind them asserting nothing. That is not a
/// defect in the compiler — the staging is deliberate — but it does mean a
/// fixture cannot bundle rejections from two stages and expect both to be
/// checked.
///
/// So the two must not overlap: nothing `invalid_typechecker.cnb` reports
/// may also be reported by the file that shadows it, or the split has
/// quietly stopped recovering anything. Asserted against what the compiler
/// actually reports, rather than against a line count or a guess at which
/// messages belong to which stage.
#[test]
fn the_shadowed_stage_reports_nothing_the_split_file_covers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let shadowing = diagnostics_of(
        &PathBuf::from(root).join("tests/fixtures/invalid_resolver_and_typechecker.cnb"),
    );
    let recovered =
        diagnostics_of(&PathBuf::from(root).join("tests/fixtures/invalid_typechecker.cnb"));
    assert!(
        !shadowing.is_empty() && !recovered.is_empty(),
        "both fixtures must still be rejected for the comparison to mean anything"
    );
    assert!(
        recovered.len() > shadowing.len(),
        "the split file recovered {} diagnostics against the {} the bundle \
         reports, so it is no longer recovering the shadowed stage",
        recovered.len(),
        shadowing.len()
    );
    let mut idx = 0usize;
    while idx < recovered.len() {
        match recovered.get(idx) {
            Some(message) => assert!(
                !shadowing.contains(message),
                "'{}' is reported by both fixtures, so the bundle is no longer \
                 shadowing the stage the split file exists to reach",
                message
            ),
            None => break,
        }
        idx += 1;
    }
}
