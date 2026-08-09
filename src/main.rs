//! Cinnabar compiler driver.
//!
//! Runs the fixed pipeline — lexer, parser, module loader, resolver,
//! typechecker, borrow checker, codegen — over the shared arenas and
//! renders every diagnostic at its real source origin.  `--dump-ast`
//! prints the parsed tree of the entry file; `--run` executes the
//! compiled binary after linking.

mod ast;
mod borrow;
mod codegen;
mod lexer;
mod module_loader;
mod parser;
mod resolver;
mod typecheck;

use crate::ast::*;
use ariadne::{Color, FnCache, Label, Report, ReportKind};
use clap::builder::PathBufValueParser;
use clap::{Arg, ArgAction, Command as ClapCommand};
use codegen::compile_and_link;
use codegen::error::codegen_error_message;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Parses the command line.  A missing required argument is a usage
/// error: clap prints the usage and exits with failure.
///
/// Returns `(input, output, dump_ast, run)`.
fn parse_args() -> Option<(PathBuf, Option<PathBuf>, bool, bool)> {
    let matches = ClapCommand::new("cinnabar")
        .about("Cinnabar compiler")
        .arg(
            Arg::new("input")
                .value_name("FILE")
                .required(true)
                .value_parser(PathBufValueParser::new())
                .help("Input Cinnabar source file"),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("PATH")
                .value_parser(PathBufValueParser::new())
                .help("Output binary path"),
        )
        .arg(
            Arg::new("dump_ast")
                .long("dump-ast")
                .action(ArgAction::SetTrue)
                .help("Print the parsed AST and exit"),
        )
        .arg(
            Arg::new("run")
                .long("run")
                .action(ArgAction::SetTrue)
                .help("Execute the compiled binary after building"),
        )
        .get_matches();
    let input = {
        let path = matches.get_one::<PathBuf>("input")?;
        path.clone()
    };
    let output = matches.get_one::<PathBuf>("output").cloned();
    Some((input, output, matches.get_flag("dump_ast"), matches.get_flag("run")))
}

fn main() -> ExitCode {
    let (input, output, dump_ast, run) = match parse_args() {
        Some(args) => args,
        None => return ExitCode::FAILURE,
    };
    let mut names: Vec<String> = Vec::new();
    let mut nodes: Vec<i64> = Vec::new();
    let mut lists: Vec<Vec<i64>> = Vec::new();
    let mut errors: Vec<Diag> = Vec::new();
    let entry = input.to_string_lossy().to_string();
    let (loaded, files) = module_loader::load(&mut names, &mut nodes, &mut lists, &mut errors, &entry);
    let (root, ext_mods) = match loaded {
        Some(program) => program,
        None => return finish_with_diagnostics(&errors, &files),
    };
    if dump_ast {
        dump_program(&names, &nodes, &lists, root);
        return ExitCode::SUCCESS;
    }
    if !resolver::resolve(&mut names, &mut nodes, &mut lists, &mut errors, root, &ext_mods) {
        return finish_with_diagnostics(&errors, &files);
    }
    let (ok, impls_list) = typecheck::typecheck(&mut names, &mut nodes, &mut lists, &mut errors, root, &ext_mods);
    if !ok {
        return finish_with_diagnostics(&errors, &files);
    }
    if !borrow::borrow_check(&mut names, &mut nodes, &mut lists, &mut errors, root, &ext_mods) {
        return finish_with_diagnostics(&errors, &files);
    }
    let out = match &output {
        Some(path) => path.clone(),
        None => default_out_path(&input),
    };
    if let Err(codegen_err) = compile_and_link(&names, &mut nodes, &mut lists, impls_list, &out) {
        return finish_with_codegen_error(&codegen_err, &files);
    }
    if run {
        return run_binary(&out);
    }
    println!("Successfully compiled {} to '{}'.", entry, out.display());
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// Diagnostics.
// ---------------------------------------------------------------------------

/// The ariadne cache for the loaded files: ids are file paths, backed
/// by ariadne's closure-driven `FnCache`.  The fetch closure resolves a
/// path to its in-memory source text; `display` shows the path itself.
fn make_cache(
    files: &[(String, String)],
) -> FnCache<String, impl FnMut(&String) -> Result<String, String>, String> {
    FnCache::new(|path: &String| {
        let mut idx = 0usize;
        while idx < files.len() {
            match files.get(idx) {
                Some((entry_path, text)) => {
                    if entry_path == path {
                        return Ok(text.clone());
                    }
                }
                None => break,
            }
            idx += 1;
        }
        Err(format!("unknown source file '{}'", path))
    })
}

/// The path of file id `file_id`, or `None` when the id is out of range.
fn file_path_of(files: &[(String, String)], file_id: i64) -> Option<String> {
    files.get(file_id as usize).map(|entry| entry.0.clone())
}

/// Renders every diagnostic, each at its own source origin.  Internal
/// diagnostics (no source) are printed plainly.  A rendering failure is
/// returned to the driver instead of being swallowed.
fn render_diagnostics(errors: &[Diag], files: &[(String, String)]) -> Result<(), String> {
    let mut cache = make_cache(files);
    let mut idx = 0usize;
    while idx < errors.len() {
        match errors.get(idx) {
            Some(diag) => render_diag(&mut cache, files, diag)?,
            None => break,
        }
        idx += 1;
    }
    Ok(())
}

/// Renders one diagnostic.  A `NO_FILE` span means the failure has no
/// Cinnabar source origin; it is printed as plain text.
fn render_diag(
    cache: &mut FnCache<String, impl FnMut(&String) -> Result<String, String>, String>,
    files: &[(String, String)],
    diag: &Diag,
) -> Result<(), String> {
    if diag.1 == NO_FILE {
        eprintln!("error: {}", diag.0);
        return Ok(());
    }
    let path = match file_path_of(files, diag.1) {
        Some(path) => path,
        None => {
            eprintln!("error: {} (unknown file {})", diag.0, diag.1);
            return Ok(());
        }
    };
    let span = diag.2 as usize..diag.3 as usize;
    let report = Report::build(ReportKind::Error, (path.clone(), span.clone()))
        .with_message(&diag.0)
        .with_label(Label::new((path, span)).with_message("here").with_color(Color::Red))
        .finish();
    report
        .print(&mut *cache)
        .map_err(|render_err| format!("cannot render '{}': {}", diag.0, render_err))
}

/// Renders a codegen failure.  The span carries the failing construct's
/// origin, or `(-1, 0, 0)` for toolchain failures with no source.
fn render_codegen_error(codegen_err: &codegen::error::CodegenError, files: &[(String, String)]) -> Result<(), String> {
    let mut cache = make_cache(files);
    if codegen_err.span.0 == NO_FILE {
        eprintln!("error: {}", codegen_error_message(codegen_err));
        return Ok(());
    }
    let path = match file_path_of(files, codegen_err.span.0) {
        Some(path) => path,
        None => {
            eprintln!(
                "error: {} (unknown file {})",
                codegen_error_message(codegen_err),
                codegen_err.span.0
            );
            return Ok(());
        }
    };
    let span = codegen_err.span.1 as usize..codegen_err.span.2 as usize;
    let report = Report::build(ReportKind::Error, (path.clone(), span.clone()))
        .with_message(codegen_error_message(codegen_err))
        .with_label(
            Label::new((path, span))
                .with_message("codegen error")
                .with_color(Color::Red),
        )
        .finish();
    report
        .print(&mut cache)
        .map_err(|render_err| format!("cannot render codegen error: {}", render_err))
}

/// The default output path: the input path with a trailing `.cnb`
/// stripped.  Only the exact `.cnb` suffix is removed, so a source file
/// with any other extension keeps its name as the output.
fn default_out_path(input: &Path) -> PathBuf {
    let text = input.to_string_lossy();
    match text.strip_suffix(".cnb") {
        Some(stripped) => PathBuf::from(stripped),
        None => input.to_path_buf(),
    }
}

/// Renders the accumulated diagnostics and exits with failure.  A
/// rendering failure is printed, but compilation already failed, so the
/// driver still exits with failure.
fn finish_with_diagnostics(errors: &[Diag], files: &[(String, String)]) -> ExitCode {
    if let Err(message) = render_diagnostics(errors, files) {
        eprintln!("failed to render diagnostic: {}", message);
    }
    ExitCode::FAILURE
}

/// Renders a codegen failure and exits with failure, propagating a
/// rendering failure the same way `finish_with_diagnostics` does.
fn finish_with_codegen_error(codegen_err: &codegen::error::CodegenError, files: &[(String, String)]) -> ExitCode {
    if let Err(message) = render_codegen_error(codegen_err, files) {
        eprintln!("failed to render diagnostic: {}", message);
    }
    ExitCode::FAILURE
}

// ---------------------------------------------------------------------------
// AST dump.
// ---------------------------------------------------------------------------

/// Two-space indent for `depth` levels.
fn pad_str(depth: i64) -> String {
    let mut out = String::new();
    let mut idx = 0i64;
    while idx < depth {
        out.push_str("  ");
        idx += 1;
    }
    out
}

/// Prints the entry file's top-level items.
fn dump_program(names: &[String], nodes: &[i64], lists: &[Vec<i64>], root: i64) {
    dump_item_list(names, nodes, lists, root, 0);
}

fn dump_item_list(names: &[String], nodes: &[i64], lists: &[Vec<i64>], list: i64, depth: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        dump_item(names, nodes, lists, list_get(lists, list, idx), depth);
        idx += 1;
    }
}

/// The dotted text of a path-segments list.
fn path_text(names: &[String], lists: &[Vec<i64>], list: i64) -> String {
    let mut parts: Vec<i64> = Vec::new();
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        parts.push(list_get(lists, list, idx));
        idx += 1;
    }
    join_path(names, &parts)
}

fn opt_name(names: &[String], id: i64) -> String {
    if id == NONE {
        "none".to_string()
    } else {
        name_text(names, id)
    }
}

fn dump_item(names: &[String], nodes: &[i64], lists: &[Vec<i64>], item: i64, depth: i64) {
    if node_tag(nodes, item) != NODE_ITEM {
        return;
    }
    let pad = pad_str(depth);
    let pub_flag = if node_b(nodes, item) == 1 {
        "is_pub: true"
    } else {
        "is_pub: false"
    };
    let kind = node_a(nodes, item);
    if kind == ITEM_MODULE {
        println!("{}Module({}, name: {}, children:", pad, pub_flag, name_text(names, node_d(nodes, item)));
        dump_item_list(names, nodes, lists, node_e(nodes, item), depth + 1);
        println!("{})", pad);
    } else if kind == ITEM_USE {
        println!(
            "Use({}, path: {}, alias: {})",
            pub_flag,
            path_text(names, lists, node_d(nodes, item)),
            opt_name(names, node_e(nodes, item))
        );
    } else if kind == ITEM_STRUCT {
        println!("{}Struct({}, name: {}, fields:", pad, pub_flag, name_text(names, node_d(nodes, item)));
        dump_field_list(names, nodes, lists, node_e(nodes, item), depth + 1);
        println!("{})", pad);
    } else if kind == ITEM_ENUM {
        println!("{}Enum({}, name: {}, variants:", pad, pub_flag, name_text(names, node_d(nodes, item)));
        dump_variant_list(names, nodes, lists, node_e(nodes, item), depth + 1);
        println!("{})", pad);
    } else if kind == ITEM_TRAIT {
        println!("{}Trait({}, name: {}, methods:", pad, pub_flag, name_text(names, node_d(nodes, item)));
        dump_fn_list(names, nodes, lists, node_e(nodes, item), depth + 1);
        println!("{})", pad);
    } else if kind == ITEM_IMPL {
        println!(
            "{}Impl({}, trait: {}, for: ",
            pad,
            pub_flag,
            path_text(names, lists, node_d(nodes, item))
        );
        dump_ty(names, nodes, lists, node_e(nodes, item), depth + 1);
        println!("{}methods:", pad);
        dump_fn_list(names, nodes, lists, node_f(nodes, item), depth + 1);
        println!("{})", pad);
    } else if kind == ITEM_FUN {
        println!("{}Fun({}, ", pad, pub_flag);
        dump_fn(names, nodes, lists, node_d(nodes, item), depth + 1);
        println!("{})", pad);
    } else if kind == ITEM_NATIVE_FUN {
        println!("{}NativeFun({}, ", pad, pub_flag);
        dump_fn(names, nodes, lists, node_d(nodes, item), depth + 1);
        println!("{})", pad);
    } else if kind == ITEM_CONST {
        println!(
            "{}Const({}, name: {}, ty: ",
            pad,
            pub_flag,
            name_text(names, node_d(nodes, item))
        );
        dump_ty(names, nodes, lists, node_e(nodes, item), depth + 1);
        println!("{}value: ", pad);
        dump_expr(names, nodes, lists, node_f(nodes, item), depth + 1);
        println!("{})", pad);
    } else if kind == ITEM_NATIVE_TYPE {
        println!("{}NativeType({}, name: {})", pad, pub_flag, name_text(names, node_d(nodes, item)));
    }
}

fn dump_field_list(names: &[String], nodes: &[i64], lists: &[Vec<i64>], list: i64, depth: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        let field = list_get(lists, list, idx);
        let pad = pad_str(depth);
        let pub_flag = if node_f(nodes, field) == 1 { "is_pub: true" } else { "is_pub: false" };
        println!("{}Field({}, name: {}, ty: ", pad, pub_flag, name_text(names, node_d(nodes, field)));
        dump_ty(names, nodes, lists, node_e(nodes, field), depth + 1);
        println!("{})", pad);
        idx += 1;
    }
}

fn dump_variant_list(names: &[String], nodes: &[i64], lists: &[Vec<i64>], list: i64, depth: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        let variant = list_get(lists, list, idx);
        let pad = pad_str(depth);
        println!("{}Variant(name: {}, payload:", pad, name_text(names, node_d(nodes, variant)));
        dump_ty_list(names, nodes, lists, node_e(nodes, variant), depth + 1);
        println!("{})", pad);
        idx += 1;
    }
}

fn dump_fn_list(names: &[String], nodes: &[i64], lists: &[Vec<i64>], list: i64, depth: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        dump_fn(names, nodes, lists, list_get(lists, list, idx), depth);
        idx += 1;
    }
}

fn dump_fn(names: &[String], nodes: &[i64], lists: &[Vec<i64>], id: i64, depth: i64) {
    if node_tag(nodes, id) != NODE_FN {
        return;
    }
    let pad = pad_str(depth);
    let impure = if node_e(nodes, id) == 1 { "impure" } else { "pure" };
    println!("{}Fn(name: {}, {}, params:", pad, name_text(names, node_a(nodes, id)), impure);
    dump_param_list(names, nodes, lists, node_c(nodes, id), depth + 1);
    println!("{}ret: ", pad);
    dump_ty(names, nodes, lists, node_d(nodes, id), depth + 1);
    if node_f(nodes, id) != NONE {
        println!("{}body:", pad);
        dump_stmt_list(names, nodes, lists, node_f(nodes, id), depth + 1);
    }
    println!("{})", pad);
}

fn dump_param_list(names: &[String], nodes: &[i64], lists: &[Vec<i64>], list: i64, depth: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(lists, list, idx);
        let pad = pad_str(depth);
        println!("{}Param(name: {}, ty: ", pad, name_text(names, node_d(nodes, param)));
        dump_ty(names, nodes, lists, node_e(nodes, param), depth + 1);
        println!("{})", pad);
        idx += 1;
    }
}

fn dump_ty_list(names: &[String], nodes: &[i64], lists: &[Vec<i64>], list: i64, depth: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        dump_ty(names, nodes, lists, list_get(lists, list, idx), depth);
        idx += 1;
    }
}

fn dump_ty(names: &[String], nodes: &[i64], lists: &[Vec<i64>], id: i64, depth: i64) {
    if node_tag(nodes, id) != NODE_TY {
        return;
    }
    let pad = pad_str(depth);
    let kind = node_a(nodes, id);
    if kind == TY_NAMED {
        println!("{}Type(Named({}))", pad, name_text(names, node_b(nodes, id)));
    } else if kind == TY_PATH {
        println!("{}Type(Path({}))", pad, path_text(names, lists, node_b(nodes, id)));
    } else if kind == TY_GENERIC {
        println!("{}Type(Generic({}, args: ", pad, path_text(names, lists, node_b(nodes, id)));
        dump_ty_list(names, nodes, lists, node_c(nodes, id), depth + 1);
        println!("{})", pad);
    } else if kind == TY_REF {
        println!("{}Type(Ref(", pad);
        dump_ty(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{}))", pad);
    } else if kind == TY_REF_MUT {
        println!("{}Type(RefMut(", pad);
        dump_ty(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{}))", pad);
    } else if kind == TY_SLICE {
        println!("{}Type(Slice(", pad);
        dump_ty(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{}))", pad);
    } else if kind == TY_ARRAY {
        println!("{}Type(Array(len: {}, elem: ", pad, node_c(nodes, id));
        dump_ty(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{}))", pad);
    } else if kind == TY_SELF {
        println!("{}Type(Self)", pad);
    } else if kind == TY_PARAM {
        println!("{}Type(Param({}))", pad, name_text(names, node_b(nodes, id)));
    }
}

fn dump_stmt_list(names: &[String], nodes: &[i64], lists: &[Vec<i64>], list: i64, depth: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        dump_stmt(names, nodes, lists, list_get(lists, list, idx), depth);
        idx += 1;
    }
}

fn dump_stmt(names: &[String], nodes: &[i64], lists: &[Vec<i64>], id: i64, depth: i64) {
    if node_tag(nodes, id) != NODE_STMT {
        return;
    }
    let pad = pad_str(depth);
    let kind = node_a(nodes, id);
    if kind == STMT_LET {
        println!(
            "{}Let(mut: {}, name: {}, ty: ",
            pad,
            node_b(nodes, id),
            name_text(names, node_c(nodes, id))
        );
        if node_d(nodes, id) != NONE {
            dump_ty(names, nodes, lists, node_d(nodes, id), depth + 1);
        }
        println!("{}init: ", pad);
        dump_expr(names, nodes, lists, node_e(nodes, id), depth + 1);
        println!("{})", pad);
    } else if kind == STMT_ASSIGN {
        println!("{}Assign(target: ", pad);
        dump_expr(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{}value: ", pad);
        dump_expr(names, nodes, lists, node_c(nodes, id), depth + 1);
        println!("{})", pad);
    } else if kind == STMT_WHILE {
        println!("{}While(cond: ", pad);
        dump_expr(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{}body:", pad);
        dump_stmt_list(names, nodes, lists, node_c(nodes, id), depth + 1);
        println!("{})", pad);
    } else if kind == STMT_IF {
        println!("{}If(cond: ", pad);
        dump_expr(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{}then:", pad);
        dump_stmt_list(names, nodes, lists, node_c(nodes, id), depth + 1);
        if node_d(nodes, id) != NONE {
            println!("{}else:", pad);
            dump_stmt_list(names, nodes, lists, node_d(nodes, id), depth + 1);
        }
        println!("{})", pad);
    } else if kind == STMT_RETURN {
        println!("{}Return(value: ", pad);
        if node_b(nodes, id) != NONE {
            dump_expr(names, nodes, lists, node_b(nodes, id), depth + 1);
        }
        println!("{})", pad);
    } else if kind == STMT_BREAK {
        println!("{}Break", pad);
    } else if kind == STMT_CONTINUE {
        println!("{}Continue", pad);
    } else if kind == STMT_EXPR {
        println!("{}ExprStmt(", pad);
        dump_expr(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{})", pad);
    }
}

fn dump_expr_list(names: &[String], nodes: &[i64], lists: &[Vec<i64>], list: i64, depth: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        dump_expr(names, nodes, lists, list_get(lists, list, idx), depth);
        idx += 1;
    }
}

fn dump_expr(names: &[String], nodes: &[i64], lists: &[Vec<i64>], id: i64, depth: i64) {
    if node_tag(nodes, id) != NODE_EXPR {
        return;
    }
    let pad = pad_str(depth);
    let kind = node_a(nodes, id);
    if kind == EXPR_LIT {
        println!("{}Lit({}, {})", pad, lit_kind_name(node_b(nodes, id)), node_c(nodes, id));
    } else if kind == EXPR_PATH {
        println!("{}Path({})", pad, path_text(names, lists, node_b(nodes, id)));
    } else if kind == EXPR_UNARY {
        println!("{}Unary({}, ", pad, unary_name(node_b(nodes, id)));
        dump_expr(names, nodes, lists, node_c(nodes, id), depth + 1);
        println!("{})", pad);
    } else if kind == EXPR_BINARY {
        println!("{}Binary({}, lhs: ", pad, bin_name(node_b(nodes, id)));
        dump_expr(names, nodes, lists, node_c(nodes, id), depth + 1);
        println!("{}rhs: ", pad);
        dump_expr(names, nodes, lists, node_d(nodes, id), depth + 1);
        println!("{})", pad);
    } else if kind == EXPR_CALL {
        println!("{}Call(callee: ", pad);
        dump_expr(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{}args:", pad);
        dump_expr_list(names, nodes, lists, node_d(nodes, id), depth + 1);
        println!("{})", pad);
    } else if kind == EXPR_STRUCT_LIT {
        println!("{}StructLit({}, fields: ", pad, path_text(names, lists, node_b(nodes, id)));
        let names_list = node_c(nodes, id);
        let values_list = node_d(nodes, id);
        let count = list_len(lists, names_list);
        let mut idx = 0i64;
        while idx < count {
            println!("{}{}: ", pad, name_text(names, list_get(lists, names_list, idx)));
            dump_expr(names, nodes, lists, list_get(lists, values_list, idx), depth + 1);
            idx += 1;
        }
        println!("{})", pad);
    } else if kind == EXPR_ARRAY {
        println!("{}Array(elems:", pad);
        dump_expr_list(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{})", pad);
    } else if kind == EXPR_MATCH {
        println!("{}Match(scrutinee: ", pad);
        dump_expr(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{}arms:", pad);
        dump_arm_list(names, nodes, lists, node_c(nodes, id), depth + 1);
        println!("{})", pad);
    } else if kind == EXPR_TRY {
        println!("{}Try(", pad);
        dump_expr(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{})", pad);    } else if kind == EXPR_INDEX {
        println!("{}Index(base:", pad);
        dump_expr(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{}index: ", pad);
        dump_expr(names, nodes, lists, node_c(nodes, id), depth + 1);
        println!("{})", pad);
    } else if kind == EXPR_FIELD_ACCESS {
        println!("{}FieldAccess(base: ", pad);
        dump_expr(names, nodes, lists, node_b(nodes, id), depth + 1);
        println!("{}.{})", pad, name_text(names, node_c(nodes, id)));
    }
}

fn dump_arm_list(names: &[String], nodes: &[i64], lists: &[Vec<i64>], list: i64, depth: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        let arm = list_get(lists, list, idx);
        let pad = pad_str(depth);
        println!("{}Arm(pattern: ", pad);
        dump_pat(names, nodes, lists, node_a(nodes, arm), depth + 1);
        println!("{}body:", pad);
        dump_stmt_list(names, nodes, lists, node_b(nodes, arm), depth + 1);
        println!("{})", pad);
        idx += 1;
    }
}

fn dump_pat_list(names: &[String], nodes: &[i64], lists: &[Vec<i64>], list: i64, depth: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        dump_pat(names, nodes, lists, list_get(lists, list, idx), depth);
        idx += 1;
    }
}

fn dump_pat(names: &[String], nodes: &[i64], lists: &[Vec<i64>], id: i64, depth: i64) {
    if node_tag(nodes, id) != NODE_PAT {
        return;
    }
    let pad = pad_str(depth);
    let kind = node_a(nodes, id);
    if kind == PAT_BIND {
        println!("{}Bind({})", pad, name_text(names, node_b(nodes, id)));
    } else if kind == PAT_PATH {
        println!("{}PatPath({})", pad, path_text(names, lists, node_b(nodes, id)));
    } else if kind == PAT_VARIANT {
        println!("{}PatVariant({}, payload:", pad, path_text(names, lists, node_b(nodes, id)));
        dump_pat_list(names, nodes, lists, node_c(nodes, id), depth + 1);
        println!("{})", pad);
    } else if kind == PAT_ARRAY {
        println!("{}PatArray(elems:", pad);
        dump_pat_list(names, nodes, lists, node_b(nodes, id), depth + 1);
        if node_c(nodes, id) != NONE {
            println!("{}rest: {}", pad, name_text(names, node_c(nodes, id)));
        }
        println!("{})", pad);
    } else if kind == PAT_LIT {
        println!("{}PatLit({}, {})", pad, lit_kind_name(node_b(nodes, id)), node_c(nodes, id));
    }
}

fn lit_kind_name(kind: i64) -> &'static str {
    if kind == TOK_INT {
        "int"
    } else if kind == TOK_HEX {
        "hex"
    } else {
        "?lit"
    }
}

fn unary_name(op: i64) -> &'static str {
    if op == UN_NEG {
        "neg"
    } else if op == UN_NOT {
        "not"
    } else if op == UN_REF {
        "ref"
    } else if op == UN_REF_MUT {
        "ref_mut"
    } else {
        "?un"
    }
}

fn bin_name(op: i64) -> &'static str {
    if op == BIN_ADD {
        "+"
    } else if op == BIN_SUB {
        "-"
    } else if op == BIN_MUL {
        "*"
    } else if op == BIN_DIV {
        "/"
    } else if op == BIN_MOD {
        "%"
    } else if op == BIN_SHL {
        "<<"
    } else if op == BIN_SHR {
        ">>"
    } else if op == BIN_BAND {
        "&"
    } else if op == BIN_BOR {
        "|"
    } else if op == BIN_BXOR {
        "^"
    } else if op == BIN_EQ {
        "=="
    } else if op == BIN_NE {
        "!="
    } else if op == BIN_LT {
        "<"
    } else if op == BIN_GT {
        ">"
    } else if op == BIN_LE {
        "<="
    } else if op == BIN_GE {
        ">="
    } else if op == BIN_AND {
        "&&"
    } else if op == BIN_OR {
        "||"
    } else {
        "?bin"
    }
}

// ---------------------------------------------------------------------------
// Running the compiled binary.
// ---------------------------------------------------------------------------

/// Executes `path` and maps its exit status to the driver's exit code.
fn run_binary(path: &Path) -> ExitCode {
    match Command::new(path).status() {
        Ok(status) => match status.code() {
            Some(code) => {
                if code == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            None => ExitCode::FAILURE,
        },
        Err(err) => {
            eprintln!("failed to execute '{}': {}", path.display(), err);
            ExitCode::FAILURE
        }
    }
}
