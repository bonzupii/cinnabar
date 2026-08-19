//! Every valid program in the corpus, run under a memory checker.
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
//! leaks this gate exists for.
//!
//! ## Fixture selection
//!
//! The corpus is `EXPECT_OK`, shared with `repro_harness` rather than copied,
//! plus the `STREAM_CASES` fixtures that read standard input. The full profile
//! runs every one of them, so "every valid program" is a description rather
//! than an aspiration. Valgrind costs 10-50x native runtime, so reduced
//! profiles check fewer and print how many — a gate that silently checked four
//! of sixty would read as coverage it does not have.
//!
//! **Invariants:**
//! - `LinkMode::Instrumented` is for this gate alone. The shipped link stays
//!   static and `-nostdlib`; no binary a user receives is built this way.
//! - A reduced profile prints how much it skipped. A gate that quietly
//!   narrows its own corpus reports coverage it does not have, which is
//!   worse than reporting none.
//! - No checker is claimed that nothing runs. UBSan and ASan are documented
//!   as absent, with the reason, rather than listed as intended coverage.

#[path = "support/test_controls.rs"]
mod test_controls;
#[path = "support/repro_corpus.rs"]
mod repro_corpus;
#[path = "support/stream_cases.rs"]
mod stream_cases;
#[path = "support/recursion_corpus.rs"]
mod recursion_corpus;

use repro_corpus::EXPECT_OK;
use recursion_corpus::EXPECT_RECURSION;
use stream_cases::STREAM_CASES;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use test_controls::{evenly_selected, profile_name, profile_usize, reduced_usize_control, test_profile};

/// The full profile checks the whole corpus, so "every valid program" is
/// true of it rather than nearly true. Reduced profiles trade coverage for
/// time and say how much they took.
const BALANCED_CASES: usize = 8;
const SMOKE_CASES: usize = 3;

/// Valgrind's exit code when it finds an error, chosen not to collide with
/// any exit code the corpus expects. Passed to valgrind from this constant
/// rather than written out again, so the two cannot disagree.
const VALGRIND_ERROR_EXIT: i32 = 99;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("repro")
        .join(format!("{}.cnb", name))
}

// One directory per test, never nested inside another test's. The tests in
// this file run concurrently, and a guard that removed a parent directory
// would delete a sibling's binaries mid-run.
fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cinnabar_sanitizer_{}_{}", std::process::id(), label))
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

/// Builds `fixture` through the instrumented link mode at the given
/// optimization level, linking against the default libc every fixture
/// shares.
fn build_instrumented(cinnabar: &str, fixture: &Path, out: &Path, level: &str) -> Result<(), String> {
    let output = Command::new(cinnabar)
        .arg(fixture)
        .arg("--instrumented")
        .arg("--opt-level")
        .arg(level)
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
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Runs `binary` under memcheck, failing the run on any error or definite
/// leak rather than only on a non-zero program exit.
///
/// `input` is written to the program's standard input and the descriptor
/// closed, so a fixture that reads until end of input terminates. A fixture
/// that reads nothing is given an empty stream, which is the same definite
/// state the repro harness gives it.
fn run_under_valgrind(binary: &Path, args: &[&str], input: &[u8]) -> Result<CheckedRun, String> {
    // The checker's own report goes to a file rather than to standard error.
    // Valgrind writes there by default, which would mix its output into the
    // program's and make the program's own stderr impossible to compare
    // against what the fixture promises.
    let log_path = binary.with_extension("valgrind");
    let mut child = Command::new("valgrind")
        .arg(format!("--error-exitcode={}", VALGRIND_ERROR_EXIT))
        .arg("--leak-check=full")
        .arg("--errors-for-leak-kinds=definite")
        .arg("--track-origins=yes")
        .arg(format!("--log-file={}", log_path.display()))
        .arg(binary)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot run valgrind on {}: {}", binary.display(), err))?;
    match child.stdin.take() {
        Some(mut pipe) => pipe
            .write_all(input)
            .map_err(|err| format!("cannot write standard input for {}: {}", binary.display(), err))?,
        None => return Err(format!("{} was spawned without a standard input pipe", binary.display())),
    }
    let output = child
        .wait_with_output()
        .map_err(|err| format!("cannot collect valgrind output for {}: {}", binary.display(), err))?;
    let exit = match output.status.code() {
        Some(code) => code,
        None => {
            let report = match std::fs::read_to_string(&log_path) {
                Ok(text) => text,
                Err(err) => format!("(cannot read the checker's report at {}: {})", log_path.display(), err),
            };
            return Err(format!(
                "{} terminated without an exit status; the checker's report:\n{}",
                binary.display(),
                report
            ));
        }
    };
    let report = std::fs::read_to_string(&log_path)
        .map_err(|err| format!("cannot read the checker's report at {}: {}", log_path.display(), err))?;
    Ok(CheckedRun { exit, report, stdout: output.stdout, stderr: output.stderr })
}

/// The checker objects to what it finds, rather than merely seeing it.
///
/// The visibility test below proves memcheck can *see* the instrumented
/// build's allocations. On its own that is not enough: drop `--leak-check`,
/// or the leak-kinds selection, and every fixture still allocates, still
/// exits with the code it should, and the whole gate stays green while
/// reporting nothing. The two together are what make a clean run evidence.
///
/// The leaking program is C rather than Cinnabar because a leak is a compile
/// error in Cinnabar — linear handles are consumed exactly once on every
/// path. That is the language working, and no help at all in testing whether
/// the checker would notice.
#[test]
fn the_checker_fails_a_program_that_leaks() {
    require_valgrind();
    let dir = temp_dir("positive_control");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());

    // Exits zero, so a non-zero result can only have come from the checker.
    let source = dir.join("leak.c");
    match std::fs::write(&source, "#include <stdlib.h>\nint main(void){ char *p = malloc(64); p[0] = 1; return p[0] - 1; }\n") {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot write the leaking source: {}", err);
            return;
        }
    }
    let binary = dir.join("leak");
    let built = Command::new("clang").arg("-o").arg(&binary).arg(&source).output();
    match built {
        Ok(output) => assert!(
            output.status.success(),
            "cannot build the leaking control: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(err) => {
            assert!(false, "cannot run clang: {}", err);
            return;
        }
    }
    let run = match run_under_valgrind(&binary, &[], b"") {
        Ok(run) => run,
        Err(message) => {
            assert!(false, "{}", message);
            return;
        }
    };
    assert_eq!(
        run.exit, VALGRIND_ERROR_EXIT,
        "a program leaking 64 bytes exited {} rather than the checker's error code — \
         memcheck is running but not objecting, so a clean report from this gate \
         means nothing:\n{}",
        run.exit, run.report
    );
    assert!(
        run.report.contains("definitely lost"),
        "the checker failed the run without reporting the leak that caused it:\n{}",
        run.report
    );

    drop(guard);
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
        profile_usize(profile, EXPECT_OK.len(), BALANCED_CASES, SMOKE_CASES),
    );
    assert!(
        budget <= EXPECT_OK.len(),
        "CINNABAR_SANITIZER_CASES ({}) cannot exceed the {} expected-success fixtures",
        budget,
        EXPECT_OK.len()
    );
    let levels = ["0", "1", "2", "3"];
    eprintln!(
        "sanitizer profile: {} ({} of {} expected-success fixtures, {} opt levels, under memcheck)",
        profile_name(profile),
        budget,
        EXPECT_OK.len(),
        levels.len()
    );

    let dir = temp_dir("corpus");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());
    let mut checked = 0usize;
    let mut lidx = 0usize;
    while lidx < levels.len() {
        let level = match levels.get(lidx) {
            Some(level) => *level,
            None => break,
        };
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
            let binary = dir.join(format!("{}_o{}_instrumented", name, level));
            match build_instrumented(cinnabar, &fixture_path(name), &binary, level) {
                Ok(()) => {}
                Err(message) => {
                    assert!(false, "{}", message);
                    return;
                }
            }
            let run = match run_under_valgrind(&binary, &[], b"") {
                Ok(run) => run,
                Err(message) => {
                    assert!(false, "{}", message);
                    return;
                }
            };
            assert!(
                run.exit != VALGRIND_ERROR_EXIT,
                "{} at -O{} is not clean under memcheck:\n{}",
                name, level, run.report
            );
            // The instrumented binary must still be the same program. A link
            // mode that changed what a fixture computes would make every clean
            // report above meaningless.
            assert_eq!(
                run.exit, want,
                "{} at -O{} exited {} under memcheck, want {} — the instrumented link \
                 changed the program's behaviour:\n{}",
                name, level, run.exit, want, run.report
            );
            checked += 1;
            idx += 1;
        }
        lidx += 1;
    }

    assert_eq!(
        checked,
        budget * levels.len(),
        "selected {} fixtures across {} levels but checked {}",
        budget,
        levels.len(),
        checked
    );
    drop(guard);
}

/// The fixtures that read standard input, checked too.
///
/// These cannot join the corpus above: `EXPECT_OK` is run with an empty
/// standard input, and a fixture that expects a line would take its
/// end-of-input path and exit non-zero. Left out entirely they would be the
/// one gap that matters most — `terminal_lines` is a `String`
/// allocate-and-free workout over `read_line`'s success path, and
/// `terminal_invalid_utf8` exercises the path that frees a line's buffer
/// after rejecting it. A leak in either is exactly what this gate exists to
/// catch, and neither would have been run at all.
#[test]
fn the_stdin_fixtures_are_clean_under_memcheck() {
    require_valgrind();
    let cinnabar = env!("CARGO_BIN_EXE_cinnabar");
    let dir = temp_dir("streams");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());

    let levels = ["0", "1", "2", "3"];
    let mut lidx = 0usize;
    while lidx < levels.len() {
        let level = match levels.get(lidx) {
            Some(level) => *level,
            None => break,
        };
        let mut idx = 0usize;
        while idx < STREAM_CASES.len() {
            let case = match STREAM_CASES.get(idx) {
                Some(case) => case,
                None => break,
            };
            let binary = dir.join(format!("{}_o{}_instrumented", case.name, level));
            match build_instrumented(cinnabar, &fixture_path(case.name), &binary, level) {
                Ok(()) => {}
                Err(message) => {
                    assert!(false, "{}", message);
                    return;
                }
            }
            let run = match run_under_valgrind(&binary, case.args, case.stdin) {
                Ok(run) => run,
                Err(message) => {
                    assert!(false, "{}", message);
                    return;
                }
            };
            assert!(
                run.exit != VALGRIND_ERROR_EXIT,
                "{} at -O{} is not clean under memcheck:\n{}",
                case.name, level, run.report
            );
            assert_eq!(
                run.exit, case.exit,
                "{} at -O{} exited {} under memcheck, want {} — the instrumented link \
                 changed the program's behaviour:\n{}",
                case.name, level, run.exit, case.exit, run.report
            );
            // The instrumented binary must be the same program byte for byte on
            // both descriptors, not merely one that exits the same way. This is
            // where the link mode would show if it had changed anything, and it
            // is the only place the two builds are compared on output at all.
            assert!(
                run.stdout == case.stdout,
                "{} at -O{}: instrumented standard output differs from the shipped \
                 contract\n  got:  {:?}\n  want: {:?}",
                case.name,
                level,
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(case.stdout)
            );
            assert!(
                run.stderr == case.stderr,
                "{} at -O{}: instrumented standard error differs from the shipped \
                 contract\n  got:  {:?}\n  want: {:?}",
                case.name,
                level,
                String::from_utf8_lossy(&run.stderr),
                String::from_utf8_lossy(case.stderr)
            );
            idx += 1;
        }
        lidx += 1;
    }

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
    let dir = temp_dir("visibility");
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
    let levels = ["0", "1", "2", "3"];
    let mut lidx = 0usize;
    while lidx < levels.len() {
        let level = match levels.get(lidx) {
            Some(level) => *level,
            None => break,
        };
        let instrumented = dir.join(format!("vec_pop_drain_o{}_instrumented", level));
        match build_instrumented(cinnabar, &source, &instrumented, level) {
            Ok(()) => {}
            Err(message) => {
                assert!(false, "{}", message);
                return;
            }
        }
        let seen = match run_under_valgrind(&instrumented, &[], b"") {
            Ok(run) => run,
            Err(message) => {
                assert!(false, "{}", message);
                return;
            }
        };
        assert!(
            !seen.report.contains("0 allocs, 0 frees"),
            "memcheck saw no allocations in the -O{} instrumented build, so the gate \
             is measuring nothing:\nexit={} stdout={:?} stderr={:?}\n{}",
            level,
            seen.exit,
            String::from_utf8_lossy(&seen.stdout),
            String::from_utf8_lossy(&seen.stderr),
            seen.report
        );
        assert!(
            seen.report.contains("total heap usage:"),
            "memcheck produced no heap summary for the -O{} instrumented build:\n{}",
            level,
            seen.report
        );
        lidx += 1;
    }

    drop(guard);
}

/// The O(1) call-stack guarantee holds under memcheck at every
/// optimization level: each self-tail-recursive fixture must exit with
/// its expected code, clean, at `-O0` through `-O3`.  A regression that
/// let recursion fall back to real calls would die on the 1M-deep or
/// 500k-deep fixture here, under the checker, instead of at whichever
/// level the corpus happened to run.
#[test]
fn recursion_fixtures_are_clean_under_memcheck_at_every_opt_level() {
    require_valgrind();
    let cinnabar = env!("CARGO_BIN_EXE_cinnabar");
    let dir = temp_dir("recursion_tiers");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());
    let levels = ["0", "1", "2", "3"];
    let mut lidx = 0usize;
    while lidx < levels.len() {
        let level = match levels.get(lidx) {
            Some(level) => *level,
            None => break,
        };
        let mut fidx = 0usize;
        while fidx < EXPECT_RECURSION.len() {
            let (name, want) = match EXPECT_RECURSION.get(fidx) {
                Some(pair) => *pair,
                None => break,
            };
            let binary = dir.join(format!("{}_o{}_instrumented", name, level));
            match build_instrumented(cinnabar, &fixture_path(name), &binary, level) {
                Ok(()) => {}
                Err(message) => {
                    assert!(false, "{}", message);
                    return;
                }
            }
            let run = match run_under_valgrind(&binary, &[], b"") {
                Ok(run) => run,
                Err(message) => {
                    assert!(false, "{}", message);
                    return;
                }
            };
            assert!(
                run.exit != VALGRIND_ERROR_EXIT,
                "{} at -O{} is not clean under memcheck:\n{}",
                name, level, run.report
            );
            assert_eq!(
                run.exit, want,
                "{} at -O{} exited {} under memcheck, want {}\n{}",
                name, level, run.exit, want, run.report
            );
            fidx += 1;
        }
        lidx += 1;
    }
    drop(guard);
}
