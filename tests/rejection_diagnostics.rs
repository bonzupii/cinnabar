//! Bundled rejection fixtures, asserted diagnostic by diagnostic.
//!
//! A fixture carrying several independent rejections still fails on its
//! siblings when one of them stops being reported, so exit codes alone stay
//! green while a class of program quietly becomes accepted. Each bundled
//! rejection here is asserted individually, against the ordered list of
//! messages (order catches reshuffles; length catches losses and additions).
//!
//! **Invariants:**
//! - Every diagnostic a bundle carries is asserted by text.
//! - A fixture with a single rejection does not belong here; the exit code
//!   already covers it.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Bundle {
    path: &'static str,
    /// Every diagnostic the fixture is meant to produce, in the order the
    /// compiler reports them.
    diagnostics: &'static [&'static str],
}

const BUNDLES: &[Bundle] = &[
    // Every disallowed `nat` position must be rejected.
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
    // Resolver failures hide later type errors.
    Bundle {
        path: "tests/fixtures/invalid_resolver_and_typechecker.cnb",
        diagnostics: &[
            "duplicate symbol 'DUPLICATE_CONST'",
            "cannot redeclare builtin 'Result'",
            "cannot redeclare builtin 'Option'",
            "cannot resolve import 'NonExistentModule.some_func'",
        ],
    },
    // One violation per casing rule.
    Bundle {
        path: "tests/fixtures/invalid_casing.cnb",
        diagnostics: &[
            "'camelCaseFunction' violates casing rule: expected snake_case",
            "'Bad_Const_Name' violates casing rule: expected SCREAMING_SNAKE_CASE",
            "'bad_type_name' violates casing rule: expected PascalCase",
        ],
    },
    // The malformed literal, then the character the lexer resumed on.
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
    // All three discard spellings, rejected in the lexer.
    Bundle {
        path: "tests/fixtures/09_discard_patterns.cnb",
        diagnostics: &[
            "discard pattern '_' is not allowed; bind the value with a real name and use it, or split the match arm so each variant has its own",
            "discard pattern '_' is not allowed; bind the value with a real name and use it, or split the match arm so each variant has its own",
            "'_unused' begins with an underscore, which marks a value as deliberately unused; bind it with a real name and use it",
        ],
    },
    // Reachability covers public and private items alike.
    Bundle {
        path: "tests/fixtures/dead_code.cnb",
        diagnostics: &["unused function 'unused_helper'", "unused constant 'UNUSED_CONST'"],
    },
    // Type-checking cases in a file the resolver accepts.
    Bundle {
        path: "tests/fixtures/repro/match_arm_multiline_recovery.cnb",
        diagnostics: &["match arm body must be a single expression; move multi-statement blocks into a helper function"],
    },
    Bundle {
        path: "tests/fixtures/repro/nested_native_mod_ice.cnb",
        diagnostics: &["unknown native function 'Display.Terminal.print'"],
    },
    Bundle {
        path: "tests/fixtures/repro/single_line_if_syntax.cnb",
        diagnostics: &["expected a newline before the if body"],
    },
    Bundle {
        path: "tests/fixtures/repro/loader_poison_cascade/Main.cnb",
        diagnostics: &["expected '='"],
    },
    // Three unconsumed `pub` modifiers on reached root-scope items.
    Bundle {
        path: "tests/fixtures/repro/unnecessary_pub.cnb",
        diagnostics: &[
            "pub on 'helper' has no cross-module caller",
            "pub on 'LIMIT' has no cross-module caller",
            "pub on 'main' has no cross-module caller",
        ],
    },
    // Two `pub` fields on a struct that only its own module touches.
    Bundle {
        path: "tests/fixtures/repro/unnecessary_field_pub.cnb",
        diagnostics: &[
            "pub on field 'x' has no cross-module access",
            "pub on field 'y' has no cross-module access",
        ],
    },
    // A dead implementing type: the impl's method is reported with it.
    Bundle {
        path: "tests/fixtures/repro/dead_impl.cnb",
        diagnostics: &["unused struct 'DeadType'", "unused method 'greet'"],
    },
    // Two enums nothing constructs or matches.
    Bundle {
        path: "tests/fixtures/repro/dead_enum.cnb",
        diagnostics: &["unused enum 'Color'", "unused enum 'Shape'"],
    },
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

/// Removes ariadne's colour sequences (`ESC [ … letter`) so diagnostics
/// can be matched by text.
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

/// The messages the compiler reported for `path`, in order; both streams read.
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

/// The pipeline stops at the first failing stage, so a fixture cannot span
/// two stages: the shadowing bundle and the split file must share no message.
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
