//! Memory access widths and handle integrity, asserted against the IR.
//!
//! Shipped binaries are linked `-static -nostdlib -no-pie`, so an external
//! memory checker has no allocation to interpose on; these properties are
//! pinned in the emitted IR instead, plus one behavioural fixture
//! (`mem_byte_access.cnb`) that detects a wide access as a clobbered
//! neighbouring byte. Covered:
//!
//! 1. `Memory.write_u8` stores exactly `i8` to the byte it computed, and
//!    `Memory.read_u8` loads exactly `i8` from it.
//! 2. Every native-handle constructor lowers its handle to the layout kind
//!    the registry declares (scalar `i64`, pair `{ ptr, i64 }`, triple
//!    `{ ptr, i64, i64 }`) and writes exactly that layout's slots.
//! 3. `Memory.deallocate` frees the pointer held in the handle's data field.
//! 4. `Terminal.read_line` reports a failed buffer growth instead of
//!    returning the bytes that fit.
//!
//! **Invariants:**
//! - Assertions read the emitted IR, covering properties no runtime oracle
//!   here can observe (e.g. a never-initialized handle field read as garbage).
//! - An assertion belongs here only when a running program cannot observe
//!   the property; anything runtime-detectable is pinned as a fixture instead.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("repro")
        .join(format!("{}.cnb", name))
}

// Per-test directory: concurrent tests must not share IR output.
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
/// `marker` (unsuffixed; emitted names carry a symbol-id suffix).
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

/// The SSA name defined by the first instruction whose RHS starts with `rhs`
/// (`%22 = getelementptr ...` yields `%22` for prefix `getelementptr`).
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

/// The lowered layout of each native handle kind.
const SCALAR_HANDLE_TY: &str = "i64";
const PAIR_HANDLE_TY: &str = "{ ptr, i64 }";
const TRIPLE_HANDLE_TY: &str = "{ ptr, i64, i64 }";

/// Finds the handle local a constructor allocates and asserts its lowered type.
fn handle_alloc_of(body: &str, what: &str, name: &str, ty: &str) -> String {
    let alloc = format!("alloca {}", ty);
    for line in body.lines() {
        if line.contains(&alloc) && line.contains(name) {
            if let Some((lhs, _rhs)) = line.split_once(" = ") {
                return lhs.trim().to_string();
            }
        }
    }
    assert!(
        false,
        "{} does not allocate its `{}` handle as {}:\n{}",
        what,
        name,
        ty,
        body
    );
    String::new()
}

/// Asserts a constructor stores through exactly `gep_count` slots of the
/// handle (0 for a scalar handle: one direct store).
fn assert_slots_written(body: &str, handle: &str, what: &str, gep_count: usize) {
    if gep_count == 0 {
        let store = format!(", ptr {},", handle);
        for line in body.lines() {
            if line.starts_with("store ") && line.contains(&store) {
                return;
            }
        }
        assert!(
            false,
            "{} does not store its scalar handle {}:\n{}",
            what,
            handle,
            body
        );
        return;
    }
    let gep = format!("ptr {},", handle);
    let mut count = 0usize;
    for line in body.lines() {
        if line.contains("getelementptr") && line.contains(&gep) {
            count += 1;
        }
    }
    assert!(
        count == gep_count,
        "{} writes {} slot(s) of handle {} (expected {}):\n{}",
        what,
        count,
        handle,
        gep_count,
        body
    );
}

/// The lowered layout every native handle shares (`native_llvm` in
/// `src/codegen/types.rs`): data pointer, length, capacity.
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

    // `write_u8` computes the target byte with a `getelementptr i8`; the
    // store landing there must be one byte wide.
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

    // The load through the same address computation must be one byte wide.
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
fn native_handle_constructors_write_their_declared_layout() {
    let dir = temp_dir("handle_layout");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());

    // Pair handles (data pointer + length): both slots written, no capacity slot.
    let mem_ir = emit_ir(&dir, "mem_byte_access");
    let mem_body = function_body(&mem_ir, "@Memory_allocate");
    let block = handle_alloc_of(&mem_body, "Memory.allocate", "block", PAIR_HANDLE_TY);
    assert_slots_written(&mem_body, &block, "Memory.allocate", 2);

    let str_ir = emit_ir(&dir, "utf8_validation");
    let str_body = function_body(&str_ir, "@Collections_string_from_slice");
    let string = handle_alloc_of(&str_body, "Collections.string_from_slice", "str", PAIR_HANDLE_TY);
    assert_slots_written(&str_body, &string, "Collections.string_from_slice", 2);

    // Triple handles (data pointer + length + capacity): all three written.
    let vec_ir = emit_ir(&dir, "vec_pop_drain");
    let vec_body = function_body(&vec_ir, "@Collections_vec_new");
    let vec = handle_alloc_of(&vec_body, "Collections.vec_new", "vec", TRIPLE_HANDLE_TY);
    assert_slots_written(&vec_body, &vec, "Collections.vec_new", 3);

    let map_ir = emit_ir(&dir, "hash_map_remove_drain");
    let map_body = function_body(&map_ir, "@Collections_hash_map_new");
    let map = handle_alloc_of(&map_body, "Collections.hash_map_new", "map", TRIPLE_HANDLE_TY);
    assert_slots_written(&map_body, &map, "Collections.hash_map_new", 3);

    // Scalar handles: one direct store of the bare descriptor integer, no struct.
    let file_ir = emit_ir(&dir, "file_roundtrip");
    let file_body = function_body(&file_ir, "@File_open");
    let file = handle_alloc_of(&file_body, "File.open", "file", SCALAR_HANDLE_TY);
    assert_slots_written(&file_body, &file, "File.open", 0);

    let net_ir = emit_ir(&dir, "net_primitives");
    let net_body = function_body(&net_ir, "@Net_socket");
    let sock = handle_alloc_of(&net_body, "Net.socket", "sock", SCALAR_HANDLE_TY);
    assert_slots_written(&net_body, &sock, "Net.socket", 0);

    let proc_ir = emit_ir(&dir, "process_spawn_wait");
    let proc_body = function_body(&proc_ir, "@Process_spawn");
    let child = handle_alloc_of(&proc_body, "Process.spawn", "child", SCALAR_HANDLE_TY);
    assert_slots_written(&proc_body, &child, "Process.spawn", 0);

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

    // `munmap` needs both handle halves: address from field 0, length from
    // field 1. The assertion is on that data flow, not a syscall number.
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

    // The loaded pointer reaches `@munmap` directly, no `ptrtoint`.
    let unmap = match body.lines().find(|line| line.contains("@munmap")) {
        Some(line) => line.to_string(),
        None => {
            assert!(false, "Memory.deallocate issues no munmap call: {}", body);
            String::new()
        }
    };
    assert!(
        unmap.contains(&format!("ptr {}", address_ssa)),
        "Memory.deallocate's munmap does not receive the handle's address: {}",
        unmap
    );
    assert!(
        unmap.contains(&format!("i64 {}", length_ssa)),
        "Memory.deallocate's munmap does not receive the handle's length: {}",
        unmap
    );

    drop(guard);
}

/// The instructions of the basic block labelled `label`, up to the next
/// label (a line whose first token ends in a colon).
fn basic_block(body: &str, label: &str) -> String {
    let opener = format!("{}:", label);
    let mut block = String::new();
    let mut inside = false;
    for line in body.lines() {
        let starts_block = match line.split_whitespace().next() {
            Some(token) => token.ends_with(':'),
            None => false,
        };
        if !inside {
            if starts_block && line.starts_with(&opener) {
                inside = true;
            }
            continue;
        }
        if starts_block {
            return block;
        }
        block.push_str(line);
        block.push('\n');
    }
    block
}

/// `Terminal.read_line` reports a failed buffer growth instead of
/// returning a short line.
#[test]
fn read_line_reports_a_failed_growth_rather_than_a_short_line() {
    let dir = temp_dir("read_line_growth");
    match std::fs::create_dir_all(&dir) {
        Ok(()) => {}
        Err(err) => {
            assert!(false, "cannot create temp dir: {}", err);
            return;
        }
    }
    let guard = TempDirGuard(dir.clone());
    let ir = emit_ir(&dir, "runtime_io");
    let body = function_body(&ir, "Terminal_read_line");
    assert!(!body.is_empty(), "runtime_io must emit Terminal.read_line");

    // Reachability first: nothing may fall through the failure block.
    let growth = basic_block(&body, "line_grow");
    assert!(
        growth.contains("label %line_grow_fail"),
        "a null from realloc must branch to the failure path, but the growth \
         block ends: {}",
        growth
    );

    let failure = basic_block(&body, "line_grow_fail");
    assert!(
        !failure.is_empty(),
        "read_line gives a failed growth no block of its own, so it falls into \
         the finish path and returns a silently truncated line as a success"
    );
    assert!(
        failure.contains("@free"),
        "the failed-growth path must release the buffer realloc left valid, \
         but its block is: {}",
        failure
    );
    assert!(
        !failure.contains("line_value") && !failure.contains("line_finish"),
        "the failed-growth path must not reach the line-value construction, \
         but its block branches there: {}",
        failure
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
        if value.starts_with(&format!("getelementptr inbounds nuw {}, ptr", PAIR_HANDLE_TY)) && value.ends_with(&suffix) {
            return name.to_string();
        }
    }
    String::new()
}
