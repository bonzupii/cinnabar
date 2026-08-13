//! Milestone 8 — every valid program run under a memory checker.
//!
//! ## Why this needs a second link mode
//!
//! A shipped Cinnabar binary is static, `-nostdlib`, `-no-pie`, linked
//! against a musl `libc.a` embedded in the compiler. It carries no dynamic
//! section, so Valgrind's memcheck has nothing to interpose on: it reports
//! `0 allocs, 0 frees` for a program that demonstrably allocates, at every
//! optimization level. That is the observation recorded at the top of
//! `tests/native_memory.rs`, and it is why the properties there had to be
//! asserted against the emitted IR instead.
//!
//! `LinkMode::Instrumented` exists for this gate alone: the same object
//! file, linked dynamically against the host libc so the checker has a
//! `malloc` to hook. The shipped link is untouched — the static-only rule
//! for release output is not relaxed, a second mode is added beside it, and
//! no binary a user receives is built this way.
//!
//! ## What is checked, and what is not
//!
//! **Valgrind memcheck** runs here. It needs no compile-time instrumentation,
//! so it applies to Cinnabar-emitted code exactly as it does to anything
//! else, and it is the check that closes the gap above: invalid reads and
//! writes, use of uninitialised values, invalid frees, and definite leaks.
//!
//! **UBSan is absent, and cannot simply be switched on.** Its checks are
//! emitted by Clang's *front end* while lowering C, not by an LLVM pass over
//! finished IR. There is no `opt` pass to run over a Cinnabar module, so
//! covering it means the Cinnabar emitter emitting the checks itself. That is
//! emitter work, not gate configuration, and claiming UBSan coverage without
//! it would be claiming a check nothing performs.
//!
//! **ASan is absent for a narrower reason:** its runtime links and works
//! here, but the instrumentation is an IR pass that would have to run between
//! `opt` and `llc` in the compiler's own pipeline. That is a real follow-up
//! and is tracked in ROADMAP.md; Valgrind already covers the heap errors and
//! leaks that motivated the milestone.
//!
//! ## Fixture selection
//!
//! The corpus is `EXPECT_OK`, shared with `repro_harness` rather than copied.
//! Valgrind costs 10–50x native runtime, so the profile controls how many run
//! and the count actually used is printed — a gate that silently checked four
//! of sixty would read as coverage it does not have.

#[path = "support/test_controls.rs"]
mod test_controls;
#[path = "support/repro_corpus.rs"]
mod repro_corpus;

use repro_corpus::EXPECT_OK;
use std::path::{Path, PathBuf};
use std::process::Command;
use test_controls::{evenly_selected, profile_name, profile_usize, reduced_usize_control, test_profile};

const FULL_CASES: usize = 24;
const BALANCED_CASES: usize = 8;
const SMOKE_CASES: usize = 3;

/// Valgrind's exit code when it finds an error, chosen not to collide with
/// any exit code the corpus expects.
const VALGRIND_ERROR_EXIT: i32 = 99;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("repro")
        .join(format!("{}.cnb", name))
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("cinnabar_sanitizer_{}", std::process::id()))
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(err) => eprintln!("sanitizer temp cleanup failed: {}", err),
        }
    }
}

/// Builds `fixture` through the instrumented link mode.
fn build_instrumented(cinnabar: &str, fixture: &Path, out: &Path) -> Result<(), String> {
    let output = Command::new(cinnabar)
        .arg(fixture)
        .arg("--instrumented")
        .arg("-o")
        .arg(out)
        .output()
        .map_err(|err| format!("cannot run the compiler on {}: {}", fixture.display(), err))?;
    if output.status.success() {
        return Ok(());
    }
    // Both streams: the compiler renders diagnostics through ariadne, and a
    // failure reported on the stream this did not read would arrive as an
    // empty explanation.
    Err(format!(
        "{} failed to build instrumented:\n{}{}",
        fixture.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

struct CheckedRun {
    exit: i32,
    report: String,
}

/// Runs `binary` under memcheck, failing the run on any error or definite
/// leak rather than only on a non-zero program exit.
fn run_under_valgrind(binary: &Path) -> Result<CheckedRun, String> {
    let output = Command::new("valgrind")
        .arg("--error-exitcode=99")
        .arg("--leak-check=full")
        .arg("--errors-for-leak-kinds=definite")
        .arg("--track-origins=yes")
        .arg(binary)
        // The same reasoning as the repro harness: a fixture must not read
        // the suite's own standard input, and its output is not what this
        // gate is about.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .output()
        .map_err(|err| format!("cannot run valgrind on {}: {}", binary.display(), err))?;
    let exit = match output.status.code() {
        Some(code) => code,
        None => return Err(format!("{} terminated without an exit status", binary.display())),
    };
    Ok(CheckedRun { exit, report: String::from_utf8_lossy(&output.stderr).to_string() })
}

/// The gate depends on valgrind, which the dev shell provides. A missing
/// checker is reported as a failure rather than skipped: a gate that quietly
/// passes when its checker is absent is worse than no gate, because it reads
/// as evidence.
fn require_valgrind() {
    let found = Command::new("valgrind").arg("--version").output();
    match found {
        Ok(output) => assert!(
            output.status.success(),
            "valgrind is present but did not run: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(err) => assert!(
            false,
            "valgrind is required by the sanitizer gate and was not found ({}). \
             The dev shell provides it; run the suite through `nix develop`.",
            err
        ),
    }
}

#[test]
fn every_selected_fixture_is_clean_under_memcheck() {
    require_valgrind();
    let cinnabar = env!("CARGO_BIN_EXE_cinnabar");
    let profile = test_profile();
    let budget = reduced_usize_control(
        profile,
        "CINNABAR_SANITIZER_CASES",
        profile_usize(profile, FULL_CASES, BALANCED_CASES, SMOKE_CASES),
    );
    assert!(
        budget <= EXPECT_OK.len(),
        "CINNABAR_SANITIZER_CASES ({}) cannot exceed the {} expected-success fixtures",
        budget,
        EXPECT_OK.len()
    );
    eprintln!(
        "sanitizer profile: {} ({} of {} expected-success fixtures under memcheck)",
        profile_name(profile),
        budget,
        EXPECT_OK.len()
    );

    let dir = temp_dir();
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());

    let mut checked = 0usize;
    let mut idx = 0usize;
    while idx < EXPECT_OK.len() {
        let (name, want) = match EXPECT_OK.get(idx) {
            Some(pair) => *pair,
            None => break,
        };
        if !evenly_selected(idx, EXPECT_OK.len(), budget) {
            idx += 1;
            continue;
        }
        let binary = dir.join(format!("{}_instrumented", name));
        match build_instrumented(cinnabar, &fixture_path(name), &binary) {
            Ok(()) => {}
            Err(message) => {
                assert!(false, "{}", message);
                return;
            }
        }
        let run = match run_under_valgrind(&binary) {
            Ok(run) => run,
            Err(message) => {
                assert!(false, "{}", message);
                return;
            }
        };
        assert!(
            run.exit != VALGRIND_ERROR_EXIT,
            "{} is not clean under memcheck:\n{}",
            name,
            run.report
        );
        // The instrumented binary must still be the same program. A link
        // mode that changed what a fixture computes would make every clean
        // report above meaningless.
        assert_eq!(
            run.exit, want,
            "{} exited {} under memcheck, want {} — the instrumented link \
             changed the program's behaviour:\n{}",
            name, run.exit, want, run.report
        );
        checked += 1;
        idx += 1;
    }

    assert_eq!(checked, budget, "selected {} fixtures but checked {}", budget, checked);
    drop(guard);
}

/// The gap this milestone exists to close, asserted directly.
///
/// A shipped binary defeats memcheck: with no dynamic section there is no
/// `malloc` to interpose on, so a program that allocates is reported as
/// allocating nothing. The instrumented build of the same source must report
/// real allocations. If this ever stops holding, the gate above is running
/// but seeing nothing, and every other assertion in this file is vacuous.
#[test]
fn the_instrumented_link_is_what_makes_memcheck_see_anything() {
    require_valgrind();
    let cinnabar = env!("CARGO_BIN_EXE_cinnabar");
    let dir = temp_dir().join("visibility");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());

    // A fixture that allocates through the native collections, so there is
    // something for a checker to have an opinion about.
    let source = fixture_path("vec_pop_drain");
    let instrumented = dir.join("vec_pop_drain_instrumented");
    match build_instrumented(cinnabar, &source, &instrumented) {
        Ok(()) => {}
        Err(message) => {
            assert!(false, "{}", message);
            return;
        }
    }
    let seen = match run_under_valgrind(&instrumented) {
        Ok(run) => run,
        Err(message) => {
            assert!(false, "{}", message);
            return;
        }
    };
    assert!(
        !seen.report.contains("0 allocs, 0 frees"),
        "memcheck saw no allocations in the instrumented build, so the gate \
         is measuring nothing:\n{}",
        seen.report
    );
    assert!(
        seen.report.contains("total heap usage:"),
        "memcheck produced no heap summary for the instrumented build:\n{}",
        seen.report
    );

    drop(guard);
}
