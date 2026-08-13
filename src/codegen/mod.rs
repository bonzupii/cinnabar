pub mod emitter;
pub mod error;
pub mod layout;
pub mod syscall;
pub mod types;

use crate::codegen::emitter::{emit_program, protocol_of, InstFns, Session};
use crate::codegen::error::*;
use crate::codegen::types::{EnumInfos, KeyTypes, PayloadStructs};
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, InitializationConfig, RelocMode, Target, TargetData, TargetMachine, TargetTriple,
};
use inkwell::OptimizationLevel;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// The embedded musl static archives, staged into OUT_DIR by build.rs at
// compile time (never committed to the source tree).  Every emitted binary
// is a standalone static executable with no host libc or dynamic linker
// dependency.
const MUSL_LIBC_A: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/libc.a"));
const MUSL_CRT1_O: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/crt1.o"));
const MUSL_CRTI_O: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/crti.o"));
const MUSL_CRTN_O: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/crtn.o"));

pub fn compile_and_link(
    names: &[String],
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    impls_list: i64,
    out: &Path,
    opt_level: &str,
    entry_span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    let ir_text = emit_to_ir(names, nodes, lists, impls_list, entry_span)?;
    let temp_root = make_temp_root()?;
    let ir_path = temp_path(out, "ll");
    let obj_path = temp_path(out, "o");
    let compiled = write_text(&ir_path, &ir_text)
        .and_then(|()| assemble(&ir_path, &obj_path, opt_level))
        .and_then(|()| link(&obj_path, out));
    finish_temp(&temp_root, compiled)
}

/// Emit the program's LLVM IR and return it as text, without running `opt`,
/// `llc`, or the linker.  This is exactly the IR `compile_and_link` hands to
/// `opt` — the emitter's own output, before any optimization pass.
pub fn compile_to_ir(
    names: &[String],
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    impls_list: i64,
    entry_span: (i64, i64, i64),
) -> Result<String, CodegenError> {
    emit_to_ir(names, nodes, lists, impls_list, entry_span)
}

/// Emit, optimize, and assemble the program to a relocatable object file at
/// `out`, skipping the final static link.  Runs the same `opt`/`llc` steps as
/// `compile_and_link` at the same optimization level.
pub fn compile_to_object(
    names: &[String],
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    impls_list: i64,
    out: &Path,
    opt_level: &str,
    entry_span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    let ir_text = emit_to_ir(names, nodes, lists, impls_list, entry_span)?;
    let temp_root = make_temp_root()?;
    let ir_path = temp_path(out, "ll");
    let obj_path = temp_path(out, "o");
    let compiled = write_text(&ir_path, &ir_text)
        .and_then(|()| assemble(&ir_path, &obj_path, opt_level))
        .and_then(|()| copy_file(&obj_path, out));
    finish_temp(&temp_root, compiled)
}

fn make_temp_root() -> Result<PathBuf, CodegenError> {
    let temp_root = temp_dir_root();
    fs::create_dir_all(&temp_root).map_err(|err| {
        io_error(&format!("cannot create temp dir '{}': {}", temp_root.display(), err))
    })?;
    Ok(temp_root)
}

// The temp dir is removed on success and failure alike, so repeated or
// failing compiles never accumulate gigabytes of `libc.a` copies in
// tmpfs.  A cleanup failure is reported through the typed error model,
// never to stderr.  A compile error takes precedence over a cleanup
// error; a missing dir is not an error.
fn finish_temp(temp_root: &Path, compiled: Result<(), CodegenError>) -> Result<(), CodegenError> {
    match std::fs::remove_dir_all(temp_root) {
        Ok(()) => compiled,
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                compiled
            } else {
                match compiled {
                    Ok(()) => Err(io_error(&format!(
                        "cannot remove temp dir '{}': {}",
                        temp_root.display(),
                        err
                    ))),
                    Err(err) => Err(err),
                }
            }
        }
    }
}

fn copy_file(from: &Path, to: &Path) -> Result<(), CodegenError> {
    match fs::copy(from, to) {
        Ok(bytes_copied) => {
            if bytes_copied == 0 {
                Err(io_error(&format!("copied empty object file to '{}'", to.display())))
            } else {
                Ok(())
            }
        }
        Err(err) => Err(io_error(&format!(
            "cannot copy '{}' to '{}': {}",
            from.display(),
            to.display(),
            err
        ))),
    }
}

fn emit_to_ir(
    names: &[String],
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    impls_list: i64,
    entry_span: (i64, i64, i64),
) -> Result<String, CodegenError> {
    let context = Context::create();
    let module = context.create_module("cinnabar");
    let builder = context.create_builder();
    let (target_data, triple) = host_target()?;
    module.set_triple(&triple);
    let layout = target_data.get_data_layout();
    module.set_data_layout(&layout);
    let key_types: KeyTypes = Vec::new();
    let enum_infos: EnumInfos = Vec::new();
    let payload_structs: PayloadStructs = Vec::new();
    let inst_fns: InstFns = Vec::new();
    run_emitter(
        (&context, &module, &builder, &target_data),
        names,
        &mut *nodes,
        &mut *lists,
        (key_types, enum_infos, payload_structs, inst_fns),
        impls_list,
        entry_span,
    )?;
    verify_module(&module)?;
    Ok(module.print_to_string().to_string())
}

fn run_emitter<'ctx, 'm, 'a>(
    llvm: (&'ctx Context, &'m Module<'ctx>, &'m inkwell::builder::Builder<'ctx>, &'m TargetData),
    names: &'a [String],
    nodes: &'a mut Vec<i64>,
    lists: &'a mut Vec<Vec<i64>>,
    caches: (KeyTypes<'ctx>, EnumInfos, PayloadStructs<'ctx>, InstFns<'ctx>),
    impls_list: i64,
    entry_span: (i64, i64, i64),
) -> Result<(), CodegenError> {
    let (context, module, builder, target_data) = llvm;
    let (key_types, enum_infos, payload_structs, inst_fns) = caches;
    let protocol = protocol_of(names);
    let mut sess: Session<'ctx, 'm, 'a> = (
        context,
        module,
        builder,
        target_data,
        names,
        nodes,
        lists,
        key_types,
        enum_infos,
        payload_structs,
        inst_fns,
        impls_list,
        protocol,
    );
    emit_program(&mut sess, entry_span)
}

pub(crate) fn host_target() -> Result<(TargetData, TargetTriple), CodegenError> {
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|message| tool_error("llvm", None, &message))?;
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple)
        .map_err(|message| tool_error("llvm", None, &message.to_string()))?;
    let cpu = TargetMachine::get_host_cpu_name().to_string();
    let features = TargetMachine::get_host_cpu_features().to_string();
    let machine = target
        .create_target_machine(
            &triple,
            &cpu,
            &features,
            OptimizationLevel::Default,
            RelocMode::Default,
            CodeModel::Default,
        )
        .ok_or_else(|| tool_error("llvm", None, "failed to create the host target machine"))?;
    let target_data = machine.get_target_data();
    Ok((target_data, triple))
}

fn verify_module(module: &Module) -> Result<(), CodegenError> {
    match module.verify() {
        Ok(()) => Ok(()),
        Err(message) => Err(tool_error("llvm", None, &message.to_string())),
    }
}

fn temp_dir_root() -> PathBuf {
    std::env::temp_dir().join(format!("cinnabar_{}", std::process::id()))
}

fn temp_path(out: &Path, ext: &str) -> PathBuf {
    let base = match out.file_name() {
        Some(name) => name.to_string_lossy().to_string(),
        None => "out".to_string(),
    };
    temp_dir_root().join(format!("{}.{}", base, ext))
}

fn write_text(path: &Path, text: &str) -> Result<(), CodegenError> {
    fs::write(path, text).map_err(|err| io_error(&format!("cannot write '{}': {}", path.display(), err)))
}

fn assemble(ir_path: &Path, obj_path: &Path, opt_level: &str) -> Result<(), CodegenError> {
    let opt_path = ir_path.with_extension("opt.ll");
    let (passes, llc_level) = opt_flags(opt_level);
    run_tool(
        "opt",
        &[
            &passes,
            "-o",
            &opt_path.to_string_lossy(),
            &ir_path.to_string_lossy(),
        ],
    )?;
    run_tool(
        "llc",
        &[
            &llc_level,
            "-filetype=obj",
            "-o",
            &obj_path.to_string_lossy(),
            &opt_path.to_string_lossy(),
        ],
    )
}

fn opt_flags(level: &str) -> (String, String) {
    if level == "0" {
        ("-passes=default<O0>".to_string(), "-O0".to_string())
    } else if level == "1" {
        ("-passes=default<O1>".to_string(), "-O1".to_string())
    } else if level == "3" {
        ("-passes=default<O3>".to_string(), "-O3".to_string())
    } else if level == "s" {
        ("-passes=default<Os>".to_string(), "-Os".to_string())
    } else if level == "z" {
        ("-passes=default<Oz>".to_string(), "-Oz".to_string())
    } else {
        ("-passes=default<O2>".to_string(), "-O2".to_string())
    }
}

fn link(obj_path: &Path, out: &Path) -> Result<(), CodegenError> {
    let libc_path = temp_path(out, "libc.a");
    let crt1_path = temp_path(out, "crt1.o");
    let crti_path = temp_path(out, "crti.o");
    let crtn_path = temp_path(out, "crtn.o");
    write_bytes(&libc_path, MUSL_LIBC_A)?;
    write_bytes(&crt1_path, MUSL_CRT1_O)?;
    write_bytes(&crti_path, MUSL_CRTI_O)?;
    write_bytes(&crtn_path, MUSL_CRTN_O)?;
    run_tool(
        "clang",
        &[
            "-static",
            "-nostdlib",
            "-no-pie",
            "-o",
            &out.to_string_lossy(),
            &crt1_path.to_string_lossy(),
            &crti_path.to_string_lossy(),
            &obj_path.to_string_lossy(),
            &libc_path.to_string_lossy(),
            &crtn_path.to_string_lossy(),
        ],
    )
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), CodegenError> {
    fs::write(path, bytes).map_err(|err| io_error(&format!("cannot write '{}': {}", path.display(), err)))
}

fn run_tool(tool: &str, args: &[&str]) -> Result<(), CodegenError> {
    let output = match Command::new(tool).args(args).output() {
        Ok(output) => output,
        Err(err) => return Err(tool_error(tool, None, &err.to_string())),
    };
    if output.status.success() {
        return Ok(());
    }
    let code = output.status.code();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim_end().to_string();
    Err(tool_error(tool, code, &detail))
}
