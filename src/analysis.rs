// IDE-facing queries over the compiler's attached facts.
//
// This module runs the standard front-end pipeline (load -> resolve ->
// typecheck -> borrow) over a file set — optionally overlaid with unsaved
// editor buffers — and answers position queries (hover, definition,
// references, completion, signature help) by reading the attachments the
// pipeline already computed: resolved symbol ids on expression/pattern/type
// rows, canonical type keys, field facts.  It never re-resolves a name or
// re-infers a type with parallel logic (Single-Fact Rule); a question the
// attachments cannot answer is answered with "nothing" rather than with a
// second implementation.

use crate::ast::*;
use crate::inspect::sym_kind_name;
use crate::typecheck::render_type_key;
use crate::{borrow, module_loader, resolver, typecheck};

pub struct Analysis {
    pub names: Vec<String>,
    pub nodes: Vec<i64>,
    pub lists: Vec<Vec<i64>>,
    pub errors: Vec<Diag>,
    pub notes: Vec<Note>,
    pub files: Vec<(String, String)>,
    pub root: i64,
    pub ext_mods: Vec<(i64, i64)>,
    // The typechecker's impls list, kept for consumers that continue into
    // codegen (NONE until the typechecker runs).
    pub impls_list: i64,
    // How far the pipeline got: symbol attachments exist once `resolved`,
    // type attachments once `typechecked` ran (even when it found errors).
    pub resolved: bool,
    pub typechecked: bool,
}

// Completion item kinds beyond the SYM_* symbol kinds.
pub const COMPLETE_LOCAL: i64 = 100;
pub const COMPLETE_KEYWORD: i64 = 101;
pub const COMPLETE_FIELD: i64 = 102;

const KEYWORDS: &[&str] = &[
    "fun", "val", "var", "const", "if", "elif", "else", "while", "match", "return", "break",
    "continue", "end", "use", "as", "pub", "impure", "nat", "try", "mod", "type", "trait", "impl",
    "true", "false",
];

/// Run the front-end over `entry_path` (with unsaved-buffer overlay) and
/// keep everything a query needs.  Never fails: whatever stage stopped the
/// pipeline leaves its diagnostics in `errors`.
pub fn analyze(entry_path: &str, overlay: &[(String, String)]) -> Analysis {
    let mut names: Vec<String> = Vec::new();
    let mut nodes: Vec<i64> = Vec::new();
    let mut lists: Vec<Vec<i64>> = Vec::new();
    let mut errors: Vec<Diag> = Vec::new();
    let mut notes: Vec<Note> = Vec::new();
    let (loaded, files) =
        module_loader::load_with_overlay(&mut names, &mut nodes, &mut lists, &mut errors, entry_path, overlay);
    let (root, ext_mods) = match loaded {
        Some(program) => program,
        None => {
            return Analysis {
                names,
                nodes,
                lists,
                errors,
                notes,
                files,
                root: NONE,
                ext_mods: Vec::new(),
                impls_list: NONE,
                resolved: false,
                typechecked: false,
            };
        }
    };
    let resolved = resolver::resolve(&mut names, &mut nodes, &mut lists, &mut errors, root, &ext_mods);
    let mut typechecked = false;
    let mut impls_list = NONE;
    if resolved {
        let (ok, impls) = typecheck::typecheck(&mut names, &mut nodes, &mut lists, &mut errors, root, &ext_mods);
        impls_list = impls;
        typechecked = true;
        if ok {
            borrow::borrow_check(&mut names, &mut nodes, &mut lists, &mut errors, &mut notes, root, &ext_mods);
        }
    }
    Analysis {
        names,
        nodes,
        lists,
        errors,
        notes,
        files,
        root,
        ext_mods,
        impls_list,
        resolved,
        typechecked,
    }
}

// ---------------------------------------------------------------------------
// Position mapping (byte offsets <-> LSP-style line / UTF-16 column)
// ---------------------------------------------------------------------------

/// Byte offset of the start of every line in `text`.
pub fn line_starts(text: &str) -> Vec<i64> {
    let mut starts: Vec<i64> = Vec::new();
    starts.push(0);
    let bytes = text.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        match bytes.get(idx) {
            Some(byte) => {
                if *byte == b'\n' {
                    starts.push(idx as i64 + 1);
                }
            }
            None => break,
        }
        idx += 1;
    }
    starts
}

/// Convert a byte offset into (line, UTF-16 column).  Offsets are clamped
/// into the text; a mid-code-point offset counts the code point it lands in.
pub fn offset_to_position(text: &str, offset: i64) -> (i64, i64) {
    let starts = line_starts(text);
    let clamped = if offset < 0 {
        0i64
    } else if offset > text.len() as i64 {
        text.len() as i64
    } else {
        offset
    };
    let mut line = 0i64;
    let mut idx = 0usize;
    while idx < starts.len() {
        match starts.get(idx) {
            Some(start) => {
                if *start <= clamped {
                    line = idx as i64;
                }
            }
            None => break,
        }
        idx += 1;
    }
    let line_start = match starts.get(line as usize) {
        Some(start) => *start,
        None => 0,
    };
    let mut col16 = 0i64;
    let mut byte_pos = line_start;
    let tail = match text.get(line_start as usize..) {
        Some(rest) => rest,
        None => "",
    };
    for ch in tail.chars() {
        if byte_pos >= clamped {
            break;
        }
        byte_pos += ch.len_utf8() as i64;
        if byte_pos > clamped {
            break;
        }
        col16 += ch.len_utf16() as i64;
    }
    (line, col16)
}

/// Convert (line, UTF-16 column) into a byte offset, clamping past-end
/// columns to the line end and past-end lines to the text end.
pub fn position_to_offset(text: &str, line: i64, character: i64) -> i64 {
    let starts = line_starts(text);
    let line_start = match starts.get(line as usize) {
        Some(start) => *start,
        None => return text.len() as i64,
    };
    let tail = match text.get(line_start as usize..) {
        Some(rest) => rest,
        None => "",
    };
    let mut col16 = 0i64;
    let mut byte_pos = line_start;
    for ch in tail.chars() {
        if ch == '\n' || col16 >= character {
            break;
        }
        col16 += ch.len_utf16() as i64;
        byte_pos += ch.len_utf8() as i64;
    }
    byte_pos
}

/// The file id of `path` in this analysis, or NONE.  Paths are compared
/// component-wise so separators and casing quirks of the invoking editor
/// don't produce a mismatch on the same file.
pub fn file_id_of(analysis: &Analysis, path: &str) -> i64 {
    let target = std::path::Path::new(path);
    let mut idx = 0usize;
    while idx < analysis.files.len() {
        match analysis.files.get(idx) {
            Some(entry) => {
                if std::path::Path::new(&entry.0) == target {
                    return idx as i64;
                }
            }
            None => break,
        }
        idx += 1;
    }
    NONE
}

/// The source text of a file id, or empty when out of range.
pub fn file_text_of(analysis: &Analysis, file: i64) -> String {
    match analysis.files.get(file as usize) {
        Some(entry) => entry.1.clone(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Node lookup
// ---------------------------------------------------------------------------

fn node_count(analysis: &Analysis) -> i64 {
    analysis.nodes.len() as i64 / NODE_STRIDE
}

fn covers(analysis: &Analysis, id: i64, file: i64, offset: i64) -> bool {
    node_file(&analysis.nodes, id) == file
        && node_start(&analysis.nodes, id) <= offset
        && offset < node_end(&analysis.nodes, id)
}

fn smallest_with_tags(analysis: &Analysis, file: i64, offset: i64, tags: &[i64]) -> i64 {
    let count = node_count(analysis);
    let mut best = NONE;
    let mut best_width = i64::MAX;
    let mut id = 0i64;
    while id < count {
        let tag = node_tag(&analysis.nodes, id);
        let mut wanted = false;
        let mut t_idx = 0usize;
        while t_idx < tags.len() {
            match tags.get(t_idx) {
                Some(candidate) => {
                    if *candidate == tag {
                        wanted = true;
                        break;
                    }
                }
                None => break,
            }
            t_idx += 1;
        }
        if wanted && covers(analysis, id, file, offset) {
            let width = node_end(&analysis.nodes, id) - node_start(&analysis.nodes, id);
            if width <= best_width {
                best = id;
                best_width = width;
            }
        }
        id += 1;
    }
    best
}

/// The smallest expression, pattern, or type node covering (file, offset).
/// When the offset sits just past a node (the cursor at the end of an
/// identifier), the position one byte back is also tried.
pub fn node_at(analysis: &Analysis, file: i64, offset: i64) -> i64 {
    let tags = [NODE_EXPR, NODE_PAT, NODE_TY];
    let found = smallest_with_tags(analysis, file, offset, &tags);
    if found != NONE {
        return found;
    }
    if offset > 0 {
        return smallest_with_tags(analysis, file, offset - 1, &tags);
    }
    NONE
}

fn sym_of_node(analysis: &Analysis, id: i64) -> i64 {
    let tag = node_tag(&analysis.nodes, id);
    if tag == NODE_EXPR {
        return expr_sym_of(&analysis.nodes, id);
    }
    if tag == NODE_PAT {
        return pat_sym_of(&analysis.nodes, id);
    }
    if tag == NODE_TY {
        return crate::ast::ty_sym_of(&analysis.nodes, id);
    }
    NONE
}

fn ty_key_of_node(analysis: &Analysis, id: i64) -> i64 {
    let tag = node_tag(&analysis.nodes, id);
    if tag == NODE_EXPR {
        return expr_ty_of(&analysis.nodes, id);
    }
    if tag == NODE_PAT {
        return pat_ty_of(&analysis.nodes, id);
    }
    if tag == NODE_TY {
        return ty_key_of(&analysis.nodes, id);
    }
    NONE
}

// The NODE_FN a symbol declares, or NONE.  Fn-item symbols point at their
// item row; method symbols point directly at their fn row.
fn fn_node_of_sym(analysis: &Analysis, sym: i64) -> i64 {
    let decl = node_c(&analysis.nodes, sym);
    if decl == NONE {
        return NONE;
    }
    let tag = node_tag(&analysis.nodes, decl);
    if tag == NODE_FN {
        return decl;
    }
    if tag == NODE_ITEM {
        let kind = node_a(&analysis.nodes, decl);
        if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
            return node_d(&analysis.nodes, decl);
        }
    }
    NONE
}

fn render_ty_syntax(analysis: &Analysis, ty: i64) -> String {
    if node_tag(&analysis.nodes, ty) != NODE_TY {
        return "?".to_string();
    }
    let kind = node_a(&analysis.nodes, ty);
    if kind == TY_NAMED {
        return name_text(&analysis.names, node_b(&analysis.nodes, ty));
    }
    if kind == TY_PATH {
        return path_join(analysis, node_b(&analysis.nodes, ty));
    }
    if kind == TY_GENERIC {
        let base = path_join(analysis, node_b(&analysis.nodes, ty));
        let args = node_c(&analysis.nodes, ty);
        let count = list_len(&analysis.lists, args);
        let mut parts: Vec<String> = Vec::new();
        let mut idx = 0i64;
        while idx < count {
            parts.push(render_ty_syntax(analysis, list_get(&analysis.lists, args, idx)));
            idx += 1;
        }
        return format!("{}({})", base, parts.join(", "));
    }
    if kind == TY_REF {
        return format!("&{}", render_ty_syntax(analysis, node_b(&analysis.nodes, ty)));
    }
    if kind == TY_REF_MUT {
        return format!("&mut {}", render_ty_syntax(analysis, node_b(&analysis.nodes, ty)));
    }
    if kind == TY_SLICE {
        return format!("[{}]", render_ty_syntax(analysis, node_b(&analysis.nodes, ty)));
    }
    if kind == TY_ARRAY {
        return format!(
            "[{}; {}]",
            render_ty_syntax(analysis, node_b(&analysis.nodes, ty)),
            node_c(&analysis.nodes, ty)
        );
    }
    if kind == TY_SELF {
        return "Self".to_string();
    }
    if kind == TY_PARAM {
        return name_text(&analysis.names, node_b(&analysis.nodes, ty));
    }
    "?".to_string()
}

fn path_join(analysis: &Analysis, list: i64) -> String {
    let count = list_len(&analysis.lists, list);
    let mut parts: Vec<String> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        parts.push(name_text(&analysis.names, list_get(&analysis.lists, list, idx)));
        idx += 1;
    }
    parts.join(".")
}

// A declared type rendered from its attached canonical key when the
// typechecker reached it, otherwise from its syntax.
fn render_ty_node(analysis: &Analysis, ty: i64) -> String {
    let key = ty_key_of(&analysis.nodes, ty);
    if key != NONE {
        return render_type_key(&analysis.names, &analysis.nodes, &analysis.lists, key);
    }
    render_ty_syntax(analysis, ty)
}

/// The `fun name(params) [impure] Ret` signature line of a fn node.
pub fn render_fn_signature(analysis: &Analysis, fn_node: i64) -> String {
    let name = name_text(&analysis.names, node_a(&analysis.nodes, fn_node));
    let type_params = node_b(&analysis.nodes, fn_node);
    let mut generics = String::new();
    let tp_count = list_len(&analysis.lists, type_params);
    if tp_count > 0 {
        let mut parts: Vec<String> = Vec::new();
        let mut idx = 0i64;
        while idx < tp_count {
            let param = list_get(&analysis.lists, type_params, idx);
            if node_tag(&analysis.nodes, param) == NODE_TY && node_a(&analysis.nodes, param) == TY_PARAM {
                parts.push(name_text(&analysis.names, node_b(&analysis.nodes, param)));
            }
            idx += 1;
        }
        if !parts.is_empty() {
            generics = format!("<{}>", parts.join(", "));
        }
    }
    let params = param_labels(analysis, fn_node);
    let impure = if node_e(&analysis.nodes, fn_node) == 1 { " impure" } else { "" };
    let ret = render_ty_node(analysis, node_d(&analysis.nodes, fn_node));
    format!("fun {}{}({}){} {}", name, generics, params.join(", "), impure, ret)
}

/// The `name: Type` label of every parameter of a fn node, in order.
pub fn param_labels(analysis: &Analysis, fn_node: i64) -> Vec<String> {
    let params = node_c(&analysis.nodes, fn_node);
    let count = list_len(&analysis.lists, params);
    let mut out: Vec<String> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(&analysis.lists, params, idx);
        let pname = name_text(&analysis.names, node_a(&analysis.nodes, param));
        let pty = render_ty_node(analysis, node_b(&analysis.nodes, param));
        out.push(format!("{}: {}", pname, pty));
        idx += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Hover
// ---------------------------------------------------------------------------

/// Markdown hover text and the span it describes, for the node under the
/// cursor.  Built entirely from attached facts: the resolved symbol and the
/// canonical type key.
pub fn hover(analysis: &Analysis, file: i64, offset: i64) -> Option<(String, (i64, i64, i64))> {
    let id = node_at(analysis, file, offset);
    if id == NONE {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    let sym = sym_of_node(analysis, id);
    if sym != NONE {
        let kind = node_a(&analysis.nodes, sym);
        let qualified = name_text(&analysis.names, node_b(&analysis.nodes, sym));
        let fn_node = fn_node_of_sym(analysis, sym);
        if fn_node != NONE {
            lines.push(format!("```cinnabar\n{}\n```", render_fn_signature(analysis, fn_node)));
        } else {
            lines.push(format!("**{}** `{}`", sym_kind_label(kind), qualified));
        }
    }
    let key = ty_key_of_node(analysis, id);
    if key != NONE {
        lines.push(format!(
            "type: `{}`",
            render_type_key(&analysis.names, &analysis.nodes, &analysis.lists, key)
        ));
        let linear = tyinfo_is_linear(&analysis.nodes, key);
        if linear == 1 {
            lines.push("linear: must be consumed exactly once on every path".to_string());
        }
    }
    if lines.is_empty() {
        return None;
    }
    let span = (
        node_file(&analysis.nodes, id),
        node_start(&analysis.nodes, id),
        node_end(&analysis.nodes, id),
    );
    Some((lines.join("\n\n"), span))
}

fn sym_kind_label(kind: i64) -> String {
    sym_kind_name(kind).to_lowercase().replace('_', " ")
}

// ---------------------------------------------------------------------------
// Definition / references
// ---------------------------------------------------------------------------

fn sym_at(analysis: &Analysis, file: i64, offset: i64) -> i64 {
    let id = node_at(analysis, file, offset);
    if id == NONE {
        return NONE;
    }
    sym_of_node(analysis, id)
}

/// The declaration span of the symbol under the cursor.  Builtins and
/// seeded native declarations have no source declaration and yield None.
pub fn definition(analysis: &Analysis, file: i64, offset: i64) -> Option<(i64, i64, i64)> {
    let sym = sym_at(analysis, file, offset);
    if sym == NONE {
        return None;
    }
    let decl = node_c(&analysis.nodes, sym);
    if decl == NONE {
        return None;
    }
    let decl_file = node_file(&analysis.nodes, decl);
    if decl_file == NO_FILE {
        return None;
    }
    Some((decl_file, node_start(&analysis.nodes, decl), node_end(&analysis.nodes, decl)))
}

/// Every span whose row carries the same resolved symbol as the node under
/// the cursor: uses in expressions, patterns, and type positions, plus the
/// declaration row itself.
pub fn references(analysis: &Analysis, file: i64, offset: i64) -> Vec<(i64, i64, i64)> {
    let sym = sym_at(analysis, file, offset);
    let mut out: Vec<(i64, i64, i64)> = Vec::new();
    if sym == NONE {
        return out;
    }
    let count = node_count(analysis);
    let mut id = 0i64;
    while id < count {
        let tag = node_tag(&analysis.nodes, id);
        let row_sym = if tag == NODE_EXPR {
            expr_sym_of(&analysis.nodes, id)
        } else if tag == NODE_PAT {
            pat_sym_of(&analysis.nodes, id)
        } else if tag == NODE_TY {
            crate::ast::ty_sym_of(&analysis.nodes, id)
        } else if tag == NODE_ITEM {
            item_sym_of(&analysis.nodes, id)
        } else if tag == NODE_VARIANT {
            variant_sym_of(&analysis.nodes, id)
        } else {
            NONE
        };
        if row_sym == sym && node_file(&analysis.nodes, id) != NO_FILE {
            out.push((
                node_file(&analysis.nodes, id),
                node_start(&analysis.nodes, id),
                node_end(&analysis.nodes, id),
            ));
        }
        id += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Completion
// ---------------------------------------------------------------------------

/// Completion labels with a kind code (SYM_* for declared symbols,
/// COMPLETE_* for locals, keywords, and struct fields).
pub fn completions(analysis: &Analysis, file: i64, offset: i64) -> Vec<(String, i64)> {
    let text = file_text_of(analysis, file);
    if offset > 0 {
        let dot = offset - 1;
        let is_dot = match text.as_bytes().get(dot as usize) {
            Some(byte) => *byte == b'.',
            None => false,
        };
        if is_dot {
            let fields = field_completions(analysis, file, dot);
            if !fields.is_empty() {
                return fields;
            }
        }
    }
    scope_completions(analysis, file, offset)
}

// Fields of the struct value ending right before the dot, read from the
// typechecker's NODE_FIELDKEY facts for that struct's canonical key.
fn field_completions(analysis: &Analysis, file: i64, dot: i64) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = Vec::new();
    let count = node_count(analysis);
    let mut best = NONE;
    let mut best_width = i64::MAX;
    let mut id = 0i64;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_EXPR
            && node_file(&analysis.nodes, id) == file
            && node_end(&analysis.nodes, id) == dot
        {
            let width = node_end(&analysis.nodes, id) - node_start(&analysis.nodes, id);
            if width <= best_width {
                best = id;
                best_width = width;
            }
        }
        id += 1;
    }
    if best == NONE {
        return out;
    }
    let mut key = expr_ty_of(&analysis.nodes, best);
    // Field access reads through references; completion does the same.
    let mut guard = 0i64;
    while guard < 8 {
        let row = find_tyinfo(&analysis.nodes, key);
        if row == NONE {
            return out;
        }
        let kind = node_b(&analysis.nodes, row);
        if kind == TYD_REF || kind == TYD_REF_MUT {
            key = node_e(&analysis.nodes, row);
            guard += 1;
            continue;
        }
        if kind != TYD_STRUCT {
            return out;
        }
        break;
    }
    let mut id2 = 0i64;
    while id2 < count {
        if node_tag(&analysis.nodes, id2) == NODE_FIELDKEY && node_a(&analysis.nodes, id2) == key {
            out.push((name_text(&analysis.names, node_b(&analysis.nodes, id2)), COMPLETE_FIELD));
        }
        id2 += 1;
    }
    out
}

fn scope_completions(analysis: &Analysis, file: i64, offset: i64) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = Vec::new();
    // Every declared symbol, by its resolver-qualified name.
    let count = node_count(analysis);
    let mut id = 0i64;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_SYM {
            let name = name_text(&analysis.names, node_b(&analysis.nodes, id));
            if !name.is_empty() {
                push_unique(&mut out, name, node_a(&analysis.nodes, id));
            }
        }
        id += 1;
    }
    // Params and let-bindings of the enclosing function, lexically before
    // the cursor.
    let fn_node = enclosing_fn(analysis, file, offset);
    if fn_node != NONE {
        let params = node_c(&analysis.nodes, fn_node);
        let pcount = list_len(&analysis.lists, params);
        let mut idx = 0i64;
        while idx < pcount {
            let param = list_get(&analysis.lists, params, idx);
            push_unique(
                &mut out,
                name_text(&analysis.names, node_a(&analysis.nodes, param)),
                COMPLETE_LOCAL,
            );
            idx += 1;
        }
        collect_lets(analysis, node_f(&analysis.nodes, fn_node), offset, &mut out);
    }
    let mut kw = 0usize;
    while kw < KEYWORDS.len() {
        match KEYWORDS.get(kw) {
            Some(word) => push_unique(&mut out, word.to_string(), COMPLETE_KEYWORD),
            None => break,
        }
        kw += 1;
    }
    out
}

fn push_unique(out: &mut Vec<(String, i64)>, label: String, kind: i64) {
    if label.is_empty() {
        return;
    }
    let mut idx = 0usize;
    while idx < out.len() {
        match out.get(idx) {
            Some(existing) => {
                if existing.0 == label {
                    return;
                }
            }
            None => break,
        }
        idx += 1;
    }
    out.push((label, kind));
}

fn enclosing_fn(analysis: &Analysis, file: i64, offset: i64) -> i64 {
    let count = node_count(analysis);
    let mut best = NONE;
    let mut best_width = i64::MAX;
    let mut id = 0i64;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_FN && covers(analysis, id, file, offset) {
            let width = node_end(&analysis.nodes, id) - node_start(&analysis.nodes, id);
            if width <= best_width {
                best = id;
                best_width = width;
            }
        }
        id += 1;
    }
    best
}

fn collect_lets(analysis: &Analysis, stmt_list: i64, offset: i64, out: &mut Vec<(String, i64)>) {
    let count = list_len(&analysis.lists, stmt_list);
    let mut idx = 0i64;
    while idx < count {
        let stmt = list_get(&analysis.lists, stmt_list, idx);
        if node_tag(&analysis.nodes, stmt) != NODE_STMT {
            idx += 1;
            continue;
        }
        let kind = node_a(&analysis.nodes, stmt);
        if kind == STMT_LET && node_start(&analysis.nodes, stmt) < offset {
            push_unique(
                out,
                name_text(&analysis.names, node_c(&analysis.nodes, stmt)),
                COMPLETE_LOCAL,
            );
        } else if kind == STMT_WHILE {
            collect_lets(analysis, node_c(&analysis.nodes, stmt), offset, out);
        } else if kind == STMT_IF {
            collect_lets(analysis, node_c(&analysis.nodes, stmt), offset, out);
            if node_d(&analysis.nodes, stmt) != NONE {
                collect_lets(analysis, node_d(&analysis.nodes, stmt), offset, out);
            }
        }
        idx += 1;
    }
}

// ---------------------------------------------------------------------------
// Signature help
// ---------------------------------------------------------------------------

pub struct SignatureInfo {
    pub label: String,
    pub params: Vec<String>,
    pub active: i64,
}

/// Signature help for the innermost call (or struct constructor) containing
/// the cursor.  The active parameter is the count of argument expressions
/// that end before the cursor.
pub fn signature_help(analysis: &Analysis, file: i64, offset: i64) -> Option<SignatureInfo> {
    let count = node_count(analysis);
    let mut best = NONE;
    let mut best_width = i64::MAX;
    let mut id = 0i64;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_EXPR && covers(analysis, id, file, offset) {
            let kind = node_a(&analysis.nodes, id);
            if kind == EXPR_CALL || kind == EXPR_STRUCT_LIT {
                let width = node_end(&analysis.nodes, id) - node_start(&analysis.nodes, id);
                if width <= best_width {
                    best = id;
                    best_width = width;
                }
            }
        }
        id += 1;
    }
    if best == NONE {
        return None;
    }
    let kind = node_a(&analysis.nodes, best);
    if kind == EXPR_CALL {
        let callee = node_b(&analysis.nodes, best);
        let sym = expr_sym_of(&analysis.nodes, callee);
        if sym == NONE {
            return None;
        }
        let fn_node = fn_node_of_sym(analysis, sym);
        if fn_node == NONE {
            return None;
        }
        let args = node_d(&analysis.nodes, best);
        return Some(SignatureInfo {
            label: render_fn_signature(analysis, fn_node),
            params: param_labels(analysis, fn_node),
            active: active_arg(analysis, args, offset),
        });
    }
    // Struct constructor: field labels double as the parameter list.
    let sym = expr_sym_of(&analysis.nodes, best);
    if sym == NONE || node_a(&analysis.nodes, sym) != SYM_STRUCT {
        return None;
    }
    let item = node_c(&analysis.nodes, sym);
    if item == NONE {
        return None;
    }
    let fields = node_e(&analysis.nodes, item);
    let fcount = list_len(&analysis.lists, fields);
    let mut params: Vec<String> = Vec::new();
    let mut idx = 0i64;
    while idx < fcount {
        let field = list_get(&analysis.lists, fields, idx);
        params.push(format!(
            "{}: {}",
            name_text(&analysis.names, node_a(&analysis.nodes, field)),
            render_ty_node(analysis, node_b(&analysis.nodes, field))
        ));
        idx += 1;
    }
    let values = node_d(&analysis.nodes, best);
    let name = name_text(&analysis.names, node_b(&analysis.nodes, sym));
    Some(SignatureInfo {
        label: format!("{}({})", name, params.join(", ")),
        params,
        active: active_arg(analysis, values, offset),
    })
}

fn active_arg(analysis: &Analysis, args: i64, offset: i64) -> i64 {
    let count = list_len(&analysis.lists, args);
    let mut active = 0i64;
    let mut idx = 0i64;
    while idx < count {
        let arg = list_get(&analysis.lists, args, idx);
        if node_end(&analysis.nodes, arg) < offset {
            active = idx + 1;
        }
        idx += 1;
    }
    if active >= count && count > 0 { count - 1 } else { active }
}
