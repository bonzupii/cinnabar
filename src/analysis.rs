//! IDE-facing queries over the compiler's attached facts.
//!
//! `analyze` runs the standard front-end pipeline (load -> resolve ->
//! typecheck -> borrow) over a file set — optionally overlaid with unsaved
//! editor buffers — and answers position queries by reading the
//! attachments the pipeline already computed: resolved symbol ids on
//! expression, pattern, and type rows; canonical type keys; field facts.
//!
//! Hover reads the attached type key, the resolved signature, and the
//! linearity flag. Definition and references follow resolver symbol ids
//! across the module graph. Completion reads resolver-attached
//! `NODE_SCOPEFACT` visibility rows, typechecker-attached `NODE_LOCALFACT`
//! lexical snapshots, and `NODE_FIELDKEY` facts for members after a `.`.
//! Byte-offset to line/UTF-16-column mapping lives here too, since that is
//! the boundary where the compiler's spans meet the protocol's positions.
//!
//! **Invariants:**
//! - No name is re-resolved and no type re-inferred here, ever. A question
//!   the attachments cannot answer is answered with "nothing" rather than
//!   with a second implementation that could drift from the first.
//! - Answering "nothing" is a real answer, not a failure to handle a case.
//!   An editor showing no hover is correct where a build would also have
//!   no fact; an editor showing a *different* answer than the build would
//!   be the bug this rule exists to prevent.

use crate::ast::*;
use crate::inspect::sym_kind_name;
use crate::target::Target;
use crate::typecheck::render_type_key;
use crate::{borrow, module_loader, resolver, typecheck};

pub struct Analysis {
    pub target: Target,
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
pub fn analyze(entry_path: &str, overlay: &[(String, String)], target: &Target) -> Analysis {
    let mut names: Vec<String> = Vec::new();
    let mut nodes: Vec<i64> = Vec::new();
    let mut lists: Vec<Vec<i64>> = Vec::new();
    let mut errors: Vec<Diag> = Vec::new();
    let mut notes: Vec<Note> = Vec::new();
    let (loaded, files) =
        module_loader::load_with_overlay(&mut names, &mut nodes, &mut lists, &mut errors, entry_path, overlay);
    if !errors.is_empty() {
        return Analysis {
            target: *target,
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
    let (root, ext_mods) = match loaded {
        Some(program) => program,
        None => {
            return Analysis {
                target: *target,
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
    let mut deferred: Vec<Diag> = Vec::new();
    let mut seeds = Seeds::new();
    let resolved = resolver::resolve(
        &mut names,
        &mut nodes,
        &mut lists,
        resolver::Diagnostics { errors: &mut errors, notes: &mut notes, deferred: &mut deferred, target },
        root,
        &ext_mods,
        &mut seeds,
    );
    let mut typechecked = false;
    let mut impls_list = NONE;
    if resolved {
        let mut check = CheckContext { errors: &mut errors, notes: &mut notes, seeds: &seeds, target };
        let (ok, impls) = typecheck::typecheck(&mut names, &mut nodes, &mut lists, &mut check, root, &ext_mods);
        impls_list = impls;
        typechecked = true;
        if ok {
            borrow::borrow_check(&mut names, &mut nodes, &mut lists, &mut check, root, &ext_mods);
        }
    }
    // Unused items are reported only once the stages that can explain a
    // broken program have run. A file with a type error is told about the
    // type error, not that the functions containing it are unreachable.
    if errors.is_empty() {
        errors.append(&mut deferred);
    }
    Analysis {
        target: *target,
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

// ---------------------------------------------------------------------------
// Name spans
// ---------------------------------------------------------------------------

fn is_ident_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

/// The span of `name`'s first whole-word occurrence within `[start, end)` of
/// `file`'s text, or `None` when the exact text can't be pinpointed there.
///
/// Several node kinds' own `(start, end)` cover more than just the name --
/// an item's span runs from its keyword through `end`, a variant's span
/// grows to include a payload -- and there is no separate name-only span
/// stored anywhere for those (see the parser's node-layout comments). This
/// recovers one by search rather than by modeling every kind's slot layout,
/// and callers that cannot tolerate an imprecise fallback (rename) treat
/// `None` as "refuse" rather than as "use the wider span".
fn locate_name(analysis: &Analysis, file: i64, start: i64, end: i64, name: &str) -> Option<(i64, i64)> {
    if name.is_empty() || start < 0 || end < start {
        return None;
    }
    let text = file_text_of(analysis, file);
    let haystack = text.get(start as usize..end as usize)?;
    let hay = haystack.as_bytes();
    let needle = name.as_bytes();
    if needle.len() > hay.len() {
        return None;
    }
    let mut idx = 0usize;
    while idx + needle.len() <= hay.len() {
        if &hay[idx..idx + needle.len()] == needle {
            let before_ok = idx == 0 || !is_ident_byte(hay[idx - 1]);
            let after = idx + needle.len();
            let after_ok = after >= hay.len() || !is_ident_byte(hay[after]);
            if before_ok && after_ok {
                return Some((start + idx as i64, start + after as i64));
            }
        }
        idx += 1;
    }
    None
}

// The short (undotted) declared name of a symbol, read from its own
// declaration row rather than the symbol's possibly-qualified display name
// (the `hover`/`sym_kind_label` name can read `Memory.allocate`; the source
// token at every use site is always just `allocate`).
fn short_name_of_sym(analysis: &Analysis, sym: i64) -> Option<String> {
    let decl = node_c(&analysis.nodes, sym);
    if decl == NONE {
        return None;
    }
    let tag = node_tag(&analysis.nodes, decl);
    if tag == NODE_FN {
        return Some(name_text(&analysis.names, node_a(&analysis.nodes, decl)));
    }
    if tag == NODE_ITEM {
        let kind = node_a(&analysis.nodes, decl);
        if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
            let fn_node = node_d(&analysis.nodes, decl);
            return Some(name_text(&analysis.names, node_a(&analysis.nodes, fn_node)));
        }
        return Some(name_text(&analysis.names, node_d(&analysis.nodes, decl)));
    }
    if tag == NODE_VARIANT {
        return Some(name_text(&analysis.names, node_a(&analysis.nodes, decl)));
    }
    None
}

// ---------------------------------------------------------------------------
// Folding ranges
// ---------------------------------------------------------------------------

/// Byte spans of every foldable block in `file`: item bodies (module,
/// struct, enum, trait, impl), function bodies (top-level and, since a
/// method carries no wrapping item, `NODE_FN` directly), `while`/`if`
/// bodies, `match` expressions, and -- via a source scan, since a plain
/// comment keeps no parse-tree span at all -- `#|...|#`/`#!|...|#` comment
/// blocks. A bodyless declaration (a native fn/type, or a trait method
/// signature) has no interior lines and folds nothing.
pub fn folding_ranges(analysis: &Analysis, file: i64) -> Vec<(i64, i64)> {
    let count = node_count(analysis);
    let mut out: Vec<(i64, i64)> = Vec::new();
    let mut id = 0i64;
    while id < count {
        if node_file(&analysis.nodes, id) == file {
            let tag = node_tag(&analysis.nodes, id);
            let foldable = if tag == NODE_ITEM {
                let kind = node_a(&analysis.nodes, id);
                kind == ITEM_MODULE
                    || kind == ITEM_STRUCT
                    || kind == ITEM_ENUM
                    || kind == ITEM_TRAIT
                    || kind == ITEM_IMPL
            } else if tag == NODE_FN {
                true
            } else if tag == NODE_STMT {
                let kind = node_a(&analysis.nodes, id);
                kind == STMT_WHILE || kind == STMT_IF
            } else if tag == NODE_EXPR {
                node_a(&analysis.nodes, id) == EXPR_MATCH
            } else {
                false
            };
            if foldable {
                let start = node_start(&analysis.nodes, id);
                let end = node_end(&analysis.nodes, id);
                if end > start {
                    out.push((start, end));
                }
            }
        }
        id += 1;
    }
    out.extend(comment_block_spans(&file_text_of(analysis, file)));
    out
}

// Plain `#|...|#`/`#!|...|#` block comments produce no token and no parse
// span: the lexer scans past their text and discards it. Comments do not
// nest in Cinnabar, so a plain byte scan for the next `|#` after an opener
// is exact -- no stack, no ambiguity, unlike trying to fold the surrounding
// keyword structure from source text (which cannot tell a bodyless
// declaration from one with a body; that is why the block above reads real
// parse-tree spans instead).
fn comment_block_spans(text: &str) -> Vec<(i64, i64)> {
    let bytes = text.as_bytes();
    let mut out: Vec<(i64, i64)> = Vec::new();
    let mut idx = 0usize;
    while idx < bytes.len() {
        let is_doc = bytes[idx] == b'#' && bytes.get(idx + 1) == Some(&b'!') && bytes.get(idx + 2) == Some(&b'|');
        let is_plain = !is_doc && bytes[idx] == b'#' && bytes.get(idx + 1) == Some(&b'|');
        if is_doc || is_plain {
            let opener_len = if is_doc { 3 } else { 2 };
            let mut cursor = idx + opener_len;
            let mut closed: Option<usize> = None;
            while cursor + 1 < bytes.len() {
                if bytes[cursor] == b'|' && bytes[cursor + 1] == b'#' {
                    closed = Some(cursor + 2);
                    break;
                }
                cursor += 1;
            }
            match closed {
                Some(close) => {
                    out.push((idx as i64, close as i64));
                    idx = close;
                    continue;
                }
                None => break,
            }
        }
        idx += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Document / workspace symbols
// ---------------------------------------------------------------------------

/// One entry in a document's or workspace's symbol outline. `span` is the
/// whole declaration; `selection_span` is just the name where `locate_name`
/// can pinpoint it, falling back to `span` otherwise -- an outline entry
/// that highlights slightly more than the name is a cosmetic gap, not a
/// wrong answer the way the same imprecision would be for `rename`.
pub struct SymbolInfo {
    pub name: String,
    pub detail: String,
    pub kind: i64,
    pub span: (i64, i64, i64),
    pub selection_span: (i64, i64, i64),
    pub children: Vec<SymbolInfo>,
}

// LSP SymbolKind numbers -- a third numbering distinct from this compiler's
// own SYM_* table and from CompletionItemKind (cinnabar_lsp.rs).
pub const SYMBOL_KIND_MODULE: i64 = 2;
pub const SYMBOL_KIND_NAMESPACE: i64 = 3;
pub const SYMBOL_KIND_CLASS: i64 = 5;
pub const SYMBOL_KIND_METHOD: i64 = 6;
pub const SYMBOL_KIND_FIELD: i64 = 8;
pub const SYMBOL_KIND_ENUM: i64 = 10;
pub const SYMBOL_KIND_INTERFACE: i64 = 11;
pub const SYMBOL_KIND_FUNCTION: i64 = 12;
pub const SYMBOL_KIND_CONSTANT: i64 = 14;
pub const SYMBOL_KIND_ENUM_MEMBER: i64 = 22;
pub const SYMBOL_KIND_STRUCT: i64 = 23;

fn symbol_kind_for_item(kind: i64) -> i64 {
    if kind == ITEM_MODULE {
        SYMBOL_KIND_MODULE
    } else if kind == ITEM_STRUCT {
        SYMBOL_KIND_STRUCT
    } else if kind == ITEM_ENUM {
        SYMBOL_KIND_ENUM
    } else if kind == ITEM_TRAIT {
        SYMBOL_KIND_INTERFACE
    } else if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
        SYMBOL_KIND_FUNCTION
    } else if kind == ITEM_CONST {
        SYMBOL_KIND_CONSTANT
    } else if kind == ITEM_NATIVE_TYPE {
        SYMBOL_KIND_CLASS
    } else {
        SYMBOL_KIND_MODULE
    }
}

fn ids_of_list(analysis: &Analysis, list: i64) -> Vec<i64> {
    let count = list_len(&analysis.lists, list);
    let mut out: Vec<i64> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        out.push(list_get(&analysis.lists, list, idx));
        idx += 1;
    }
    out
}

fn selection_span(analysis: &Analysis, file: i64, start: i64, end: i64, name: &str) -> (i64, i64, i64) {
    match locate_name(analysis, file, start, end, name) {
        Some((s, e)) => (file, s, e),
        None => (file, start, end),
    }
}

fn symbol_for_fn(analysis: &Analysis, fn_node: i64, kind: i64) -> SymbolInfo {
    let file = node_file(&analysis.nodes, fn_node);
    let start = node_start(&analysis.nodes, fn_node);
    let end = node_end(&analysis.nodes, fn_node);
    let name = name_text(&analysis.names, node_a(&analysis.nodes, fn_node));
    SymbolInfo {
        selection_span: selection_span(analysis, file, start, end, &name),
        detail: render_fn_signature(analysis, fn_node),
        name,
        kind,
        span: (file, start, end),
        children: Vec::new(),
    }
}

// `None` only for `ITEM_USE`: an import is not a declaration, so it has no
// outline entry.
fn symbol_for_item(analysis: &Analysis, item: i64) -> Option<SymbolInfo> {
    let kind = node_a(&analysis.nodes, item);
    if kind == ITEM_USE {
        return None;
    }
    let file = node_file(&analysis.nodes, item);
    let start = node_start(&analysis.nodes, item);
    let end = node_end(&analysis.nodes, item);
    let span = (file, start, end);

    if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
        let fn_node = node_d(&analysis.nodes, item);
        return Some(symbol_for_fn(analysis, fn_node, symbol_kind_for_item(kind)));
    }
    if kind == ITEM_IMPL {
        let trait_name = path_join(analysis, node_d(&analysis.nodes, item));
        let target = render_ty_node(analysis, node_e(&analysis.nodes, item));
        let name = if trait_name.is_empty() {
            format!("impl {}", target)
        } else {
            format!("impl {} for {}", trait_name, target)
        };
        let children: Vec<SymbolInfo> = ids_of_list(analysis, node_f(&analysis.nodes, item))
            .into_iter()
            .map(|fn_node| symbol_for_fn(analysis, fn_node, SYMBOL_KIND_METHOD))
            .collect();
        return Some(SymbolInfo {
            name,
            detail: String::new(),
            kind: SYMBOL_KIND_NAMESPACE,
            span,
            selection_span: span,
            children,
        });
    }

    let name = name_text(&analysis.names, node_d(&analysis.nodes, item));
    let selection = selection_span(analysis, file, start, end, &name);
    let mut children: Vec<SymbolInfo> = Vec::new();

    if kind == ITEM_MODULE {
        for child in ids_of_list(analysis, node_e(&analysis.nodes, item)) {
            if let Some(info) = symbol_for_item(analysis, child) {
                children.push(info);
            }
        }
    } else if kind == ITEM_STRUCT {
        for field in ids_of_list(analysis, node_e(&analysis.nodes, item)) {
            let fname = name_text(&analysis.names, node_a(&analysis.nodes, field));
            let ffile = node_file(&analysis.nodes, field);
            let fstart = node_start(&analysis.nodes, field);
            let fend = node_end(&analysis.nodes, field);
            children.push(SymbolInfo {
                selection_span: selection_span(analysis, ffile, fstart, fend, &fname),
                name: fname,
                detail: render_ty_node(analysis, node_b(&analysis.nodes, field)),
                kind: SYMBOL_KIND_FIELD,
                span: (ffile, fstart, fend),
                children: Vec::new(),
            });
        }
    } else if kind == ITEM_ENUM {
        for variant in ids_of_list(analysis, node_e(&analysis.nodes, item)) {
            let vname = name_text(&analysis.names, node_a(&analysis.nodes, variant));
            let vfile = node_file(&analysis.nodes, variant);
            let vstart = node_start(&analysis.nodes, variant);
            let vend = node_end(&analysis.nodes, variant);
            children.push(SymbolInfo {
                selection_span: selection_span(analysis, vfile, vstart, vend, &vname),
                name: vname,
                detail: String::new(),
                kind: SYMBOL_KIND_ENUM_MEMBER,
                span: (vfile, vstart, vend),
                children: Vec::new(),
            });
        }
    } else if kind == ITEM_TRAIT {
        for fn_node in ids_of_list(analysis, node_e(&analysis.nodes, item)) {
            children.push(symbol_for_fn(analysis, fn_node, SYMBOL_KIND_METHOD));
        }
    }

    Some(SymbolInfo {
        name,
        detail: String::new(),
        kind: symbol_kind_for_item(kind),
        span,
        selection_span: selection,
        children,
    })
}

/// Every top-level declaration in `file`, nested hierarchically (a module's
/// children, a struct's fields, an enum's variants, a trait's or impl's
/// methods). Imports are not declarations and are excluded.
pub fn document_symbols(analysis: &Analysis, file: i64) -> Vec<SymbolInfo> {
    let count = node_count(analysis);
    let mut nested: Vec<i64> = Vec::new();
    let mut id = 0i64;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_ITEM
            && node_file(&analysis.nodes, id) == file
            && node_a(&analysis.nodes, id) == ITEM_MODULE
        {
            nested.extend(ids_of_list(analysis, node_e(&analysis.nodes, id)));
        }
        id += 1;
    }
    let mut out: Vec<SymbolInfo> = Vec::new();
    id = 0i64;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_ITEM
            && node_file(&analysis.nodes, id) == file
            && !nested.contains(&id)
            && let Some(info) = symbol_for_item(analysis, id)
        {
            out.push(info);
        }
        id += 1;
    }
    out
}

fn push_if_matches(out: &mut Vec<SymbolInfo>, info: SymbolInfo, needle: &str) {
    if needle.is_empty() || info.name.to_lowercase().contains(needle) {
        out.push(info);
    }
}

/// Every declaration across every loaded file whose name contains `query`
/// (case-insensitive; an empty query matches everything), flattened -- a
/// workspace outline has no per-file nesting the way a document outline
/// does. Struct fields and enum variants are reached only through a
/// document outline's tree, not listed here on their own.
pub fn workspace_symbols(analysis: &Analysis, query: &str) -> Vec<SymbolInfo> {
    let needle = query.to_lowercase();
    let count = node_count(analysis);
    let mut wrapped_fns: Vec<i64> = Vec::new();
    let mut id = 0i64;
    while id < count {
        if node_tag(&analysis.nodes, id) == NODE_ITEM {
            let kind = node_a(&analysis.nodes, id);
            if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
                wrapped_fns.push(node_d(&analysis.nodes, id));
            }
        }
        id += 1;
    }
    let mut out: Vec<SymbolInfo> = Vec::new();
    id = 0i64;
    while id < count {
        let tag = node_tag(&analysis.nodes, id);
        if tag == NODE_ITEM && node_file(&analysis.nodes, id) != NO_FILE {
            if let Some(info) = symbol_for_item(analysis, id) {
                push_if_matches(&mut out, info, &needle);
            }
        } else if tag == NODE_FN && node_file(&analysis.nodes, id) != NO_FILE && !wrapped_fns.contains(&id) {
            // Bare method rows: top-level functions are already covered
            // above through their wrapping `ITEM_FUN`/`ITEM_NATIVE_FUN`.
            let info = symbol_for_fn(analysis, id, SYMBOL_KIND_METHOD);
            push_if_matches(&mut out, info, &needle);
        }
        id += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Rename
// ---------------------------------------------------------------------------

/// The identifier-only span of the symbol under the cursor, or `None` when
/// there is nothing there `rename_edits` could safely act on.
pub fn prepare_rename(analysis: &Analysis, file: i64, offset: i64) -> Option<(i64, i64, i64)> {
    let sym = sym_at(analysis, file, offset);
    if sym == NONE {
        return None;
    }
    let name = short_name_of_sym(analysis, sym)?;
    let id = node_at(analysis, file, offset);
    if id == NONE {
        return None;
    }
    let start = node_start(&analysis.nodes, id);
    let end = node_end(&analysis.nodes, id);
    locate_name(analysis, file, start, end, &name).map(|(s, e)| (file, s, e))
}

/// Every occurrence of the symbol under the cursor, each narrowed from its
/// enclosing node span down to the identifier's own span by a whole-word
/// text search, paired with `new_name`. Returns `None` -- rather than a
/// partial edit -- if the symbol's short name can't be read, if it has no
/// occurrences, or if any occurrence's exact name text can't be pinpointed.
/// Struct fields are never covered: `references` never resolves them, since
/// a field carries no `SYM_*` symbol (see the module doc).
pub fn rename_edits(
    analysis: &Analysis,
    file: i64,
    offset: i64,
    new_name: &str,
) -> Option<Vec<(i64, i64, i64, String)>> {
    let sym = sym_at(analysis, file, offset);
    if sym == NONE {
        return None;
    }
    let short = short_name_of_sym(analysis, sym)?;
    if short.is_empty() {
        return None;
    }
    let occurrences = references(analysis, file, offset);
    if occurrences.is_empty() {
        return None;
    }
    let mut out: Vec<(i64, i64, i64, String)> = Vec::new();
    for (occ_file, start, end) in occurrences {
        let (name_start, name_end) = locate_name(analysis, occ_file, start, end, &short)?;
        out.push((occ_file, name_start, name_end, new_name.to_string()));
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Semantic tokens
// ---------------------------------------------------------------------------

/// Standard LSP semantic token type names, in legend order -- the index
/// into this array is what `semantic_tokens` encodes per token.
pub const SEMANTIC_TOKEN_TYPES: &[&str] =
    &["namespace", "type", "enumMember", "function", "method", "variable"];

fn semantic_token_type(kind: i64) -> Option<i64> {
    if kind == SYM_MODULE {
        Some(0)
    } else if kind == SYM_STRUCT || kind == SYM_ENUM || kind == SYM_TRAIT || kind == SYM_TYPE {
        Some(1)
    } else if kind == SYM_VARIANT {
        Some(2)
    } else if kind == SYM_FUN || kind == SYM_NATIVE_FUN {
        Some(3)
    } else if kind == SYM_IMPL_METHOD || kind == SYM_TRAIT_METHOD {
        Some(4)
    } else if kind == SYM_CONST {
        Some(5)
    } else {
        None
    }
}

/// Every symbol-resolved occurrence in `file` as `(start, end, token_type)`
/// triples in source order, ready for delta-encoding. Locals, parameters,
/// and struct fields carry no `SYM_*` symbol (see the module doc on
/// `references`) and so are not classified here -- they keep whatever the
/// grammar already highlights them as, rather than this layer guessing.
pub fn semantic_tokens(analysis: &Analysis, file: i64) -> Vec<(i64, i64, i64)> {
    let count = node_count(analysis);
    let mut out: Vec<(i64, i64, i64)> = Vec::new();
    let mut id = 0i64;
    while id < count {
        if node_file(&analysis.nodes, id) == file {
            let tag = node_tag(&analysis.nodes, id);
            let sym = sym_of_node(analysis, id);
            if sym != NONE {
                let kind = node_a(&analysis.nodes, sym);
                if let Some(token_type) = semantic_token_type(kind) {
                    let raw_start = node_start(&analysis.nodes, id);
                    let raw_end = node_end(&analysis.nodes, id);
                    // Item and variant spans run wider than their name (see
                    // `locate_name`'s doc); expression, pattern, and type
                    // spans already are the name.
                    let span = if tag == NODE_ITEM || tag == NODE_VARIANT {
                        short_name_of_sym(analysis, sym)
                            .and_then(|name| locate_name(analysis, file, raw_start, raw_end, &name))
                    } else {
                        Some((raw_start, raw_end))
                    };
                    if let Some((start, end)) = span
                        && end > start
                    {
                        out.push((start, end, token_type));
                    }
                }
            }
        }
        id += 1;
    }
    out.sort_by_key(|entry| entry.0);
    out
}

// ---------------------------------------------------------------------------
// Inlay hints
// ---------------------------------------------------------------------------

/// `: Type` hints for every `val`/`var` binding in `file` with no explicit
/// type annotation, read from the typechecker's own attached inference --
/// never re-inferred here (see the module doc). A binding the typechecker
/// never reached (its canonical type key does not resolve) is skipped
/// rather than shown with a guessed or garbled type. Each hint pairs the
/// byte offset it anchors to (immediately after the bound name) with the
/// label text to insert there.
pub fn inlay_hints(analysis: &Analysis, file: i64) -> Vec<(i64, String)> {
    if !analysis.typechecked {
        return Vec::new();
    }
    let count = node_count(analysis);
    let mut out: Vec<(i64, String)> = Vec::new();
    let mut id = 0i64;
    while id < count {
        if node_file(&analysis.nodes, id) == file
            && node_tag(&analysis.nodes, id) == NODE_STMT
            && node_a(&analysis.nodes, id) == STMT_LET
            && node_d(&analysis.nodes, id) == NONE
        {
            let key = stmt_ty_of(&analysis.nodes, id);
            if key != NONE && find_tyinfo(&analysis.nodes, key) != NONE {
                let name = name_text(&analysis.names, node_c(&analysis.nodes, id));
                let start = node_start(&analysis.nodes, id);
                let end = node_end(&analysis.nodes, id);
                if let Some(name_span) = locate_name(analysis, file, start, end, &name) {
                    let rendered = render_type_key(&analysis.names, &analysis.nodes, &analysis.lists, key);
                    out.push((name_span.1, format!(": {}", rendered)));
                }
            }
        }
        id += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Code actions
// ---------------------------------------------------------------------------

/// One mechanically-generated fix: a title and the `(file, start, end,
/// replacement)` edits it applies. Every fix is derived from the structured
/// `DiagKind` a diagnostic carries; a kind with no mechanical fix produces
/// none rather than guessing from rendered prose.
pub struct CodeFix {
    pub title: String,
    pub edits: Vec<(i64, i64, i64, String)>,
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

// Splits an identifier into words at underscores and camel/Pascal humps, so
// any of the three casings can be rebuilt from any other.
fn split_words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut idx = 0usize;
    while idx < chars.len() {
        let ch = chars[idx];
        if ch == '_' {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
        } else if ch.is_uppercase() && !current.is_empty() {
            let prev = chars[idx - 1];
            let next_lower = chars.get(idx + 1).map(|c| c.is_lowercase()).unwrap_or(false);
            if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower) {
                words.push(current.clone());
                current.clear();
            }
            current.push(ch);
        } else {
            current.push(ch);
        }
        idx += 1;
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn casing_fix_name(names: &[String], name: i64, expected: i64) -> Option<String> {
    let words = split_words(&name_text(names, name));
    if words.is_empty() {
        return None;
    }
    let rule = casing_rule_name(expected);
    if rule == "snake_case" {
        Some(words.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join("_"))
    } else if rule == "PascalCase" {
        Some(words.iter().map(|w| capitalize(w)).collect::<Vec<_>>().join(""))
    } else if rule == "SCREAMING_SNAKE_CASE" {
        Some(words.iter().map(|w| w.to_uppercase()).collect::<Vec<_>>().join("_"))
    } else {
        None
    }
}

fn delete_span_with_newline(analysis: &Analysis, file: i64, start: i64, end: i64) -> (i64, i64, i64, String) {
    let text = file_text_of(analysis, file);
    let bytes = text.as_bytes();
    let mut extended_end = end;
    if bytes.get(extended_end as usize) == Some(&b'\n') {
        extended_end += 1;
    } else if bytes.get(extended_end as usize) == Some(&b'\r')
        && bytes.get((extended_end + 1) as usize) == Some(&b'\n')
    {
        extended_end += 2;
    }
    (file, start, extended_end, String::new())
}

// The declaring `NODE_ITEM` a use site's symbol resolves to, if any -- only
// item-kind declarations carry an `is_pub` flag (a bare method's `NODE_FN`
// row does not), so a private *method* offers no "make public" fix here.
fn add_pub_fix(analysis: &Analysis, sym: i64) -> Option<(i64, i64, i64, String)> {
    if sym == NONE {
        return None;
    }
    let decl = node_c(&analysis.nodes, sym);
    if decl == NONE || node_tag(&analysis.nodes, decl) != NODE_ITEM {
        return None;
    }
    if item_is_pub(&analysis.nodes, decl) != 0 {
        return None;
    }
    let decl_file = node_file(&analysis.nodes, decl);
    let start = node_start(&analysis.nodes, decl);
    Some((decl_file, start, start, "pub ".to_string()))
}

// The short name an import item binds (its alias, or the last path
// segment), the same name `check_unused` reports.
fn import_name_of(analysis: &Analysis, item: i64) -> String {
    let alias = node_e(&analysis.nodes, item);
    let name = if alias != NONE { alias } else { list_last(&analysis.lists, node_d(&analysis.nodes, item)) };
    name_text(&analysis.names, name)
}

/// Fixes for diagnostics in `file` overlapping `[range_start, range_end)`,
/// derived from each diagnostic's `DiagKind`: remove an unused import,
/// remove an unused declaration, fix casing (built on `rename_edits`, so it
/// shares its all-or-nothing guarantee), and make an item public
/// (declarations only, not methods).
pub fn code_actions(analysis: &Analysis, file: i64, range_start: i64, range_end: i64) -> Vec<CodeFix> {
    let mut out: Vec<CodeFix> = Vec::new();
    for diag in &analysis.errors {
        if diag.file != file || diag.end < range_start || diag.start > range_end {
            continue;
        }
        if let DiagKind::UnusedImport(item) = &diag.kind {
            out.push(CodeFix {
                title: format!("Remove unused import '{}'", import_name_of(analysis, *item)),
                edits: vec![delete_span_with_newline(analysis, file, diag.start, diag.end)],
            });
            continue;
        }
        if let DiagKind::UnusedDeclaration(sym) = &diag.kind {
            let name = short_name_of_sym(analysis, *sym).unwrap_or_default();
            out.push(CodeFix {
                title: format!("Remove unused declaration '{}'", name),
                edits: vec![delete_span_with_newline(analysis, file, diag.start, diag.end)],
            });
            continue;
        }
        if let DiagKind::CasingViolation { name, expected } = &diag.kind {
            if let Some(fixed) = casing_fix_name(&analysis.names, *name, *expected)
                && let Some(edits) = rename_edits(analysis, file, diag.start, &fixed)
            {
                out.push(CodeFix {
                    title: format!("Rename '{}' to '{}'", name_text(&analysis.names, *name), fixed),
                    edits,
                });
            }
            continue;
        }
        if let DiagKind::PrivateAccess { sym } = &diag.kind
            && let Some(edit) = add_pub_fix(analysis, *sym)
        {
            let name = short_name_of_sym(analysis, *sym).unwrap_or_default();
            out.push(CodeFix { title: format!("Make '{}' public", name), edits: vec![edit] });
        }
    }
    out
}
