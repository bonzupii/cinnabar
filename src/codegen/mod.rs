
pub mod emitter;
pub mod error;
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

pub fn compile_and_link(
    names: &[String],
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    impls_list: i64,
    out: &Path,
) -> Result<(), CodegenError> {
    let ir_text = emit_to_ir(names, nodes, lists, impls_list)?;
    let ir_path = temp_path(out, "ll");
    let obj_path = temp_path(out, "o");
    write_text(&ir_path, &ir_text)?;
    assemble(&ir_path, &obj_path)?;
    link(&obj_path, out)
}

fn emit_to_ir(
    names: &[String],
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    impls_list: i64,
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
    emit_program(&mut sess)
}

fn host_target() -> Result<(TargetData, TargetTriple), CodegenError> {
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

fn temp_path(out: &Path, ext: &str) -> PathBuf {
    let base = match out.file_name() {
        Some(name) => name.to_string_lossy().to_string(),
        None => "out".to_string(),
    };
    std::env::temp_dir().join(format!("cinnabar_{}_{}.{}", std::process::id(), base, ext))
}

fn write_text(path: &Path, text: &str) -> Result<(), CodegenError> {
    fs::write(path, text).map_err(|err| io_error(&format!("cannot write '{}': {}", path.display(), err)))
}

fn assemble(ir_path: &Path, obj_path: &Path) -> Result<(), CodegenError> {
    let opt_path = ir_path.with_extension("opt.ll");
    run_tool(
        "opt",
        &[
            "-passes=default<O2>",
            "-o",
            &opt_path.to_string_lossy(),
            &ir_path.to_string_lossy(),
        ],
    )?;
    run_tool(
        "llc",
        &[
            "-O2",
            "-filetype=obj",
            "-o",
            &obj_path.to_string_lossy(),
            &opt_path.to_string_lossy(),
        ],
    )
}

fn link(obj_path: &Path, out: &Path) -> Result<(), CodegenError> {
    run_tool(
        "clang",
        &[
            "-nostdinc",
            "-nostdinc++",
            "-no-pie",
            "-o",
            &out.to_string_lossy(),
            &obj_path.to_string_lossy(),
        ],
    )
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
