use std::path::{Path, PathBuf};
use std::process::Command;

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
    ("mem_probe", 70),
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
    ("multiline_const", 30),
    ("fib", 155),
    ("linear_field_reinit", 0),
    ("linear_ref_swap", 0),
    ("linear_field_consume", 0),
    ("linear_ref_nonlinear_read", 14),
    ("ret_borrow_shared_twice", 0),
    ("ret_borrow_single_origin", 0),
    ("slice_test", 0),
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
    "idx10j2_dyn_dyn_match",
    "idx10f_element_move_while_borrowed",
    "idx10g_element_double_move",
    "b3_two_mut",
    "b4_mut_shared",
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

fn compile(cinnabar: &str, fixture: &Path, bin: &Path) -> i32 {
    exit_code(Command::new(cinnabar).arg(fixture).arg("-o").arg(bin))
}

const RUN_TIMEOUT_SECS: u64 = 10;

const TIMEOUT_CODE: i32 = 124;

fn run_binary(bin: &Path) -> i32 {
    let mut child = match Command::new(bin)
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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(RUN_TIMEOUT_SECS);
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

#[test]
fn repro_corpus_baseline() {
    let cinnabar = env!("CARGO_BIN_EXE_cinnabar");
    let dir = temp_dir();
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("cannot create temp dir: {}", err);
            return;
        }
    }

    let mut idx = 0usize;
    while idx < EXPECT_OK.len() {
        let (name, want) = match EXPECT_OK.get(idx) {
            Some(pair) => *pair,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", name));
        let compile_code = compile(cinnabar, &fixture_path(name), &bin);
        assert_eq!(compile_code, 0, "{} failed to compile (code {})", name, compile_code);
        let run_code = run_binary(&bin);
        assert_eq!(run_code, want, "{} ran with exit {} (want {})", name, run_code, want);
        idx += 1;
    }

    let mut ridx = 0usize;
    while ridx < EXPECT_REJECTED.len() {
        let name = match EXPECT_REJECTED.get(ridx) {
            Some(name) => *name,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", name));
        let compile_code = compile(cinnabar, &fixture_path(name), &bin);
        assert!(compile_code != 0, "{} was unexpectedly accepted", name);
        ridx += 1;
    }

    let mut oidx = 0usize;
    while oidx < RECORD_ONLY.len() {
        let name = match RECORD_ONLY.get(oidx) {
            Some(name) => *name,
            None => break,
        };
        let bin = dir.join(format!("{}_bin", name));
        let compile_code = compile(cinnabar, &fixture_path(name), &bin);
        if compile_code == 0 {
            let run_code = run_binary(&bin);
            println!("RECORD {}: compile=OK run={}", name, run_code);
        } else {
            println!("RECORD {}: compile=FAIL({})", name, compile_code);
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
        let compile_code = compile(cinnabar, &fixture_rel_path(rel), &bin);
        assert_eq!(compile_code, 0, "{} failed to compile (code {})", name, compile_code);
        cidx += 1;
    }

    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => eprintln!("temp cleanup failed: {}", err),
    }
}
