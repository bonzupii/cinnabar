//! Every valid program in the corpus, run under a memory checker.
//!
//! Builds each corpus binary through `LinkMode::Instrumented` — the same
//! object files linked dynamically against the host libc so an interposing
//! checker sees real allocations — then runs it under Valgrind memcheck and
//! fails on any invalid read or write, uninitialised value use, invalid
//! free, or definite leak. UBSan and ASan are not run: their checks are not
//! expressible as a pass over Cinnabar-emitted IR in this pipeline.
//!
//! The corpus is `EXPECT_OK`, shared with `repro_harness`, plus the
//! `STREAM_CASES` fixtures that read standard input. The full profile runs
//! every one; reduced profiles run an even sample and print how many ran.
//!
//! **Invariants:**
//! - `LinkMode::Instrumented` is for this gate alone; the shipped link stays
//!   static and `-nostdlib`.
//! - A reduced profile prints how many fixtures it skipped.
//! - No checker is claimed that nothing runs.

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

/// Corpus sizes for the balanced and smoke profiles; reduced profiles
/// print how many they ran.
const BALANCED_CASES: usize = 8;
const SMOKE_CASES: usize = 3;

/// Valgrind's error exit code, distinct from every exit code the corpus uses.
const VALGRIND_ERROR_EXIT: i32 = 99;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("repro")
        .join(format!("{}.cnb", name))
}

// Directories are never nested: concurrent tests would delete siblings' binaries.
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
/// optimization level, against the shared default libc.
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
    // Read both streams: diagnostics may land on either.
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

/// Runs `binary` under memcheck, failing on any error or definite leak;
/// `input` is written to stdin and the descriptor closed.
fn run_under_valgrind(binary: &Path, args: &[&str], input: &[u8]) -> Result<CheckedRun, String> {
    // The report goes to a log file so valgrind output never mixes with
    // the program's own stderr.
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

/// Positive control: a deliberately leaking C program that exits zero, so
/// only memcheck can fail it.
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

/// Fails unless valgrind is present; the dev shell provides it.
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
            // The instrumented binary must compute the same result.
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

/// The stdin-reading fixtures, checked too: `EXPECT_OK` runs with an empty
/// stdin, so these need their own inputs to reach their allocation paths.
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
            // Instrumented stdout/stderr must match the shipped contract byte for byte.
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

/// Asserts the instrumented link is what makes memcheck see allocations:
/// a shipped binary has no dynamic section, so memcheck interposes nothing.
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

/// Each self-tail-recursive fixture must exit with its expected code, clean,
/// under memcheck at `-O0` through `-O3`.
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
