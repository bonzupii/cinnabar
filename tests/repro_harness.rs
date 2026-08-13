#[path = "support/test_controls.rs"]
mod test_controls;

use std::path::{Path, PathBuf};
use std::process::Command;
use test_controls::{
    evenly_selected, profile_name, profile_usize, reduced_usize_control, test_profile,
};

const EXPECT_OK: &[(&str, i32)] = &[
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

const EXPECT_REJECTED: &[&str] = &[
    "index_oob_const",
    "rt2",
    "div_zero_const",
    "mod_zero_const",
    "assign_shared_ref",
    "linear_field_reassign",
    "linear_field_dup",
    "linear_struct_dead_end",
    "linear_field_dup_extract",
    "linear_ref_no_restore",
    "linear_ref_no_restore_falloff",
    "linear_ref_untracked",
    "ret_borrow_ambiguous",
    "ret_borrow_sole_input",
    "ret_borrow_uaf",
    "duplicate_builtin_unit",
    "duplicate_builtin_int",
    "duplicate_user_symbol",
    "idx10b_mut_alias_used",
    "idx10c_mut_shared_same",
    "vec_push_linear_move",
    "idx10j2_dyn_dyn_match",
    "idx10f_element_move_while_borrowed",
    "idx10g_element_double_move",
    "b3_two_mut",
    "b4_mut_shared",
    "int_literal_range",
    "int_literal_no_peer",
    "string_bad_escape",
    "string_not_an_int",
    "file_unclosed",
    "borrow_after_move",
    "int_unsigned_neg",
    "non_tail_recursion",
    "vec_no_extraction",
    "vec_undrained_free",
    "vec_pop_unconsumed",
    "hash_map_undrained_free",
    "hash_map_linear_key_undrained_free",
    "unresolved_call_cascade",
    "non_struct_field_cascade",
    "undeclared_const_cascade",
    "const_div_zero_cascade",
    "malformed_type_cascade",
];

const RECORD_ONLY: &[&str] = &[
    "full_rt",
    "mem_test",
    "rt1",
    "vec_test",
    "vm5",
    "vm10",
];

// Compile-only fixtures: the binary must build, but is never executed.
// http_server.cnb is a blocking network server loop, so running it would
// hang the harness; compiling it proves the Net native surface lowers and
// links (per the zero-execution rule for that fixture).
const EXPECT_COMPILE: &[(&str, &str)] = &[("http_server", "tests/fixtures/http_server.cnb")];

fn fixture_path(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    root.join("tests")
        .join("fixtures")
        .join("repro")
        .join(format!("{}.cnb", name))
}

fn fixture_rel_path(rel: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    root.join(rel)
}

fn exit_code(cmd: &mut Command) -> i32 {
    match cmd.status() {
        Ok(status) => match status.code() {
            Some(code) => code,
            None => 139,
        },
        Err(err) => {
            eprintln!("spawn failed: {}", err);
            139
        }
    }
}

fn compile_and_link(cinnabar: &str, fixture: &Path, bin: &Path) -> i32 {
    exit_code(Command::new(cinnabar).arg(fixture).arg("-o").arg(bin))
}

fn compile_to_llvm(cinnabar: &str, fixture: &Path, ir: &Path) -> i32 {
    exit_code(
        Command::new(cinnabar)
            .arg(fixture)
            .arg("--emit-llvm")
            .arg("-o")
            .arg(ir),
    )
}

const DEFAULT_RUN_TIMEOUT_SECS: usize = 10;
const BALANCED_RUN_CASES: usize = 10;
const BALANCED_RECORD_CASES: usize = 2;
const SMOKE_RUN_CASES: usize = 4;
const SMOKE_RECORD_CASES: usize = 0;

const TIMEOUT_CODE: i32 = 124;

struct ReproConfig {
    profile: test_controls::TestProfile,
    run_cases: usize,
    record_cases: usize,
    link_compile_only: bool,
    run_timeout_secs: u64,
}

fn bool_control(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => match value.as_str() {
            "1" | "true" | "yes" => true,
            "0" | "false" | "no" => false,
            invalid => {
                assert!(
                    false,
                    "{} must be one of 1, true, yes, 0, false, or no; got '{}'",
                    name,
                    invalid
                );
                default
            }
        },
        Err(error) => match error {
            std::env::VarError::NotPresent => default,
            std::env::VarError::NotUnicode(value) => {
                assert!(false, "{} is not Unicode: {:?}", name, value);
                default
            }
        },
    }
}

fn repro_config() -> ReproConfig {
    let profile = test_profile();
    let run_default = profile_usize(
        profile,
        EXPECT_OK.len(),
        BALANCED_RUN_CASES,
        SMOKE_RUN_CASES,
    );
    let record_default = profile_usize(
        profile,
        RECORD_ONLY.len(),
        BALANCED_RECORD_CASES,
        SMOKE_RECORD_CASES,
    );
    let run_cases =
        reduced_usize_control(profile, "CINNABAR_REPRO_RUN_CASES", run_default);
    let record_cases =
        reduced_usize_control(profile, "CINNABAR_REPRO_RECORD_CASES", record_default);
    assert!(
        run_cases <= EXPECT_OK.len(),
        "CINNABAR_REPRO_RUN_CASES ({}) cannot exceed the {} expected-success fixtures",
        run_cases,
        EXPECT_OK.len()
    );
    assert!(
        record_cases <= RECORD_ONLY.len(),
        "CINNABAR_REPRO_RECORD_CASES ({}) cannot exceed the {} record-only fixtures",
        record_cases,
        RECORD_ONLY.len()
    );
    let link_default = match profile {
        test_controls::TestProfile::Full => true,
        test_controls::TestProfile::Balanced => false,
        test_controls::TestProfile::Smoke => false,
    };
    let link_compile_only = match profile {
        test_controls::TestProfile::Full => link_default,
        test_controls::TestProfile::Balanced => {
            bool_control("CINNABAR_REPRO_LINK_COMPILE_ONLY", link_default)
        }
        test_controls::TestProfile::Smoke => {
            bool_control("CINNABAR_REPRO_LINK_COMPILE_ONLY", link_default)
        }
    };
    let run_timeout = reduced_usize_control(
        profile,
        "CINNABAR_TEST_RUN_TIMEOUT_SECS",
        DEFAULT_RUN_TIMEOUT_SECS,
    );
    assert!(run_timeout > 0, "CINNABAR_TEST_RUN_TIMEOUT_SECS must be greater than zero");
    ReproConfig {
        profile,
        run_cases,
        record_cases,
        link_compile_only,
        run_timeout_secs: run_timeout as u64,
    }
}

fn run_binary(bin: &Path, timeout_secs: u64) -> i32 {
    let mut child = match Command::new(bin)
        // A fixture must never read the harness's own standard input.
        // `Terminal.read_line` blocks until a line or end of input arrives,
        // so an inherited descriptor would make a fixture's exit code
        // depend on whether the suite was run from a terminal, a pipe, or
        // CI. A null stdin is at end of input immediately, which is a
        // definite state every run agrees on.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            eprintln!("spawn failed: {}", err);
            return 139;
        }
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(code) => code,
                    None => 139,
                };
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    match child.kill() {
                        Ok(()) => {}
                        Err(err) => {
                            if err.kind() != std::io::ErrorKind::InvalidInput {
                                eprintln!("kill after deadline failed: {}", err);
                            }
                        }
                    }
                    match child.wait() {
                        Ok(status) => {
                            return match status.code() {
                                Some(code) => code,
                                None => TIMEOUT_CODE,
                            };
                        }
                        Err(err) => {
                            eprintln!("reap failed: {}", err);
                            return TIMEOUT_CODE;
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(err) => {
                eprintln!("wait failed: {}", err);
                return 139;
            }
        }
    }
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("cinnabar_repro_{}", std::process::id()))
}

// Removes the harness temp dir even when an assertion fails mid-run: a
// failed iteration must not leak its compiled binaries (each a ~4.5 MB
// embedded-libc.a link) into the temp filesystem.
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(err) => eprintln!("repro temp cleanup failed: {}", err),
        }
    }
}

#[test]
fn repro_corpus_baseline() {
    let cinnabar = env!("CARGO_BIN_EXE_cinnabar");
    let config = repro_config();
    eprintln!(
        "repro profile: {} (link+run expected-success={}, LLVM-only expected-success={}, record-only={}, link compile-only={})",
        profile_name(config.profile),
        config.run_cases,
        EXPECT_OK.len() - config.run_cases,
        config.record_cases,
        config.link_compile_only
    );
    let dir = temp_dir();
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());

    let mut idx = 0usize;
    while idx < EXPECT_OK.len() {
        let (name, want) = match EXPECT_OK.get(idx) {
            Some(pair) => *pair,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", name));
        let ir = dir.join(format!("{}.ll", name));
        let execute = evenly_selected(idx, EXPECT_OK.len(), config.run_cases);
        let compile_code = if execute {
            compile_and_link(cinnabar, &fixture_path(name), &bin)
        } else {
            compile_to_llvm(cinnabar, &fixture_path(name), &ir)
        };
        assert_eq!(compile_code, 0, "{} failed to compile (code {})", name, compile_code);
        if execute {
            let run_code = run_binary(&bin, config.run_timeout_secs);
            assert_eq!(run_code, want, "{} ran with exit {} (want {})", name, run_code, want);
        }
        idx += 1;
    }

    let mut ridx = 0usize;
    while ridx < EXPECT_REJECTED.len() {
        let name = match EXPECT_REJECTED.get(ridx) {
            Some(name) => *name,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", name));
        let compile_code = compile_and_link(cinnabar, &fixture_path(name), &bin);
        assert!(compile_code != 0, "{} was unexpectedly accepted", name);
        ridx += 1;
    }

    let mut oidx = 0usize;
    while oidx < RECORD_ONLY.len() {
        let name = match RECORD_ONLY.get(oidx) {
            Some(name) => *name,
            None => break,
        };
        if evenly_selected(oidx, RECORD_ONLY.len(), config.record_cases) {
            let bin = dir.join(format!("{}_bin", name));
            let compile_code = compile_and_link(cinnabar, &fixture_path(name), &bin);
            if compile_code == 0 {
                let run_code = run_binary(&bin, config.run_timeout_secs);
                println!("RECORD {}: compile=OK run={}", name, run_code);
            } else {
                println!("RECORD {}: compile=FAIL({})", name, compile_code);
            }
        }
        oidx += 1;
    }

    let mut cidx = 0usize;
    while cidx < EXPECT_COMPILE.len() {
        let (name, rel) = match EXPECT_COMPILE.get(cidx) {
            Some(pair) => *pair,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", name));
        let ir = dir.join(format!("{}.ll", name));
        let compile_code = if config.link_compile_only {
            compile_and_link(cinnabar, &fixture_rel_path(rel), &bin)
        } else {
            compile_to_llvm(cinnabar, &fixture_rel_path(rel), &ir)
        };
        assert_eq!(compile_code, 0, "{} failed to compile (code {})", name, compile_code);
        cidx += 1;
    }

    drop(guard);
}
