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
    let tail: &str = text.get(line_start as usize..).unwrap_or_default();
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
    let tail: &str = text.get(line_start as usize..).unwrap_or_default();
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

/// The smallest expression, pattern, type, item, or variant node covering
/// (file, offset). Item and variant nodes let the cursor land on a
/// declaration's own name (e.g. hovering `SERVER_PORT` in its `const`
/// declaration) and still resolve a symbol, matching the coverage
/// `references` already scans for. When the offset sits just past a node
/// (the cursor at the end of an identifier), the position one byte back is
/// also tried.
pub fn node_at(analysis: &Analysis, file: i64, offset: i64) -> i64 {
    let tags = [NODE_EXPR, NODE_PAT, NODE_TY, NODE_ITEM, NODE_VARIANT];
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
    if tag == NODE_ITEM {
        return item_sym_of(&analysis.nodes, id);
    }
    if tag == NODE_VARIANT {
        return variant_sym_of(&analysis.nodes, id);
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
    if tag == NODE_ITEM && node_a(&analysis.nodes, id) == ITEM_CONST {
        let ty_node = node_e(&analysis.nodes, id);
        if ty_node == NONE {
            return NONE;
        }
        return ty_key_of(&analysis.nodes, ty_node);
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
    let use_items = use_path_completions(analysis, file, offset);
    if !use_items.is_empty() {
        return use_items;
    }
    let qualified = qualified_completions(analysis, file, offset);
    if !qualified.is_empty() {
        return qualified;
    }
    scope_completions(analysis, file, offset)
}

fn scope_at(analysis: &Analysis, file: i64, offset: i64) -> i64 {
    let count = node_count(analysis);
    let mut best_scope = 0i64;
    let mut best_width = i64::MAX;
    let mut id = 0i64;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_SCOPEFACT && node_a(&analysis.nodes, id) == SCOPE_AT {
            let source = node_b(&analysis.nodes, id);
            if covers(analysis, source, file, offset) {
                let width = node_end(&analysis.nodes, source) - node_start(&analysis.nodes, source);
                if width <= best_width {
                    best_scope = node_c(&analysis.nodes, id);
                    best_width = width;
                }
            }
        }
        id += 1;
    }
    best_scope
}

fn use_path_completions(analysis: &Analysis, file: i64, offset: i64) -> Vec<(String, i64)> {
    let text = file_text_of(analysis, file);
    let end = if offset < 0 {
        0usize
    } else if offset as usize > text.len() {
        text.len()
    } else {
        offset as usize
    };
    let before = match text.get(0..end) {
        Some(value) => value,
        None => return Vec::new(),
    };
    let line = match before.rsplit('\n').next() {
        Some(value) => value.trim_start(),
        None => return Vec::new(),
    };
    let path = match line.strip_prefix("use ") {
        Some(value) => value.trim(),
        None => return Vec::new(),
    };
    let parts: Vec<&str> = path.split('.').collect();
    let scope = scope_at(analysis, file, offset);
    let count = node_count(analysis);
    if parts.len() <= 1 {
        let typed = match parts.first() {
            Some(value) => *value,
            None => "",
        };
        let mut roots: Vec<(String, i64)> = Vec::new();
        let mut id = 0i64;
        while id < count {
            if node_tag(&analysis.nodes, id) == NODE_SCOPEFACT
                && node_a(&analysis.nodes, id) == SCOPE_VISIBLE
                && node_b(&analysis.nodes, id) == scope
            {
                let label = name_text(&analysis.names, node_c(&analysis.nodes, id));
                let sym = node_d(&analysis.nodes, id);
                if label.starts_with(typed) && node_a(&analysis.nodes, sym) == SYM_MODULE {
                    push_unique(&mut roots, label, SYM_MODULE);
                }
            }
            id += 1;
        }
        return roots;
    }

    dotted_path_completions(analysis, file, offset, &parts)
}

// Members of the module a dotted path names, filtered by the partial final
// segment.  Shared by `use` lines and by qualified paths in ordinary code so
// both resolve through the same scope facts, rather than two lookups that can
// disagree about what a module exports.
fn dotted_path_completions(
    analysis: &Analysis,
    file: i64,
    offset: i64,
    parts: &[&str],
) -> Vec<(String, i64)> {
    let scope = scope_at(analysis, file, offset);
    let count = node_count(analysis);
    let first = match parts.first() {
        Some(value) => *value,
        None => return Vec::new(),
    };
    let first_name = find_name(&analysis.names, first);
    let mut module_sym = NONE;
    let mut id = 0i64;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_SCOPEFACT
            && node_a(&analysis.nodes, id) == SCOPE_VISIBLE
            && node_b(&analysis.nodes, id) == scope
            && node_c(&analysis.nodes, id) == first_name
        {
            let candidate = node_d(&analysis.nodes, id);
            if node_a(&analysis.nodes, candidate) == SYM_MODULE {
                module_sym = candidate;
                break;
            }
        }
        id += 1;
    }
    if module_sym == NONE {
        return Vec::new();
    }
    let mut segment = 1usize;
    while segment + 1 < parts.len() {
        let wanted = match parts.get(segment) {
            Some(value) => find_name(&analysis.names, value),
            None => return Vec::new(),
        };
        let member_scope = node_e(&analysis.nodes, module_sym);
        let mut next = NONE;
        let mut row = 0i64;
        while row < count {
            if node_tag(&analysis.nodes, row) == NODE_SCOPEFACT
                && node_a(&analysis.nodes, row) == SCOPE_MEMBER
                && node_b(&analysis.nodes, row) == scope
                && node_c(&analysis.nodes, row) == member_scope
                && node_d(&analysis.nodes, row) == wanted
            {
                let candidate = node_e(&analysis.nodes, row);
                if node_a(&analysis.nodes, candidate) == SYM_MODULE {
                    next = candidate;
                    break;
                }
            }
            row += 1;
        }
        if next == NONE {
            return Vec::new();
        }
        module_sym = next;
        segment += 1;
    }
    let typed = match parts.last() {
        Some(value) => *value,
        None => "",
    };
    let member_scope = node_e(&analysis.nodes, module_sym);
    let mut out: Vec<(String, i64)> = Vec::new();
    id = 0;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_SCOPEFACT
            && node_a(&analysis.nodes, id) == SCOPE_MEMBER
            && node_b(&analysis.nodes, id) == scope
            && node_c(&analysis.nodes, id) == member_scope
        {
            let label = name_text(&analysis.names, node_d(&analysis.nodes, id));
            let sym = node_e(&analysis.nodes, id);
            if label.starts_with(typed) {
                push_unique(&mut out, label, node_a(&analysis.nodes, sym));
            }
        }
        id += 1;
    }
    out
}
// The dotted path immediately left of the cursor, anywhere an expression or a
// type may appear: `Memory.` offers that module's members, `Memory.all`
// narrows them.  Without this, a dot that is not a struct field access falls
// through to whole-scope completion, which offers keywords like `fun` and
// `if` in a position where no keyword can legally follow.
//
// `use` lines keep their own entry point because a bare `use F` must also
// suggest module roots, which is meaningless mid-expression.
fn qualified_completions(analysis: &Analysis, file: i64, offset: i64) -> Vec<(String, i64)> {
    let text = file_text_of(analysis, file);
    let end = if offset < 0 {
        0usize
    } else if offset as usize > text.len() {
        text.len()
    } else {
        offset as usize
    };
    let before = match text.get(0..end) {
        Some(value) => value,
        None => return Vec::new(),
    };
    // Walk back over path characters only.  Every byte tested is ASCII, so the
    // resulting index is always on a character boundary.
    let bytes = before.as_bytes();
    let mut start = end;
    while start > 0 {
        let byte = match bytes.get(start - 1) {
            Some(value) => *value,
            None => break,
        };
        if byte == b'.' || byte == b'_' || byte.is_ascii_alphanumeric() {
            start -= 1;
            continue;
        }
        break;
    }
    let token = match before.get(start..end) {
        Some(value) => value,
        None => return Vec::new(),
    };
    // No dot means an unqualified name, which scope completion already covers.
    if !token.contains('.') {
        return Vec::new();
    }
    let parts: Vec<&str> = token.split('.').collect();
    dotted_path_completions(analysis, file, offset, &parts)
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
    // Only symbols the resolver attached as visible from the cursor's
    // precise scope.  Names and visibility are not reconstructed here.
    let count = node_count(analysis);
    let scope = scope_at(analysis, file, offset);
    let mut id = 0i64;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_SCOPEFACT
            && node_a(&analysis.nodes, id) == SCOPE_VISIBLE
            && node_b(&analysis.nodes, id) == scope
        {
            let name = name_text(&analysis.names, node_c(&analysis.nodes, id));
            let sym = node_d(&analysis.nodes, id);
            if !name.is_empty() {
                push_unique(&mut out, name, node_a(&analysis.nodes, sym));
            }
        }
        id += 1;
    }
    append_local_completions(analysis, file, offset, &mut out);
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

fn append_local_completions(
    analysis: &Analysis,
    file: i64,
    offset: i64,
    out: &mut Vec<(String, i64)>,
) {
    let count = node_count(analysis);
    let mut best_source = NONE;
    let mut best_width = i64::MAX;
    let mut id = 0i64;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_LOCALFACT
            && node_file(&analysis.nodes, id) == file
            && node_start(&analysis.nodes, id) <= offset
            && offset <= node_end(&analysis.nodes, id)
        {
            let width = node_end(&analysis.nodes, id) - node_start(&analysis.nodes, id);
            if width <= best_width {
                best_source = node_a(&analysis.nodes, id);
                best_width = width;
            }
        }
        id += 1;
    }
    if best_source == NONE {
        return;
    }
    id = 0;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_LOCALFACT
            && node_a(&analysis.nodes, id) == best_source
        {
            push_unique(
                out,
                name_text(&analysis.names, node_b(&analysis.nodes, id)),
                COMPLETE_LOCAL,
            );
        }
        id += 1;
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
