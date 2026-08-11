use cinnabar::ast::*;
use cinnabar::{borrow, codegen, module_loader, resolver, typecheck};
use ariadne::{Color, FnCache, Label, Report, ReportKind, Source};
use clap::builder::PathBufValueParser;
use clap::{Arg, ArgAction, Command as ClapCommand};
use cinnabar::codegen::error::codegen_error_message;
use cinnabar::codegen::{compile_and_link, compile_to_ir, compile_to_object};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

struct CliArgs {
    input: PathBuf,
    output: Option<PathBuf>,
    dump_ast: bool,
    dump_typed_ast: bool,
    print_layout: bool,
    explain_borrow: bool,
    run: bool,
    opt_level: String,
    emit_llvm: bool,
    emit_obj: bool,
}

fn parse_args() -> Option<CliArgs> {
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
            Arg::new("dump_typed_ast")
                .long("dump-typed-ast")
                .action(ArgAction::SetTrue)
                .conflicts_with_all(["dump_ast", "emit_llvm", "emit_obj", "run"])
                .help("Run the full front-end (resolve, typecheck, borrow-check), then print the node arena with every attached fact and exit"),
        )
        .arg(
            Arg::new("explain_borrow")
                .long("explain-borrow")
                .action(ArgAction::SetTrue)
                .help("Attach secondary labels to borrow/linearity errors explaining which paths consume a value, where it was bound, and where it was previously moved"),
        )
        .arg(
            Arg::new("print_layout")
                .long("print-layout")
                .action(ArgAction::SetTrue)
                .conflicts_with_all(["dump_ast", "dump_typed_ast", "emit_llvm", "emit_obj", "run"])
                .help("Run the full front-end, then print size/alignment/field offsets for every concrete struct, enum, and native handle and exit"),
        )
        .arg(
            Arg::new("run")
                .long("run")
                .action(ArgAction::SetTrue)
                .conflicts_with_all(["emit_llvm", "emit_obj"])
                .help("Execute the compiled binary after building"),
        )
        .arg(
            Arg::new("opt_level")
                .short('O')
                .long("opt-level")
                .value_name("LEVEL")
                .value_parser(clap::builder::PossibleValuesParser::new(["0", "1", "2", "3", "s", "z"]))
                .help("Optimization level: 0, 1, 2, 3, s, z (default 2)"),
        )
        .arg(
            Arg::new("emit_llvm")
                .long("emit-llvm")
                .action(ArgAction::SetTrue)
                .conflicts_with("emit_obj")
                .help("Write the emitted LLVM IR (before optimization) and stop; default output is the input path with .ll"),
        )
        .arg(
            Arg::new("emit_obj")
                .long("emit-obj")
                .action(ArgAction::SetTrue)
                .help("Optimize and assemble to a relocatable object file, skipping the link; default output is the input path with .o"),
        )
        .get_matches();
    let input = {
        let path = matches.get_one::<PathBuf>("input")?;
        path.clone()
    };
    let output = matches.get_one::<PathBuf>("output").cloned();
    let opt_level = matches
        .get_one::<String>("opt_level")
        .cloned()
        .unwrap_or_else(|| "2".to_string());
    Some(CliArgs {
        input,
        output,
        dump_ast: matches.get_flag("dump_ast"),
        dump_typed_ast: matches.get_flag("dump_typed_ast"),
        print_layout: matches.get_flag("print_layout"),
        explain_borrow: matches.get_flag("explain_borrow"),
        run: matches.get_flag("run"),
        opt_level,
        emit_llvm: matches.get_flag("emit_llvm"),
        emit_obj: matches.get_flag("emit_obj"),
    })
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Some(args) => args,
        None => return ExitCode::FAILURE,
    };
    let mut names: Vec<String> = Vec::new();
    let mut nodes: Vec<i64> = Vec::new();
    let mut lists: Vec<Vec<i64>> = Vec::new();
    let mut errors: Vec<Diag> = Vec::new();
    let mut notes: Vec<Note> = Vec::new();
    let entry = args.input.to_string_lossy().to_string();
    let (loaded, files) = module_loader::load(&mut names, &mut nodes, &mut lists, &mut errors, &entry);
    let (root, ext_mods) = match loaded {
        Some(program) => program,
        None => return finish_with_diagnostics(&errors, &[], &files),
    };
    if args.dump_ast {
        dump_program(&names, &nodes, &lists, root);
        return ExitCode::SUCCESS;
    }
    if !resolver::resolve(&mut names, &mut nodes, &mut lists, &mut errors, root, &ext_mods) {
        return finish_with_diagnostics(&errors, &[], &files);
    }
    let (ok, impls_list) = typecheck::typecheck(&mut names, &mut nodes, &mut lists, &mut errors, root, &ext_mods);
    if !ok {
        return finish_with_diagnostics(&errors, &[], &files);
    }
    if !borrow::borrow_check(&mut names, &mut nodes, &mut lists, &mut errors, &mut notes, root, &ext_mods) {
        let shown_notes: &[Note] = if args.explain_borrow { &notes } else { &[] };
        return finish_with_diagnostics(&errors, shown_notes, &files);
    }
    if args.dump_typed_ast {
        print!("{}", cinnabar::inspect::dump_typed_arena(&names, &nodes, &lists));
        return ExitCode::SUCCESS;
    }
    if args.print_layout {
        match cinnabar::codegen::layout::render_layouts(&names, &mut nodes, &mut lists) {
            Ok(report) => {
                print!("{}", report);
                return ExitCode::SUCCESS;
            }
            Err(codegen_err) => return finish_with_codegen_error(&codegen_err, &files),
        }
    }
    let entry_span = entry_span_of(&files);
    if args.emit_llvm {
        let out = emit_out_path(&args, "ll");
        let written = compile_to_ir(&names, &mut nodes, &mut lists, impls_list, entry_span)
            .and_then(|ir_text| write_output_text(&out, &ir_text));
        if let Err(codegen_err) = written {
            return finish_with_codegen_error(&codegen_err, &files);
        }
        println!("Emitted LLVM IR for {} to '{}'.", entry, out.display());
        return ExitCode::SUCCESS;
    }
    if args.emit_obj {
        let out = emit_out_path(&args, "o");
        if let Err(codegen_err) =
            compile_to_object(&names, &mut nodes, &mut lists, impls_list, &out, &args.opt_level, entry_span)
        {
            return finish_with_codegen_error(&codegen_err, &files);
        }
        println!("Emitted object file for {} to '{}'.", entry, out.display());
        return ExitCode::SUCCESS;
    }
    let out = match &args.output {
        Some(path) => path.clone(),
        None => default_out_path(&args.input),
    };
    if let Err(codegen_err) =
        compile_and_link(&names, &mut nodes, &mut lists, impls_list, &out, &args.opt_level, entry_span)
    {
        return finish_with_codegen_error(&codegen_err, &files);
    }
    if args.run {
        return run_binary(&out);
    }
    println!("Successfully compiled {} to '{}'.", entry, out.display());
    ExitCode::SUCCESS
}

fn emit_out_path(args: &CliArgs, ext: &str) -> PathBuf {
    match &args.output {
        Some(path) => path.clone(),
        None => default_out_path(&args.input).with_extension(ext),
    }
}

fn write_output_text(path: &Path, text: &str) -> Result<(), cinnabar::codegen::error::CodegenError> {
    std::fs::write(path, text).map_err(|err| {
        cinnabar::codegen::error::io_error(&format!("cannot write '{}': {}", path.display(), err))
    })
}

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

fn file_path_of(files: &[(String, String)], file_id: i64) -> Option<String> {
    files.get(file_id as usize).map(|entry| entry.0.clone())
}

fn render_diagnostics(errors: &[Diag], notes: &[Note], files: &[(String, String)]) -> Result<(), String> {
    let mut cache = make_cache(files);
    let mut idx = 0usize;
    while idx < errors.len() {
        match errors.get(idx) {
            Some(diag) => render_diag(&mut cache, files, diag, notes, idx as i64)?,
            None => break,
        }
        idx += 1;
    }
    Ok(())
}

fn render_source_less(message: &str) -> Result<(), String> {
    Report::build(ReportKind::Error, 0..0)
        .with_message(message)
        .finish()
        .print(Source::from(""))
        .map_err(|render_err| format!("cannot render '{}': {}", message, render_err))
}

fn render_diag(
    cache: &mut FnCache<String, impl FnMut(&String) -> Result<String, String>, String>,
    files: &[(String, String)],
    diag: &Diag,
    notes: &[Note],
    diag_idx: i64,
) -> Result<(), String> {
    if diag.1 == NO_FILE {
        return render_source_less(&diag.0);
    }
    let path = match file_path_of(files, diag.1) {
        Some(path) => path,
        None => {
            return render_source_less(&format!("{} (unknown source file {})", diag.0, diag.1));
        }
    };
    let span = diag.2 as usize..diag.3 as usize;
    let mut report = Report::build(ReportKind::Error, (path.clone(), span.clone()))
        .with_message(&diag.0)
        .with_label(Label::new((path, span)).with_message("here").with_color(Color::Red));
    let mut note_idx = 0usize;
    while note_idx < notes.len() {
        match notes.get(note_idx) {
            Some(note) => {
                if note.0 == diag_idx && note.2 != NO_FILE {
                    if let Some(note_path) = file_path_of(files, note.2) {
                        report = report.with_label(
                            Label::new((note_path, note.3 as usize..note.4 as usize))
                                .with_message(&note.1)
                                .with_color(Color::Yellow),
                        );
                    }
                }
            }
            None => break,
        }
        note_idx += 1;
    }
    report
        .finish()
        .print(&mut *cache)
        .map_err(|render_err| format!("cannot render '{}': {}", diag.0, render_err))
}

fn render_codegen_error(codegen_err: &codegen::error::CodegenError, files: &[(String, String)]) -> Result<(), String> {
    let mut cache = make_cache(files);
    if codegen_err.span.0 == NO_FILE {
        return render_source_less(&codegen_error_message(codegen_err));
    }
    let path = match file_path_of(files, codegen_err.span.0) {
        Some(path) => path,
        None => {
            return render_source_less(&format!(
                "{} (unknown source file {})",
                codegen_error_message(codegen_err),
                codegen_err.span.0
            ));
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

fn entry_span_of(files: &[(String, String)]) -> (i64, i64, i64) {
    match files.first() {
        Some(pair) => (0, 0, pair.1.len() as i64),
        None => (NO_FILE, 0, 0),
    }
}

fn default_out_path(input: &Path) -> PathBuf {
    let text = input.to_string_lossy();
    match text.strip_suffix(".cnb") {
        Some(stripped) => PathBuf::from(stripped),
        None => input.to_path_buf(),
    }
}

fn finish_with_diagnostics(errors: &[Diag], notes: &[Note], files: &[(String, String)]) -> ExitCode {
    if let Err(message) = render_diagnostics(errors, notes, files) {
        let detail = format!("failed to render diagnostic: {}", message);
        if render_source_less(&detail).is_err() {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::FAILURE
}

fn finish_with_codegen_error(codegen_err: &codegen::error::CodegenError, files: &[(String, String)]) -> ExitCode {
    if let Err(message) = render_codegen_error(codegen_err, files) {
        let detail = format!("failed to render diagnostic: {}", message);
        if render_source_less(&detail).is_err() {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::FAILURE
}

fn pad_str(depth: i64) -> String {
    let mut out = String::new();
    let mut idx = 0i64;
    while idx < depth {
        out.push_str("  ");
        idx += 1;
    }
    out
}

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
        let pub_flag = if node_c(nodes, field) == 1 { "is_pub: true" } else { "is_pub: false" };
        println!("{}Field({}, name: {}, ty: ", pad, pub_flag, name_text(names, node_a(nodes, field)));
        dump_ty(names, nodes, lists, node_b(nodes, field), depth + 1);
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
        println!("{}Variant(name: {}, payload:", pad, name_text(names, node_a(nodes, variant)));
        dump_ty_list(names, nodes, lists, node_b(nodes, variant), depth + 1);
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
        println!("{}Param(name: {}, ty: ", pad, name_text(names, node_a(nodes, param)));
        dump_ty(names, nodes, lists, node_b(nodes, param), depth + 1);
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
