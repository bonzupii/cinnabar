//! Milestone 3 — native memory access widths and handle integrity,
//! asserted against the emitted LLVM IR.
//!
//! An external memory checker cannot cover this. A Cinnabar binary is
//! linked `-static -nostdlib -no-pie` against a musl `libc.a` embedded in
//! the compiler, so it carries no dynamic section; Valgrind's memcheck has
//! nothing to interpose on and reports `0 allocs, 0 frees` for a program
//! that demonstrably allocates, at every optimization level. ASan is no
//! better placed: its runtime wants the libc the link deliberately does
//! not provide.
//!
//! So the two properties Milestone 3 is about are pinned where they are
//! stated literally — in the IR — and behaviourally, by
//! `tests/fixtures/repro/mem_byte_access.cnb`, which detects a wide access
//! as a clobbered neighbouring byte. The IR assertions here catch what the
//! runtime oracle structurally cannot: a handle field that is never
//! initialized is read as garbage rather than producing a wrong value, so
//! no in-language check can observe it.
//!
//! 1. `Memory.write_u8` stores exactly `i8` to the byte it computed, and
//!    `Memory.read_u8` loads exactly `i8` from it. A wider access is the
//!    reported "overreads the allocation by 7 bytes" defect.
//! 2. Every native-handle constructor zero-fills the handle across its
//!    whole lowered layout before storing any field into it. Handles are
//!    moved and returned *by value* (`deallocate` takes a `Block` as
//!    `{ ptr, i64, i64 }`), so a field a constructor skipped is read as
//!    uninitialized stack at the first move — the shape of the reported
//!    "`deallocate` frees a garbage pointer" defect.
//! 3. `Memory.deallocate` frees the pointer held in the handle's data
//!    field, not some other field.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("repro")
        .join(format!("{}.cnb", name))
}

// One directory per test: the tests in this file run concurrently, so a
// shared per-process directory would let the first one to finish delete
// the IR the others are still reading.
fn temp_dir(test: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cinnabar_native_memory_{}_{}", std::process::id(), test))
}

// Removes the temp dir even when an assertion fails mid-run, so a failed
// iteration never leaks emitted IR into the temp filesystem.
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(err) => eprintln!("native_memory temp cleanup failed: {}", err),
        }
    }
}

fn emit_ir(dir: &Path, fixture: &str) -> String {
    let out = dir.join(format!("{}.ll", fixture));
    let status = Command::new(env!("CARGO_BIN_EXE_cinnabar"))
        .arg(fixture_path(fixture))
        .arg("--emit-llvm")
        .arg("-o")
        .arg(&out)
        .status();
    match status {
        Ok(code) => assert!(code.success(), "{} failed to emit IR ({})", fixture, code),
        Err(err) => assert!(false, "{} could not be compiled: {}", fixture, err),
    }
    match std::fs::read_to_string(&out) {
        Ok(text) => text,
        Err(err) => {
            assert!(false, "cannot read emitted IR {}: {}", out.display(), err);
            String::new()
        }
    }
}

/// The body of the first LLVM function whose `define` line mentions
/// `marker`, without the `define` line or the closing brace. Emitted
/// function names carry a symbol-id suffix (`@Memory_write_u8_3472`), so
/// the marker is the unsuffixed name.
fn function_body(ir: &str, marker: &str) -> String {
    let mut body = String::new();
    let mut inside = false;
    for line in ir.lines() {
        if !inside {
            if line.starts_with("define ") && line.contains(marker) {
                inside = true;
            }
            continue;
        }
        if line == "}" {
            return body;
        }
        body.push_str(line.trim());
        body.push('\n');
    }
    body
}

/// The SSA name defined by the first instruction whose right-hand side
/// starts with `rhs`: `%22 = getelementptr i8, ptr %5, i64 %8` yields
/// `%22` for the prefix `getelementptr i8, ptr`.
fn ssa_defined_by(body: &str, rhs: &str) -> String {
    for line in body.lines() {
        let (name, value) = match line.split_once(" = ") {
            Some(pair) => pair,
            None => continue,
        };
        if value.starts_with(rhs) {
            return name.to_string();
        }
    }
    String::new()
}

/// The `store` instruction writing to `dest`.
fn store_to(body: &str, dest: &str) -> String {
    let needle = format!(", ptr {},", dest);
    for line in body.lines() {
        if line.starts_with("store ") && line.contains(&needle) {
            return line.to_string();
        }
    }
    String::new()
}

/// The `load` instruction reading from `src`.
fn load_from(body: &str, src: &str) -> String {
    let needle = format!(", ptr {},", src);
    for line in body.lines() {
        let value = match line.split_once(" = ") {
            Some(pair) => pair.1,
            None => continue,
        };
        if value.starts_with("load ") && value.contains(&needle) {
            return value.to_string();
        }
    }
    String::new()
}

fn line_index(body: &str, needle: &str) -> Option<usize> {
    for (idx, line) in body.lines().enumerate() {
        if line.contains(needle) {
            return Some(idx);
        }
    }
    None
}

/// Asserts that the native handle a constructor builds is zero-filled
/// across its whole lowered layout before any field is stored into it.
///
/// The check is ordering-sensitive on purpose: a zero-fill emitted *after*
/// the field stores would erase them, and a zero-fill of a narrower type
/// than the handle would leave the tail uninitialized, so both the
/// aggregate type and its position relative to the first field access are
/// asserted.
fn assert_handle_zero_filled(body: &str, what: &str) {
    let fill = format!("store {} zeroinitializer, ptr ", HANDLE_TY);
    let fill_line = match line_index(body, &fill) {
        Some(idx) => idx,
        None => {
            assert!(
                false,
                "{} does not zero-fill the native handle across its whole layout \
                 (no `store {} zeroinitializer`); a field it never writes is read \
                 as uninitialized stack when the handle is moved by value",
                what, HANDLE_TY
            );
            return;
        }
    };
    let slot = match body.lines().nth(fill_line) {
        Some(line) => match line.rsplit_once(", ptr ") {
            Some((_lhs, rest)) => match rest.split_once(',') {
                Some((name, _align)) => name.to_string(),
                None => rest.to_string(),
            },
            None => String::new(),
        },
        None => String::new(),
    };
    assert!(!slot.is_empty(), "{}: cannot read the zero-filled handle slot", what);
    let field_access = format!("getelementptr inbounds nuw {}, ptr {},", HANDLE_TY, slot);
    match line_index(body, &field_access) {
        Some(idx) => assert!(
            idx > fill_line,
            "{} stores into handle {} at line {} before zero-filling it at line {}; \
             the zero-fill would erase the field",
            what,
            slot,
            idx,
            fill_line
        ),
        None => {}
    }
}

/// The lowered layout every native handle shares (`native_llvm` in
/// `src/codegen/types.rs`): data pointer, length, capacity.
const HANDLE_TY: &str = "{ ptr, i64, i64 }";

#[test]
fn memory_natives_access_exactly_one_byte() {
    let dir = temp_dir("access_width");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());
    let ir = emit_ir(&dir, "mem_byte_access");

    // `write_u8` computes the target byte with a `getelementptr i8` over
    // the block's data pointer; the store landing there must be one byte
    // wide. An `i64` store here is the reported 7-byte overrun.
    let write = function_body(&ir, "@Memory_write_u8");
    assert!(!write.is_empty(), "no Memory.write_u8 in the emitted IR");
    let write_target = ssa_defined_by(&write, "getelementptr i8, ptr");
    assert!(!write_target.is_empty(), "Memory.write_u8 computes no byte address");
    let store = store_to(&write, &write_target);
    assert!(
        store.starts_with("store i8 "),
        "Memory.write_u8 does not store exactly one byte to {}: {}",
        write_target,
        store
    );

    // `read_u8` computes its address the same way; the load must be one
    // byte wide, or a read near the end of an allocation runs off it.
    let read = function_body(&ir, "@Memory_read_u8");
    assert!(!read.is_empty(), "no Memory.read_u8 in the emitted IR");
    let read_target = ssa_defined_by(&read, "getelementptr i8, ptr");
    assert!(!read_target.is_empty(), "Memory.read_u8 computes no byte address");
    let load = load_from(&read, &read_target);
    assert!(
        load.starts_with("load i8,"),
        "Memory.read_u8 does not load exactly one byte from {}: {}",
        read_target,
        load
    );

    drop(guard);
}

#[test]
fn native_handle_constructors_initialize_every_field() {
    let dir = temp_dir("handle_init");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());

    // `Memory.allocate` uses only the data and length fields of the shared
    // handle layout, `Collections.string_from_slice` likewise; both left
    // the capacity field uninitialized before Milestone 3.
    let mem_ir = emit_ir(&dir, "mem_byte_access");
    assert_handle_zero_filled(&function_body(&mem_ir, "@Memory_allocate"), "Memory.allocate");

    let vec_ir = emit_ir(&dir, "vec_pop_drain");
    assert_handle_zero_filled(
        &function_body(&vec_ir, "@Collections_vec_new"),
        "Collections.vec_new",
    );

    let map_ir = emit_ir(&dir, "hash_map_remove_drain");
    assert_handle_zero_filled(
        &function_body(&map_ir, "@Collections_hash_map_new"),
        "Collections.hash_map_new",
    );

    let str_ir = emit_ir(&dir, "utf8_validation");
    assert_handle_zero_filled(
        &function_body(&str_ir, "@Collections_string_from_slice"),
        "Collections.string_from_slice",
    );

    drop(guard);
}

#[test]
fn deallocate_unmaps_the_handles_address_and_length() {
    let dir = temp_dir("deallocate");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());
    let ir = emit_ir(&dir, "mem_byte_access");

    // `Memory.deallocate` unmaps the mapping `allocate` made, and `munmap`
    // needs *both* halves of the handle: the address from field 0 and the
    // length from field 1. Releasing a pointer from any other field is the
    // reported garbage-pointer free, and passing a length from anywhere but
    // the handle would unmap memory the program still owns — which is the
    // second reason a handle must be fully initialized.
    //
    // The assertion is on the data flow, not on a syscall number, so it
    // holds on every architecture. The numbers themselves are checked
    // independently by the unit tests in `src/codegen/syscall.rs`, against
    // hand-written Linux ABI values.
    let body = function_body(&ir, "@Memory_deallocate");
    assert!(!body.is_empty(), "no Memory.deallocate in the emitted IR");

    let data_field = handle_field(&body, 0);
    assert!(!data_field.is_empty(), "Memory.deallocate does not read the handle's data field");
    let address = load_from(&body, &data_field);
    assert!(
        address.starts_with("load ptr,"),
        "Memory.deallocate does not load a pointer from the handle's data field: {}",
        address
    );
    let address_ssa = ssa_defined_by(&body, &format!("load ptr, ptr {},", data_field));

    let length_field = handle_field(&body, 1);
    assert!(!length_field.is_empty(), "Memory.deallocate does not read the handle's length field");
    let length_ssa = ssa_defined_by(&body, &format!("load i64, ptr {},", length_field));
    assert!(!length_ssa.is_empty(), "Memory.deallocate does not load the handle's length");

    // The address reaches the kernel as an integer, so it passes through a
    // `ptrtoint` of exactly the pointer loaded from the data field.
    let address_word = ssa_defined_by(&body, &format!("ptrtoint ptr {} to i64", address_ssa));
    assert!(
        !address_word.is_empty(),
        "Memory.deallocate does not pass the handle's data pointer to the system call"
    );

    let unmap = match body.lines().find(|line| line.contains("asm sideeffect")) {
        Some(line) => line.to_string(),
        None => {
            assert!(false, "Memory.deallocate issues no system call: {}", body);
            String::new()
        }
    };
    assert!(
        unmap.contains(&format!("i64 {}", address_word)),
        "Memory.deallocate's system call does not receive the handle's address: {}",
        unmap
    );
    assert!(
        unmap.contains(&format!("i64 {}", length_ssa)),
        "Memory.deallocate's system call does not receive the handle's length: {}",
        unmap
    );

    drop(guard);
}

/// The SSA name of the `getelementptr` selecting field `index` of a native
/// handle.
fn handle_field(body: &str, index: u32) -> String {
    let suffix = format!("i32 0, i32 {}", index);
    for line in body.lines() {
        let (name, value) = match line.split_once(" = ") {
            Some(pair) => pair,
            None => continue,
        };
        if value.starts_with(&format!("getelementptr inbounds nuw {}, ptr", HANDLE_TY)) && value.ends_with(&suffix) {
            return name.to_string();
        }
    }
    String::new()
}
