// The typed-arena dump behind `--dump-typed-ast`.
//
// This serializes the compiler's entire state after the front-end has run:
// the interning table, the list arena, and every node row with its raw
// slots plus a symbolic annotation of what the row means — including the
// attachments earlier stages computed (resolved symbols, canonical type
// keys, linearity flags, variant tags, field facts).  It prints facts the
// pipeline already attached; it never re-derives them.

use crate::ast::*;
use crate::typecheck::render_type_key;

pub fn dump_typed_arena(names: &[String], nodes: &[i64], lists: &[Vec<i64>]) -> String {
    let mut out = String::new();
    dump_names(names, &mut out);
    dump_lists(lists, &mut out);
    dump_nodes(names, nodes, lists, &mut out);
    out
}

fn dump_names(names: &[String], out: &mut String) {
    out.push_str(&format!("== names ({}) ==\n", names.len()));
    let mut idx = 0usize;
    while idx < names.len() {
        match names.get(idx) {
            Some(text) => out.push_str(&format!("{}: {:?}\n", idx, text)),
            None => break,
        }
        idx += 1;
    }
}

fn dump_lists(lists: &[Vec<i64>], out: &mut String) {
    out.push_str(&format!("== lists ({}) ==\n", lists.len()));
    let mut idx = 0usize;
    while idx < lists.len() {
        match lists.get(idx) {
            Some(items) => {
                let rendered: Vec<String> = items.iter().map(|value| value.to_string()).collect();
                out.push_str(&format!("{}: [{}]\n", idx, rendered.join(", ")));
            }
            None => break,
        }
        idx += 1;
    }
}

fn dump_nodes(names: &[String], nodes: &[i64], lists: &[Vec<i64>], out: &mut String) {
    let count = nodes.len() as i64 / NODE_STRIDE;
    out.push_str(&format!("== nodes ({}) ==\n", count));
    let mut id = 0i64;
    while id < count {
        dump_node_row(names, nodes, lists, id, out);
        id += 1;
    }
}

fn dump_node_row(names: &[String], nodes: &[i64], lists: &[Vec<i64>], id: i64, out: &mut String) {
    let tag = node_tag(nodes, id);
    out.push_str(&format!(
        "#{} {} file={} span={}..{} a={} b={} c={} d={} e={} f={}",
        id,
        tag_name(tag),
        node_file(nodes, id),
        node_start(nodes, id),
        node_end(nodes, id),
        node_a(nodes, id),
        node_b(nodes, id),
        node_c(nodes, id),
        node_d(nodes, id),
        node_e(nodes, id),
        node_f(nodes, id),
    ));
    let detail = row_detail(names, nodes, lists, id, tag);
    if !detail.is_empty() {
        out.push_str(" ; ");
        out.push_str(&detail);
    }
    out.push('\n');
}

fn row_detail(names: &[String], nodes: &[i64], lists: &[Vec<i64>], id: i64, tag: i64) -> String {
    if tag == NODE_TOKEN {
        return token_detail(names, nodes, id);
    }
    if tag == NODE_ITEM {
        return item_detail(names, nodes, id);
    }
    if tag == NODE_FN {
        return format!("fn '{}'", name_text(names, node_a(nodes, id)));
    }
    if tag == NODE_PARAM {
        return format!("param '{}'", name_text(names, node_a(nodes, id)));
    }
    if tag == NODE_FIELD {
        return format!("field '{}'", name_text(names, node_a(nodes, id)));
    }
    if tag == NODE_VARIANT {
        return format!("variant '{}' sym=#{}", name_text(names, node_a(nodes, id)), variant_sym_of(nodes, id));
    }
    if tag == NODE_TY {
        return ty_detail(names, nodes, lists, id);
    }
    if tag == NODE_EXPR {
        return expr_detail(names, nodes, lists, id);
    }
    if tag == NODE_STMT {
        return stmt_detail(names, nodes, lists, id);
    }
    if tag == NODE_PAT {
        return pat_detail(names, nodes, lists, id);
    }
    if tag == NODE_SYM {
        return sym_detail(names, nodes, id);
    }
    if tag == NODE_TYINFO {
        return tyinfo_detail(names, nodes, lists, id);
    }
    if tag == NODE_INST {
        return format!("inst fn=#{} mono-key={}", inst_fn_of(nodes, id), inst_mono_of(nodes, id));
    }
    if tag == NODE_CONSTVAL {
        return format!("constval sym=#{} value={}", node_a(nodes, id), node_b(nodes, id));
    }
    if tag == NODE_DOC {
        return format!("doc target=#{} parts=list#{}", node_a(nodes, id), node_b(nodes, id));
    }
    if tag == NODE_TRAIT {
        return format!(
            "trait-dispatch call=#{} trait-sym=#{} method='{}'",
            node_a(nodes, id),
            trait_call_trait(nodes, id),
            name_text(names, trait_call_method(nodes, id))
        );
    }
    if tag == NODE_VARFACT {
        return format!(
            "varfact enum-key={} variant='{}' tag={}",
            node_a(nodes, id),
            name_text(names, node_b(nodes, id)),
            node_d(nodes, id)
        );
    }
    if tag == NODE_FIELDKEY {
        return format!(
            "fieldkey struct-key={} field='{}' field-key={} decl-idx={}",
            node_a(nodes, id),
            name_text(names, node_b(nodes, id)),
            fieldkey_key_of(nodes, id),
            fieldkey_idx_of(nodes, id)
        );
    }
    String::new()
}

fn token_detail(names: &[String], nodes: &[i64], id: i64) -> String {
    let kind = node_a(nodes, id);
    if kind == TOK_IDENT {
        return format!("tok IDENT '{}'", name_text(names, node_b(nodes, id)));
    }
    if kind == TOK_SYM {
        return format!("tok SYM '{}'", name_text(names, node_b(nodes, id)));
    }
    if kind == TOK_DOC {
        return format!("tok DOC '{}'", name_text(names, node_b(nodes, id)));
    }
    if kind == TOK_INT {
        return format!("tok INT {}", node_c(nodes, id));
    }
    if kind == TOK_HEX {
        return format!("tok HEX {}", node_c(nodes, id));
    }
    if kind == TOK_STRING {
        // The lexer decoded the escapes and interned the bytes, so the
        // token's payload is a name id like an identifier's, not a value.
        return format!("tok STRING \"{}\"", escaped_literal_text(&name_text(names, node_b(nodes, id))));
    }
    if kind == TOK_NL {
        return "tok NL".to_string();
    }
    if kind == TOK_EOF {
        return "tok EOF".to_string();
    }
    "tok ?".to_string()
}

fn item_detail(names: &[String], nodes: &[i64], id: i64) -> String {
    let kind = node_a(nodes, id);
    let sym = item_sym_of(nodes, id);
    let mut text = format!("item {}", item_kind_name(kind));
    if kind == ITEM_MODULE
        || kind == ITEM_STRUCT
        || kind == ITEM_ENUM
        || kind == ITEM_TRAIT
        || kind == ITEM_CONST
        || kind == ITEM_NATIVE_TYPE
    {
        text.push_str(&format!(" '{}'", name_text(names, node_d(nodes, id))));
    }
    if sym != NONE {
        text.push_str(&format!(" sym=#{}", sym));
    }
    text
}

fn ty_detail(names: &[String], nodes: &[i64], lists: &[Vec<i64>], id: i64) -> String {
    let mut text = format!("ty {}", ty_kind_name(node_a(nodes, id)));
    let key = ty_key_of(nodes, id);
    if key != NONE {
        text.push_str(&format!(" key={} '{}'", key, render_type_key(names, nodes, lists, key)));
    }
    let sym = ty_sym_of(nodes, id);
    if sym != NONE {
        text.push_str(&format!(" sym=#{}", sym));
    }
    text
}

fn expr_detail(names: &[String], nodes: &[i64], lists: &[Vec<i64>], id: i64) -> String {
    let mut text = format!("expr {}", expr_kind_name(node_a(nodes, id)));
    let ty = expr_ty_of(nodes, id);
    if ty != NONE {
        text.push_str(&format!(" ty={} '{}'", ty, render_type_key(names, nodes, lists, ty)));
    }
    let sym = expr_sym_of(nodes, id);
    if sym != NONE {
        text.push_str(&format!(" sym=#{} '{}'", sym, name_text(names, node_b(nodes, sym))));
    }
    text
}

fn stmt_detail(names: &[String], nodes: &[i64], lists: &[Vec<i64>], id: i64) -> String {
    let mut text = format!("stmt {}", stmt_kind_name(node_a(nodes, id)));
    let ty = stmt_ty_of(nodes, id);
    if ty != NONE {
        text.push_str(&format!(" ty={} '{}'", ty, render_type_key(names, nodes, lists, ty)));
    }
    text
}

fn pat_detail(names: &[String], nodes: &[i64], lists: &[Vec<i64>], id: i64) -> String {
    let mut text = format!("pat {}", pat_kind_name(node_a(nodes, id)));
    let ty = pat_ty_of(nodes, id);
    if ty != NONE {
        text.push_str(&format!(" ty={} '{}'", ty, render_type_key(names, nodes, lists, ty)));
    }
    let sym = pat_sym_of(nodes, id);
    if sym != NONE {
        text.push_str(&format!(" sym=#{}", sym));
    }
    text
}

fn sym_detail(names: &[String], nodes: &[i64], id: i64) -> String {
    format!(
        "sym {} '{}' decl=#{}",
        sym_kind_name(node_a(nodes, id)),
        name_text(names, node_b(nodes, id)),
        node_c(nodes, id)
    )
}

fn tyinfo_detail(names: &[String], nodes: &[i64], lists: &[Vec<i64>], id: i64) -> String {
    let key = node_a(nodes, id);
    format!(
        "tyinfo key={} kind={} linear={} '{}'",
        key,
        tyd_kind_name(node_b(nodes, id)),
        node_file(nodes, id),
        render_type_key(names, nodes, lists, key)
    )
}

fn tag_name(tag: i64) -> &'static str {
    if tag == NODE_TOKEN {
        "TOKEN"
    } else if tag == NODE_ITEM {
        "ITEM"
    } else if tag == NODE_FN {
        "FN"
    } else if tag == NODE_PARAM {
        "PARAM"
    } else if tag == NODE_FIELD {
        "FIELD"
    } else if tag == NODE_VARIANT {
        "VARIANT"
    } else if tag == NODE_ARM {
        "ARM"
    } else if tag == NODE_TY {
        "TY"
    } else if tag == NODE_EXPR {
        "EXPR"
    } else if tag == NODE_STMT {
        "STMT"
    } else if tag == NODE_PAT {
        "PAT"
    } else if tag == NODE_SYM {
        "SYM"
    } else if tag == NODE_TYINFO {
        "TYINFO"
    } else if tag == NODE_INST {
        "INST"
    } else if tag == NODE_CONSTVAL {
        "CONSTVAL"
    } else if tag == NODE_DOC {
        "DOC"
    } else if tag == NODE_TRAIT {
        "TRAIT"
    } else if tag == NODE_VARFACT {
        "VARFACT"
    } else if tag == NODE_FIELDKEY {
        "FIELDKEY"
    } else {
        "?TAG"
    }
}

fn item_kind_name(kind: i64) -> &'static str {
    if kind == ITEM_MODULE {
        "MODULE"
    } else if kind == ITEM_USE {
        "USE"
    } else if kind == ITEM_STRUCT {
        "STRUCT"
    } else if kind == ITEM_ENUM {
        "ENUM"
    } else if kind == ITEM_TRAIT {
        "TRAIT"
    } else if kind == ITEM_IMPL {
        "IMPL"
    } else if kind == ITEM_FUN {
        "FUN"
    } else if kind == ITEM_NATIVE_FUN {
        "NATIVE_FUN"
    } else if kind == ITEM_CONST {
        "CONST"
    } else if kind == ITEM_NATIVE_TYPE {
        "NATIVE_TYPE"
    } else {
        "?ITEM"
    }
}

fn ty_kind_name(kind: i64) -> &'static str {
    if kind == TY_NAMED {
        "NAMED"
    } else if kind == TY_PATH {
        "PATH"
    } else if kind == TY_GENERIC {
        "GENERIC"
    } else if kind == TY_REF {
        "REF"
    } else if kind == TY_REF_MUT {
        "REF_MUT"
    } else if kind == TY_SLICE {
        "SLICE"
    } else if kind == TY_ARRAY {
        "ARRAY"
    } else if kind == TY_SELF {
        "SELF"
    } else if kind == TY_PARAM {
        "PARAM"
    } else {
        "?TY"
    }
}

pub fn expr_kind_name(kind: i64) -> &'static str {
    if kind == EXPR_LIT {
        "LIT"
    } else if kind == EXPR_PATH {
        "PATH"
    } else if kind == EXPR_UNARY {
        "UNARY"
    } else if kind == EXPR_BINARY {
        "BINARY"
    } else if kind == EXPR_CALL {
        "CALL"
    } else if kind == EXPR_STRUCT_LIT {
        "STRUCT_LIT"
    } else if kind == EXPR_ARRAY {
        "ARRAY"
    } else if kind == EXPR_MATCH {
        "MATCH"
    } else if kind == EXPR_TRY {
        "TRY"
    } else if kind == EXPR_INDEX {
        "INDEX"
    } else if kind == EXPR_FIELD_ACCESS {
        "FIELD_ACCESS"
    } else {
        "?EXPR"
    }
}

fn stmt_kind_name(kind: i64) -> &'static str {
    if kind == STMT_LET {
        "LET"
    } else if kind == STMT_ASSIGN {
        "ASSIGN"
    } else if kind == STMT_WHILE {
        "WHILE"
    } else if kind == STMT_IF {
        "IF"
    } else if kind == STMT_RETURN {
        "RETURN"
    } else if kind == STMT_BREAK {
        "BREAK"
    } else if kind == STMT_CONTINUE {
        "CONTINUE"
    } else if kind == STMT_EXPR {
        "EXPR"
    } else {
        "?STMT"
    }
}

fn pat_kind_name(kind: i64) -> &'static str {
    if kind == PAT_BIND {
        "BIND"
    } else if kind == PAT_PATH {
        "PATH"
    } else if kind == PAT_VARIANT {
        "VARIANT"
    } else if kind == PAT_ARRAY {
        "ARRAY"
    } else if kind == PAT_LIT {
        "LIT"
    } else {
        "?PAT"
    }
}

pub fn sym_kind_name(kind: i64) -> &'static str {
    if kind == SYM_MODULE {
        "MODULE"
    } else if kind == SYM_STRUCT {
        "STRUCT"
    } else if kind == SYM_ENUM {
        "ENUM"
    } else if kind == SYM_TRAIT {
        "TRAIT"
    } else if kind == SYM_TYPE {
        "TYPE"
    } else if kind == SYM_VARIANT {
        "VARIANT"
    } else if kind == SYM_FUN {
        "FUN"
    } else if kind == SYM_NATIVE_FUN {
        "NATIVE_FUN"
    } else if kind == SYM_CONST {
        "CONST"
    } else if kind == SYM_IMPL_METHOD {
        "IMPL_METHOD"
    } else if kind == SYM_TRAIT_METHOD {
        "TRAIT_METHOD"
    } else {
        "?SYM"
    }
}

fn tyd_kind_name(kind: i64) -> &'static str {
    if kind == TYD_UNKNOWN {
        "UNKNOWN"
    } else if kind == TYD_BUILTIN {
        "BUILTIN"
    } else if kind == TYD_STRUCT {
        "STRUCT"
    } else if kind == TYD_ENUM {
        "ENUM"
    } else if kind == TYD_NATIVE {
        "NATIVE"
    } else if kind == TYD_REF {
        "REF"
    } else if kind == TYD_REF_MUT {
        "REF_MUT"
    } else if kind == TYD_SLICE {
        "SLICE"
    } else if kind == TYD_ARRAY {
        "ARRAY"
    } else if kind == TYD_PARAM {
        "PARAM"
    } else if kind == TYD_MONO {
        "MONO"
    } else {
        "?TYD"
    }
}
