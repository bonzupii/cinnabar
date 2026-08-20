//! Recursive-descent parsing over the lexer's token rows.
//!
//! Hand-rolled, with no parser generator or PEG crate. The token rows the
//! lexer left in the arena are consumed in place and `NODE_ITEM`, `NODE_FN`,
//! `NODE_TY`, `NODE_EXPR`, `NODE_STMT`, and `NODE_PAT` rows are emitted
//! through the small `alloc_*` helpers. Indentation carries no meaning:
//! blocks close with `end`, newlines separate statements, and a multi-line
//! expression is permitted anywhere inside `()`, `[]`, or `{}`.
//!
//! Error recovery is line-based (`recover_line`), so one malformed
//! statement does not abort the file and a single invocation can report
//! many parse errors — which is what makes a file that is mid-edit usable
//! in the editor rather than a wall of one error at a time.
//!
//! `#!` doc comments are attached here, as `NODE_DOC` rows pointing at the
//! item that follows. The API documentation renderer reads those rows
//! rather than re-scanning source text for comment syntax.
//!
//! **Invariants:**
//! - The parser records structure, never meaning. It does not resolve a
//!   name, decide a type, or evaluate a constant: a syntactically valid but
//!   semantically nonsensical program parses cleanly and is rejected by the
//!   stage that owns the fact it violates.
//! - Every row allocated here carries the real span of the source text it
//!   came from; spans start here and are carried forward, never rebuilt.

use crate::ast::*;

pub fn parse(
    names: &mut [String],
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    errors: &mut Vec<Diag>,
    root: i64,
    token_start: i64,
) -> bool {
    let mut pos = token_start;
    while !at_eof(nodes, pos) {
        let docs = take_doc_comments(nodes, lists, &mut pos);
        if at_eof(nodes, pos) {
            break;
        }
        let before = pos;
        match parse_item(&mut pos, names, nodes, lists, errors) {
            Some(item) => {
                list_push(lists, root, item);
                attach_docs(nodes, item, docs);
            }
            None => {
                recover_line(nodes, &mut pos);
            }
        }
        if pos <= before {
            pos += 1;
        }
    }
    errors.is_empty()
}

fn at_eof(nodes: &[i64], pos: i64) -> bool {
    node_a(nodes, pos) == TOK_EOF
}

fn is_name(nodes: &[i64], names: &[String], pos: i64, text: &str) -> bool {
    tok_is_name(nodes, names, pos, text)
}

fn is_sym(nodes: &[i64], names: &[String], pos: i64, text: &str) -> bool {
    tok_is_sym(nodes, names, pos, text)
}

fn is_word(nodes: &[i64], pos: i64) -> bool {
    node_tag(nodes, pos) == NODE_TOKEN && node_a(nodes, pos) == TOK_IDENT
}

fn is_block_open(nodes: &[i64], names: &[String], pos: i64) -> bool {
    is_name(nodes, names, pos, "mod")
        || is_name(nodes, names, pos, "trait")
        || is_name(nodes, names, pos, "impl")
        || is_name(nodes, names, pos, "type")
        || is_name(nodes, names, pos, "fun")
        || is_name(nodes, names, pos, "while")
        || is_name(nodes, names, pos, "if")
        || is_name(nodes, names, pos, "match")
}

fn skip_nl(nodes: &[i64], pos: &mut i64) {
    while node_a(nodes, *pos) == TOK_NL {
        *pos += 1;
    }
}

fn take_doc_comments(nodes: &[i64], lists: &mut Vec<Vec<i64>>, pos: &mut i64) -> i64 {
    let mut docs = NONE;
    loop {
        skip_nl(nodes, pos);
        if node_tag(nodes, *pos) != NODE_TOKEN || node_a(nodes, *pos) != TOK_DOC {
            break;
        }
        if docs == NONE {
            docs = alloc_list(lists);
        }
        list_push(lists, docs, node_b(nodes, *pos));
        *pos += 1;
    }
    docs
}

fn skip_statement_layout(nodes: &[i64], pos: &mut i64) {
    loop {
        skip_nl(nodes, pos);
        if node_tag(nodes, *pos) == NODE_TOKEN && node_a(nodes, *pos) == TOK_DOC {
            *pos += 1;
        } else {
            break;
        }
    }
}

fn attach_docs(nodes: &mut Vec<i64>, target: i64, docs: i64) {
    if docs == NONE {
        return;
    }
    alloc_node(
        nodes,
        &[
            NODE_DOC,
            node_file(nodes, target),
            node_start(nodes, target),
            node_end(nodes, target),
            target,
            docs,
            NONE,
            NONE,
            NONE,
            NONE,
        ],
    );
}

fn tok_text_is(nodes: &[i64], names: &[String], pos: i64, text: &str) -> bool {
    // A keyword and a symbol are both looked up by interned text, but that
    // interning table is shared with string-literal and doc-comment bodies:
    // matching text alone would let a string literal like ")" or "end" be
    // consumed as the punctuation/keyword it merely spells. Every text this
    // is called with is either an all-letter keyword (TOK_IDENT) or pure
    // punctuation (TOK_SYM), so the token kind expected is determined by the
    // text itself.
    let expected_kind = match text.chars().next() {
        Some(c) => {
            if c.is_ascii_alphabetic() {
                TOK_IDENT
            } else {
                TOK_SYM
            }
        }
        None => TOK_SYM,
    };
    node_tag(nodes, pos) == NODE_TOKEN
        && node_a(nodes, pos) == expected_kind
        && name_is(names, node_b(nodes, pos), text)
}

fn accept(nodes: &[i64], names: &[String], pos: &mut i64, text: &str) -> bool {
    if tok_text_is(nodes, names, *pos, text) {
        *pos += 1;
        true
    } else {
        false
    }
}

fn expect(nodes: &[i64], names: &[String], pos: &mut i64, text: &str, errors: &mut Vec<Diag>) -> bool {
    if accept(nodes, names, pos, text) {
        true
    } else {
        let file = node_file(nodes, *pos);
        let start = node_start(nodes, *pos);
        let end = node_end(nodes, *pos);
        push_syntax(errors, &format!("expected '{}'", text), file, start, end);
        false
    }
}

fn recover_line(nodes: &[i64], pos: &mut i64) {
    while !at_eof(nodes, *pos) && node_a(nodes, *pos) != TOK_NL {
        *pos += 1;
    }
    if !at_eof(nodes, *pos) {
        *pos += 1;
    }
}

fn slot(operands: &[i64], idx: usize) -> i64 {
    match operands.get(idx) {
        Some(value) => *value,
        None => NONE,
    }
}

fn alloc_item(nodes: &mut Vec<i64>, kind: i64, file: i64, start: i64, end: i64, is_pub: i64, operands: &[i64]) -> i64 {
    alloc_node(
        nodes,
        &[
            NODE_ITEM, file, start, end, kind, is_pub, NONE,
            slot(operands, 0),
            slot(operands, 1),
            slot(operands, 2),
        ],
    )
}

fn alloc_param(nodes: &mut Vec<i64>, name: i64, file: i64, start: i64, end: i64, ty: i64) -> i64 {
    alloc_node(nodes, &[NODE_PARAM, file, start, end, name, ty, NONE, NONE, NONE, NONE])
}

fn alloc_field(nodes: &mut Vec<i64>, name: i64, file: i64, start: i64, end: i64, ty: i64, is_pub: i64) -> i64 {
    alloc_node(nodes, &[NODE_FIELD, file, start, end, name, ty, is_pub, NONE, NONE, NONE])
}

fn alloc_variant(nodes: &mut Vec<i64>, name: i64, file: i64, start: i64, end: i64, payload: i64, is_pub: i64) -> i64 {
    alloc_node(nodes, &[NODE_VARIANT, file, start, end, name, payload, is_pub, NONE, NONE, NONE])
}

fn alloc_arm(nodes: &mut Vec<i64>, file: i64, start: i64, end: i64, pattern: i64, body: i64) -> i64 {
    alloc_node(nodes, &[NODE_ARM, file, start, end, pattern, body, NONE, NONE, NONE, NONE])
}

fn alloc_ty(nodes: &mut Vec<i64>, kind: i64, file: i64, start: i64, end: i64, operands: &[i64]) -> i64 {
    alloc_node(
        nodes,
        &[
            NODE_TY, file, start, end, kind,
            slot(operands, 0),
            slot(operands, 1),
            NONE, NONE, NONE,
        ],
    )
}

fn alloc_expr(nodes: &mut Vec<i64>, kind: i64, file: i64, start: i64, end: i64, operands: &[i64]) -> i64 {
    alloc_node(
        nodes,
        &[
            NODE_EXPR, file, start, end, kind,
            slot(operands, 0),
            slot(operands, 1),
            slot(operands, 2),
            NONE, NONE,
        ],
    )
}

fn alloc_stmt(nodes: &mut Vec<i64>, kind: i64, file: i64, start: i64, end: i64, operands: &[i64]) -> i64 {
    alloc_node(
        nodes,
        &[
            NODE_STMT, file, start, end, kind,
            slot(operands, 0),
            slot(operands, 1),
            slot(operands, 2),
            slot(operands, 3),
            NONE,
        ],
    )
}

fn alloc_pat(nodes: &mut Vec<i64>, kind: i64, file: i64, start: i64, end: i64, operands: &[i64]) -> i64 {
    alloc_node(
        nodes,
        &[
            NODE_PAT, file, start, end, kind,
            slot(operands, 0),
            slot(operands, 1),
            NONE, NONE, NONE,
        ],
    )
}

fn parse_item(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    if is_name(nodes, names, *pos, "pub") {
        parse_pub_item(pos, names, nodes, lists, errors)
    } else if is_native_keyword(nodes, names, *pos) {
        *pos += 1;
        parse_native_item(pos, names, nodes, lists, errors, 0)
    } else {
        parse_item_body(pos, names, nodes, lists, errors, 0)
    }
}

fn parse_pub_item(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    *pos += 1;
    if is_native_keyword(nodes, names, *pos) {
        *pos += 1;
        parse_native_item(pos, names, nodes, lists, errors, 1)
    } else {
        parse_item_body(pos, names, nodes, lists, errors, 1)
    }
}

fn is_native_keyword(nodes: &[i64], names: &[String], pos: i64) -> bool {
    is_name(nodes, names, pos, "nat")
}

fn parse_native_item(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, is_pub: i64) -> Option<i64> {
    if is_name(nodes, names, *pos, "fun") {
        parse_fun_item(pos, names, nodes, lists, errors, is_pub, 1)
    } else if is_name(nodes, names, *pos, "type") {
        parse_native_type(pos, names, nodes, lists, errors, is_pub)
    } else {
        let file = node_file(nodes, *pos);
        let start = node_start(nodes, *pos);
        let end = node_end(nodes, *pos);
        push_syntax(errors, "native modifier is only allowed on fun and type", file, start, end);
        if is_name(nodes, names, *pos, "mod") || is_name(nodes, names, *pos, "trait") || is_name(nodes, names, *pos, "impl") {
            *pos += 1;
            if node_tag(nodes, *pos) == NODE_TOKEN && node_a(nodes, *pos) == TOK_IDENT {
                *pos += 1;
            }
            skip_nl(nodes, pos);
            let mut depth = 0i64;
            let mut in_trait = 0i64;
            while !at_eof(nodes, *pos) {
                if is_name(nodes, names, *pos, "nat") {
                    *pos += 1;
                    if !at_eof(nodes, *pos) {
                        if is_name(nodes, names, *pos, "trait") {
                            depth += 1;
                            in_trait += 1;
                        }
                        *pos += 1;
                    }
                } else if is_name(nodes, names, *pos, "trait") {
                    depth += 1;
                    in_trait += 1;
                    *pos += 1;
                } else if is_block_open(nodes, names, *pos) {
                    if in_trait == 0 || !is_name(nodes, names, *pos, "fun") {
                        depth += 1;
                    }
                    *pos += 1;
                } else if is_name(nodes, names, *pos, "end") {
                    if depth == 0 {
                        *pos += 1;
                        break;
                    }
                    depth -= 1;
                    if in_trait > 0 {
                        in_trait -= 1;
                    }
                    *pos += 1;
                } else {
                    *pos += 1;
                }
            }
        }
        None
    }
}

fn parse_item_body(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, is_pub: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    if is_name(nodes, names, *pos, "mod") {
        parse_module(pos, names, nodes, lists, errors, is_pub)
    } else if is_name(nodes, names, *pos, "use") {
        parse_use(pos, names, nodes, lists, errors, is_pub)
    } else if is_name(nodes, names, *pos, "type") {
        parse_type_decl(pos, names, nodes, lists, errors, is_pub)
    } else if is_name(nodes, names, *pos, "trait") {
        parse_trait(pos, names, nodes, lists, errors, is_pub)
    } else if is_name(nodes, names, *pos, "impl") {
        parse_impl(pos, names, nodes, lists, errors, is_pub)
    } else if is_name(nodes, names, *pos, "fun") {
        parse_fun_item(pos, names, nodes, lists, errors, is_pub, 0)
    } else if is_name(nodes, names, *pos, "const") {
        parse_const(pos, names, nodes, lists, errors, is_pub)
    } else {
        let end = node_end(nodes, *pos);
        push_syntax(errors, "expected an item declaration", file, start, end);
        None
    }
}

fn parse_module(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, is_pub: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let name = expect_word(pos, nodes, errors)?;
    let children = alloc_list(lists);
    loop {
        let docs = take_doc_comments(nodes, lists, pos);
        if is_name(nodes, names, *pos, "end") || at_eof(nodes, *pos) {
            break;
        }
        match parse_item(pos, names, nodes, lists, errors) {
            Some(item) => {
                list_push(lists, children, item);
                attach_docs(nodes, item, docs);
            }
            None => {
                recover_line(nodes, pos);
            }
        }
    }
    let end = expect_end(pos, names, nodes, errors)?;
    Some(alloc_item(nodes, ITEM_MODULE, file, start, end, is_pub, &[name, children]))
}

fn parse_use(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, is_pub: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let segs = parse_path(pos, names, nodes, lists, errors)?;
    let mut alias = NONE;
    if accept(nodes, names, pos, "as") {
        alias = expect_word(pos, nodes, errors)?;
    }
    let end = node_end(nodes, *pos - 1);
    Some(alloc_item(nodes, ITEM_USE, file, start, end, is_pub, &[segs, alias, NONE]))
}

fn parse_type_decl(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, is_pub: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let name = expect_word(pos, nodes, errors)?;
    let type_params = parse_type_params(pos, names, nodes, lists, errors)?;
    let fields = alloc_list(lists);
    let variants = alloc_list(lists);
    loop {
        let docs = take_doc_comments(nodes, lists, pos);
        if is_name(nodes, names, *pos, "end") || at_eof(nodes, *pos) {
            break;
        }
        let member_pub = if accept(nodes, names, pos, "pub") { 1 } else { 0 };
        let member_name = expect_word(pos, nodes, errors)?;
        let member_start = node_start(nodes, *pos - 1);
        let member_file = file;
        if accept(nodes, names, pos, ":") {
            let ty = parse_type(pos, names, nodes, lists, errors)?;
            let field = alloc_field(nodes, member_name, member_file, member_start, node_end(nodes, ty), ty, member_pub);
            list_push(lists, fields, field);
            attach_docs(nodes, field, docs);
        } else if is_sym(nodes, names, *pos, "(") {
            let payload = parse_payload_types(pos, names, nodes, lists, errors)?;
            let variant = alloc_variant(nodes, member_name, member_file, member_start, node_end(nodes, *pos - 1), payload, member_pub);
            list_push(lists, variants, variant);
            attach_docs(nodes, variant, docs);
        } else {
            let payload = alloc_list(lists);
            let variant = alloc_variant(nodes, member_name, member_file, member_start, node_end(nodes, *pos - 1), payload, member_pub);
            list_push(lists, variants, variant);
            attach_docs(nodes, variant, docs);
        }
    }
    let end = expect_end(pos, names, nodes, errors)?;
    if list_len(lists, fields) > 0 && list_len(lists, variants) > 0 {
        push_syntax(errors, "type cannot mix struct fields and enum variants", file, start, end);
        return None;
    }
    if list_len(lists, variants) > 0 {
        Some(alloc_item(nodes, ITEM_ENUM, file, start, end, is_pub, &[name, variants, type_params]))
    } else {
        Some(alloc_item(nodes, ITEM_STRUCT, file, start, end, is_pub, &[name, fields, type_params]))
    }
}

fn parse_payload_types(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let payload = alloc_list(lists);
    *pos += 1;
    skip_nl(nodes, pos);
    while !is_sym(nodes, names, *pos, ")") && !at_eof(nodes, *pos) {
        let ty = parse_type(pos, names, nodes, lists, errors)?;
        list_push(lists, payload, ty);
        if !accept(nodes, names, pos, ",") {
            break;
        }
        skip_nl(nodes, pos);
    }
    skip_nl(nodes, pos);
    if !expect(nodes, names, pos, ")", errors) {
        return None;
    }
    Some(payload)
}

fn parse_trait(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, is_pub: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let name = expect_word(pos, nodes, errors)?;
    let methods = alloc_list(lists);
    loop {
        let docs = take_doc_comments(nodes, lists, pos);
        if is_name(nodes, names, *pos, "end") || at_eof(nodes, *pos) {
            break;
        }
        accept(nodes, names, pos, "pub");
        if !expect(nodes, names, pos, "fun", errors) {
            recover_line(nodes, pos);
            continue;
        }
        match parse_fun_body_or_sig(pos, names, nodes, lists, errors, 0) {
            Some(fn_id) => {
                list_push(lists, methods, fn_id);
                attach_docs(nodes, fn_id, docs);
            }
            None => {
                recover_line(nodes, pos);
            }
        }
    }
    let end = expect_end(pos, names, nodes, errors)?;
    Some(alloc_item(nodes, ITEM_TRAIT, file, start, end, is_pub, &[name, methods, NONE]))
}

fn parse_impl(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, is_pub: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let trait_segs = parse_path(pos, names, nodes, lists, errors)?;
    if !expect(nodes, names, pos, "for", errors) {
        return None;
    }
    let for_ty = parse_type(pos, names, nodes, lists, errors)?;
    let methods = alloc_list(lists);
    loop {
        let docs = take_doc_comments(nodes, lists, pos);
        if is_name(nodes, names, *pos, "end") || at_eof(nodes, *pos) {
            break;
        }
        accept(nodes, names, pos, "pub");
        if !expect(nodes, names, pos, "fun", errors) {
            recover_line(nodes, pos);
            continue;
        }
        match parse_fun_body_or_sig(pos, names, nodes, lists, errors, 1) {
            Some(fn_id) => {
                list_push(lists, methods, fn_id);
                attach_docs(nodes, fn_id, docs);
            }
            None => {
                recover_line(nodes, pos);
            }
        }
    }
    let end = expect_end(pos, names, nodes, errors)?;
    Some(alloc_item(nodes, ITEM_IMPL, file, start, end, is_pub, &[trait_segs, for_ty, methods]))
}

fn parse_fun_item(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, is_pub: i64, is_native: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let body_required = 1 - is_native;
    let fn_id = parse_fun_body_or_sig(pos, names, nodes, lists, errors, body_required)?;
    let end = node_end(nodes, fn_id);
    let kind = if is_native == 1 { ITEM_NATIVE_FUN } else { ITEM_FUN };
    Some(alloc_item(nodes, kind, file, start, end, is_pub, &[fn_id]))
}

fn parse_const(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, is_pub: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let name = expect_word(pos, nodes, errors)?;
    if !expect(nodes, names, pos, ":", errors) {
        return None;
    }
    let ty = parse_type(pos, names, nodes, lists, errors)?;
    if !expect(nodes, names, pos, "=", errors) {
        return None;
    }
    skip_nl(nodes, pos);
    let value = parse_expr(pos, names, nodes, lists, errors, 0)?;
    let end = node_end(nodes, value);
    Some(alloc_item(nodes, ITEM_CONST, file, start, end, is_pub, &[name, ty, value]))
}

fn parse_native_type(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, is_pub: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let name = expect_word(pos, nodes, errors)?;
    let type_params = parse_type_params(pos, names, nodes, lists, errors)?;
    let end = node_end(nodes, *pos - 1);
    Some(alloc_item(nodes, ITEM_NATIVE_TYPE, file, start, end, is_pub, &[name, type_params]))
}

fn parse_fun_body_or_sig(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, body_required: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    let name = expect_word(pos, nodes, errors)?;
    let type_params = parse_angle_params(pos, names, nodes, lists, errors)?;
    let params = alloc_list(lists);
    if !expect(nodes, names, pos, "(", errors) {
        return None;
    }
    skip_nl(nodes, pos);
    while !is_sym(nodes, names, *pos, ")") && !at_eof(nodes, *pos) {
        let param = parse_param(pos, names, nodes, lists, errors)?;
        list_push(lists, params, param);
        if !accept(nodes, names, pos, ",") {
            break;
        }
        skip_nl(nodes, pos);
    }
    skip_nl(nodes, pos);
    if !expect(nodes, names, pos, ")", errors) {
        return None;
    }
    let mut is_impure = 0;
    if accept(nodes, names, pos, "impure") {
        is_impure = 1;
    }
    let ret_ty = parse_type(pos, names, nodes, lists, errors)?;
    let mut body = NONE;
    let mut end = node_end(nodes, ret_ty);
    if body_required == 1 {
        skip_nl(nodes, pos);
        let block = parse_block(pos, names, nodes, lists, errors, &["end"])?;
        body = block;
        end = expect_end(pos, names, nodes, errors)?;
    }
    Some(alloc_node(nodes, &[NODE_FN, file, start, end, name, type_params, params, ret_ty, is_impure, body]))
}

fn parse_param(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    let name = expect_word(pos, nodes, errors)?;
    if !expect(nodes, names, pos, ":", errors) {
        return None;
    }
    let ty = parse_type(pos, names, nodes, lists, errors)?;
    let end = node_end(nodes, ty);
    Some(alloc_param(nodes, name, file, start, end, ty))
}

fn parse_type_params(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let params = alloc_list(lists);
    if !is_sym(nodes, names, *pos, "(") {
        return Some(params);
    }
    *pos += 1;
    skip_nl(nodes, pos);
    while !is_sym(nodes, names, *pos, ")") && !at_eof(nodes, *pos) {
        let name = expect_word(pos, nodes, errors)?;
        let param = alloc_ty(nodes, TY_PARAM, node_file(nodes, *pos - 1), node_start(nodes, *pos - 1), node_end(nodes, *pos - 1), &[name, NONE]);
        list_push(lists, params, param);
        if !accept(nodes, names, pos, ",") {
            break;
        }
        skip_nl(nodes, pos);
    }
    skip_nl(nodes, pos);
    if !expect(nodes, names, pos, ")", errors) {
        return None;
    }
    Some(params)
}

fn parse_angle_params(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let params = alloc_list(lists);
    if !is_sym(nodes, names, *pos, "<") {
        return Some(params);
    }
    *pos += 1;
    while !is_sym(nodes, names, *pos, ">") && !at_eof(nodes, *pos) {
        let name = expect_word(pos, nodes, errors)?;
        let mut bound = NONE;
        if accept(nodes, names, pos, ":") {
            bound = parse_path(pos, names, nodes, lists, errors)?;
        }
        let param = alloc_ty(nodes, TY_PARAM, node_file(nodes, *pos - 1), node_start(nodes, *pos - 1), node_end(nodes, *pos - 1), &[name, bound]);
        list_push(lists, params, param);
        if !accept(nodes, names, pos, ",") {
            break;
        }
    }
    skip_nl(nodes, pos);
    if !expect(nodes, names, pos, ">", errors) {
        return None;
    }
    Some(params)
}

fn parse_path(pos: &mut i64, names: &[String], nodes: &mut [i64], lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let segs = alloc_list(lists);
    let mut seg = expect_word(pos, nodes, errors)?;
    list_push(lists, segs, seg);
    while accept(nodes, names, pos, ".") {
        seg = expect_word(pos, nodes, errors)?;
        list_push(lists, segs, seg);
    }
    Some(segs)
}

fn expect_word(pos: &mut i64, nodes: &mut [i64], errors: &mut Vec<Diag>) -> Option<i64> {
    if node_tag(nodes, *pos) == NODE_TOKEN && node_a(nodes, *pos) == TOK_IDENT {
        let name = node_b(nodes, *pos);
        *pos += 1;
        Some(name)
    } else {
        let file = node_file(nodes, *pos);
        let start = node_start(nodes, *pos);
        let end = node_end(nodes, *pos);
        push_syntax(errors, "expected a name", file, start, end);
        None
    }
}

fn expect_end(pos: &mut i64, names: &[String], nodes: &mut [i64], errors: &mut Vec<Diag>) -> Option<i64> {
    let end = node_end(nodes, *pos);
    if expect(nodes, names, pos, "end", errors) {
        Some(end)
    } else {
        None
    }
}

fn parse_type(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    if is_sym(nodes, names, *pos, "&") {
        *pos += 1;
        if accept(nodes, names, pos, "mut") {
            parse_ref_tail(pos, names, nodes, lists, errors, (file, start), TY_REF_MUT)
        } else {
            parse_ref_tail(pos, names, nodes, lists, errors, (file, start), TY_REF)
        }
    } else if is_sym(nodes, names, *pos, "[") {
        parse_array_tail(pos, names, nodes, lists, errors, file, start)
    } else if is_name(nodes, names, *pos, "Self") {
        *pos += 1;
        let end = node_end(nodes, *pos - 1);
        Some(alloc_ty(nodes, TY_SELF, file, start, end, &[NONE, NONE]))
    } else if is_word(nodes, *pos) {
        parse_named_type(pos, names, nodes, lists, errors)
    } else {
        let end = node_end(nodes, *pos);
        push_syntax(errors, "expected a type", file, start, end);
        None
    }
}

fn parse_ref_tail(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, span: (i64, i64), kind: i64) -> Option<i64> {
    let (file, start) = span;
    let inner = parse_type(pos, names, nodes, lists, errors)?;
    let end = node_end(nodes, inner);
    Some(alloc_ty(nodes, kind, file, start, end, &[inner, NONE]))
}

fn parse_array_tail(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, file: i64, start: i64) -> Option<i64> {
    *pos += 1;
    let elem = parse_type(pos, names, nodes, lists, errors)?;
    if accept(nodes, names, pos, ";") {
        let len = parse_array_len(pos, nodes, errors)?;
        if !expect(nodes, names, pos, "]", errors) {
            return None;
        }
        let end = node_end(nodes, *pos - 1);
        Some(alloc_ty(nodes, TY_ARRAY, file, start, end, &[elem, len]))
    } else {
        if !expect(nodes, names, pos, "]", errors) {
            return None;
        }
        let end = node_end(nodes, *pos - 1);
        Some(alloc_ty(nodes, TY_SLICE, file, start, end, &[elem, NONE]))
    }
}

fn parse_array_len(pos: &mut i64, nodes: &mut [i64], errors: &mut Vec<Diag>) -> Option<i64> {
    if node_a(nodes, *pos) == TOK_INT {
        let len = node_c(nodes, *pos);
        *pos += 1;
        Some(len)
    } else {
        let file = node_file(nodes, *pos);
        let start = node_start(nodes, *pos);
        let end = node_end(nodes, *pos);
        push_syntax(errors, "expected an array length", file, start, end);
        None
    }
}

fn parse_named_type(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    let segs = parse_path(pos, names, nodes, lists, errors)?;
    let end = node_end(nodes, *pos - 1);
    if is_sym(nodes, names, *pos, "(") {
        let args = parse_payload_types(pos, names, nodes, lists, errors)?;
        let end = node_end(nodes, *pos - 1);
        Some(alloc_ty(nodes, TY_GENERIC, file, start, end, &[segs, args]))
    } else if list_len(lists, segs) == 1 {
        let name = list_first(lists, segs);
        Some(alloc_ty(nodes, TY_NAMED, file, start, end, &[name, NONE]))
    } else {
        Some(alloc_ty(nodes, TY_PATH, file, start, end, &[segs, NONE]))
    }
}

fn parse_block(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, terminators: &[&str]) -> Option<i64> {
    let list = alloc_list(lists);
    skip_statement_layout(nodes, pos);
    while !at_terminator(nodes, names, *pos, terminators) && !at_eof(nodes, *pos) {
        let before = *pos;
        match parse_stmt(pos, names, nodes, lists, errors) {
            Some(stmt) => {
                list_push(lists, list, stmt);
            }
            None => {
                recover_line(nodes, pos);
            }
        }
        skip_statement_layout(nodes, pos);
        if *pos <= before {
            *pos += 1;
        }
    }
    Some(list)
}

fn at_terminator(nodes: &[i64], names: &[String], pos: i64, terminators: &[&str]) -> bool {
    let mut idx = 0usize;
    while idx < terminators.len() {
        let term = match terminators.get(idx) {
            Some(term) => term,
            None => break,
        };
        if is_name(nodes, names, pos, term) {
            return true;
        }
        idx += 1;
    }
    false
}

fn parse_stmt(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    if is_name(nodes, names, *pos, "pub") {
        let file = node_file(nodes, *pos);
        let start = node_start(nodes, *pos);
        let end = node_end(nodes, *pos);
        let next = *pos + 1;
        if is_name(nodes, names, next, "val") || is_name(nodes, names, next, "var") {
            push_syntax(errors, "'pub' modifier is not allowed on local variables", file, start, end);
            return None;
        }
        parse_expr_or_assign(pos, names, nodes, lists, errors)
    } else if is_name(nodes, names, *pos, "val") {
        parse_let(pos, names, nodes, lists, errors, 0)
    } else if is_name(nodes, names, *pos, "var") {
        parse_let(pos, names, nodes, lists, errors, 1)
    } else if is_name(nodes, names, *pos, "while") {
        parse_while(pos, names, nodes, lists, errors)
    } else if is_name(nodes, names, *pos, "if") {
        parse_if(pos, names, nodes, lists, errors)
    } else if is_name(nodes, names, *pos, "return") {
        parse_return(pos, names, nodes, lists, errors)
    } else if is_name(nodes, names, *pos, "break") {
        parse_break(nodes, pos)
    } else if is_name(nodes, names, *pos, "continue") {
        parse_continue(nodes, pos)
    } else {
        parse_expr_or_assign(pos, names, nodes, lists, errors)
    }
}

fn parse_expr_or_assign(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    let target = parse_expr(pos, names, nodes, lists, errors, 0)?;
    if accept(nodes, names, pos, "=") {
        skip_nl(nodes, pos);
        let value = parse_expr(pos, names, nodes, lists, errors, 0)?;
        let end = node_end(nodes, value);
        Some(alloc_stmt(nodes, STMT_ASSIGN, file, start, end, &[target, value, NONE, NONE]))
    } else {
        let end = node_end(nodes, target);
        Some(alloc_stmt(nodes, STMT_EXPR, file, start, end, &[target, NONE, NONE, NONE]))
    }
}

fn parse_let(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, is_mut: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let name = expect_word(pos, nodes, errors)?;
    let mut ty = NONE;
    if accept(nodes, names, pos, ":") {
        ty = parse_type(pos, names, nodes, lists, errors)?;
    }
    if !expect(nodes, names, pos, "=", errors) {
        return None;
    }
    skip_nl(nodes, pos);
    let init = parse_expr(pos, names, nodes, lists, errors, 0)?;
    let end = node_end(nodes, init);
    Some(alloc_stmt(nodes, STMT_LET, file, start, end, &[is_mut, name, ty, init]))
}

fn parse_while(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let cond = parse_expr(pos, names, nodes, lists, errors, 0)?;
    let body = parse_block(pos, names, nodes, lists, errors, &["end"])?;
    let end = expect_end(pos, names, nodes, errors)?;
    Some(alloc_stmt(nodes, STMT_WHILE, file, start, end, &[cond, body, NONE, NONE]))
}

fn parse_if(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let cond = parse_expr(pos, names, nodes, lists, errors, 0)?;
    if node_a(nodes, *pos) != TOK_NL && !at_eof(nodes, *pos) {
        let error_file = node_file(nodes, *pos);
        let error_start = node_start(nodes, *pos);
        let error_end = node_end(nodes, *pos);
        push_syntax(errors, "expected a newline before the if body", error_file, error_start, error_end);
    }
    let then_body = parse_block(pos, names, nodes, lists, errors, &["end", "else", "elif"])?;
    let mut else_body = NONE;
    if is_name(nodes, names, *pos, "elif") {
        let elif_pos = *pos;
        *pos += 1;
        let chain = parse_elif_chain(pos, names, nodes, lists, errors, elif_pos)?;
        else_body = single_stmt_list(lists, chain);
    } else if accept(nodes, names, pos, "else") {
        else_body = parse_block(pos, names, nodes, lists, errors, &["end"])?;
    }
    let end = expect_end(pos, names, nodes, errors)?;
    Some(alloc_stmt(nodes, STMT_IF, file, start, end, &[cond, then_body, else_body, NONE]))
}

fn single_stmt_list(lists: &mut Vec<Vec<i64>>, stmt: i64) -> i64 {
    let list = alloc_list(lists);
    list_push(lists, list, stmt);
    list
}

fn parse_elif_chain(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, elif_pos: i64) -> Option<i64> {
    let file = node_file(nodes, elif_pos);
    let start = node_start(nodes, elif_pos);
    let cond = parse_expr(pos, names, nodes, lists, errors, 0)?;
    if node_a(nodes, *pos) != TOK_NL && !at_eof(nodes, *pos) {
        let error_file = node_file(nodes, *pos);
        let error_start = node_start(nodes, *pos);
        let error_end = node_end(nodes, *pos);
        push_syntax(errors, "expected a newline before the if body", error_file, error_start, error_end);
    }
    let then_body = parse_block(pos, names, nodes, lists, errors, &["end", "else", "elif"])?;
    let mut else_body = NONE;
    if is_name(nodes, names, *pos, "elif") {
        let next_elif = *pos;
        *pos += 1;
        let chain = parse_elif_chain(pos, names, nodes, lists, errors, next_elif)?;
        else_body = single_stmt_list(lists, chain);
    } else if accept(nodes, names, pos, "else") {
        else_body = parse_block(pos, names, nodes, lists, errors, &["end"])?;
    }
    let end = node_end(nodes, *pos - 1);
    Some(alloc_stmt(nodes, STMT_IF, file, start, end, &[cond, then_body, else_body, NONE]))
}

fn parse_return(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let mut value = NONE;
    let mut end = node_end(nodes, *pos - 1);
    if !at_eof(nodes, *pos) && node_a(nodes, *pos) != TOK_NL {
        let expr = parse_expr(pos, names, nodes, lists, errors, 0)?;
        value = expr;
        end = node_end(nodes, expr);
    }
    Some(alloc_stmt(nodes, STMT_RETURN, file, start, end, &[value, NONE, NONE, NONE]))
}

fn parse_break(nodes: &mut Vec<i64>, pos: &mut i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    let end = node_end(nodes, *pos);
    *pos += 1;
    Some(alloc_stmt(nodes, STMT_BREAK, file, start, end, &[NONE, NONE, NONE, NONE]))
}

fn parse_continue(nodes: &mut Vec<i64>, pos: &mut i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    let end = node_end(nodes, *pos);
    *pos += 1;
    Some(alloc_stmt(nodes, STMT_CONTINUE, file, start, end, &[NONE, NONE, NONE, NONE]))
}

fn parse_expr(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, multi: i64) -> Option<i64> {
    parse_binary(pos, names, nodes, lists, errors, 0, multi)
}

fn parse_binary(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, min_prec: i64, multi: i64) -> Option<i64> {
    let mut lhs = parse_unary(pos, names, nodes, lists, errors, multi)?;
    loop {
        if multi == 1 {
            skip_nl(nodes, pos);
        }
        let (op, prec) = match bin_op_at(nodes, names, *pos) {
            Some(pair) => pair,
            None => break,
        };
        if prec < min_prec {
            break;
        }
        *pos += 1;
        let rhs = parse_binary(pos, names, nodes, lists, errors, prec + 1, multi)?;
        let file = node_file(nodes, lhs);
        let start = node_start(nodes, lhs);
        let end = node_end(nodes, rhs);
        lhs = alloc_expr(nodes, EXPR_BINARY, file, start, end, &[op, lhs, rhs]);
    }
    Some(lhs)
}

fn parse_unary(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, multi: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    if is_sym(nodes, names, *pos, "-") {
        *pos += 1;
        let operand = parse_unary(pos, names, nodes, lists, errors, multi)?;
        let end = node_end(nodes, operand);
        Some(alloc_expr(nodes, EXPR_UNARY, file, start, end, &[UN_NEG, operand, NONE]))
    } else if is_sym(nodes, names, *pos, "!") {
        *pos += 1;
        let operand = parse_unary(pos, names, nodes, lists, errors, multi)?;
        let end = node_end(nodes, operand);
        Some(alloc_expr(nodes, EXPR_UNARY, file, start, end, &[UN_NOT, operand, NONE]))
    } else if is_sym(nodes, names, *pos, "&") {
        *pos += 1;
        let op = if accept(nodes, names, pos, "mut") { UN_REF_MUT } else { UN_REF };
        let operand = parse_unary(pos, names, nodes, lists, errors, multi)?;
        let end = node_end(nodes, operand);
        Some(alloc_expr(nodes, EXPR_UNARY, file, start, end, &[op, operand, NONE]))
    } else {
        parse_postfix(pos, names, nodes, lists, errors, multi)
    }
}

fn parse_postfix(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, multi: i64) -> Option<i64> {
    let mut expr = parse_primary(pos, names, nodes, lists, errors, multi)?;
    loop {
        if is_sym(nodes, names, *pos, "(") {
            expr = parse_call_tail(pos, names, nodes, lists, errors, expr, NONE)?;
        } else if is_sym(nodes, names, *pos, "[") {
            let mut scan = *pos + 1;
            let mut depth = 1i64;
            while depth > 0 && !at_eof(nodes, scan) {
                if is_sym(nodes, names, scan, "[") {
                    depth += 1;
                } else if is_sym(nodes, names, scan, "]") {
                    depth -= 1;
                }
                scan += 1;
            }
            if is_sym(nodes, names, scan, "(") {
                let targs = parse_type_args_tail(pos, names, nodes, lists, errors)?;
                expr = parse_call_tail(pos, names, nodes, lists, errors, expr, targs)?;
            } else {
                expr = parse_index_tail(pos, names, nodes, lists, errors, expr)?;
            }
        } else if is_sym(nodes, names, *pos, ".") {
            let file = node_file(nodes, expr);
            let start = node_start(nodes, expr);
            *pos += 1;
            let field = expect_word(pos, nodes, errors)?;
            let end = node_end(nodes, *pos - 1);
            expr = alloc_expr(nodes, EXPR_FIELD_ACCESS, file, start, end, &[expr, field, NONE]);
        } else {
            break;
        }
    }
    Some(expr)
}

fn parse_index_tail(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, base: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    skip_nl(nodes, pos);
    let index = parse_expr(pos, names, nodes, lists, errors, 1)?;
    skip_nl(nodes, pos);
    if !expect(nodes, names, pos, "]", errors) {
        return None;
    }
    let end = node_end(nodes, *pos - 1);
    Some(alloc_expr(nodes, EXPR_INDEX, file, start, end, &[base, index, NONE]))
}

fn parse_primary(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, multi: i64) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    let kind = node_a(nodes, *pos);
    if kind == TOK_INT || kind == TOK_HEX {
        let lit = if kind == TOK_INT { LIT_INT } else { LIT_HEX };
        let value = node_c(nodes, *pos);
        let end = node_end(nodes, *pos);
        *pos += 1;
        Some(alloc_expr(nodes, EXPR_LIT, file, start, end, &[lit, value, NONE]))
    } else if kind == TOK_STRING {
        // The lexer already decoded the escapes and interned the bytes; the
        // literal carries that name id, and no stage re-reads the quoted
        // source text.
        let name = node_b(nodes, *pos);
        let end = node_end(nodes, *pos);
        *pos += 1;
        Some(alloc_expr(nodes, EXPR_LIT, file, start, end, &[LIT_STRING, name, NONE]))
    } else if is_name(nodes, names, *pos, "true") {
        let end = node_end(nodes, *pos);
        *pos += 1;
        Some(alloc_expr(nodes, EXPR_LIT, file, start, end, &[LIT_TRUE, 1, NONE]))
    } else if is_name(nodes, names, *pos, "false") {
        let end = node_end(nodes, *pos);
        *pos += 1;
        Some(alloc_expr(nodes, EXPR_LIT, file, start, end, &[LIT_FALSE, 0, NONE]))
    } else if is_sym(nodes, names, *pos, "(") {
        *pos += 1;
        skip_nl(nodes, pos);
        let inner = parse_expr(pos, names, nodes, lists, errors, 1)?;
        skip_nl(nodes, pos);
        if !expect(nodes, names, pos, ")", errors) {
            return None;
        }
        Some(inner)
    } else if is_sym(nodes, names, *pos, "[") {
        parse_array_lit(pos, names, nodes, lists, errors)
    } else if is_name(nodes, names, *pos, "match") {
        parse_match_expr(pos, names, nodes, lists, errors)
    } else if is_name(nodes, names, *pos, "try") {
        *pos += 1;
        let inner = parse_unary(pos, names, nodes, lists, errors, multi)?;
        let end = node_end(nodes, inner);
        Some(alloc_expr(nodes, EXPR_TRY, file, start, end, &[inner, NONE, NONE]))
    } else if is_word(nodes, *pos) {
        parse_name_chain(pos, names, nodes, lists, errors)
    } else {
        let end = node_end(nodes, *pos);
        push_syntax(errors, "expected an expression", file, start, end);
        None
    }
}

fn parse_array_lit(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let elements = alloc_list(lists);
    skip_nl(nodes, pos);
    while !is_sym(nodes, names, *pos, "]") && !at_eof(nodes, *pos) {
        let element = parse_expr(pos, names, nodes, lists, errors, 1)?;
        list_push(lists, elements, element);
        if !accept(nodes, names, pos, ",") {
            break;
        }
        skip_nl(nodes, pos);
    }
    skip_nl(nodes, pos);
    if !expect(nodes, names, pos, "]", errors) {
        return None;
    }
    let end = node_end(nodes, *pos - 1);
    Some(alloc_expr(nodes, EXPR_ARRAY, file, start, end, &[elements, NONE, NONE]))
}

fn parse_name_chain(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    let segs = parse_path(pos, names, nodes, lists, errors)?;
    let end = node_end(nodes, *pos - 1);
    Some(alloc_expr(nodes, EXPR_PATH, file, start, end, &[segs, NONE, NONE]))
}

fn parse_call_tail(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, callee: i64, targs: i64) -> Option<i64> {
    *pos += 1;
    skip_nl(nodes, pos);
    if is_word(nodes, *pos) && is_sym(nodes, names, *pos + 1, ":") {
        parse_struct_lit_fields(pos, names, nodes, lists, errors, callee)
    } else {
        parse_call_args(pos, names, nodes, lists, errors, callee, targs)
    }
}

fn parse_call_args(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, callee: i64, targs: i64) -> Option<i64> {
    let file = node_file(nodes, callee);
    let start = node_start(nodes, callee);
    let args = alloc_list(lists);
    while !is_sym(nodes, names, *pos, ")") && !at_eof(nodes, *pos) {
        let arg = parse_expr(pos, names, nodes, lists, errors, 1)?;
        list_push(lists, args, arg);
        if !accept(nodes, names, pos, ",") {
            break;
        }
        skip_nl(nodes, pos);
    }
    skip_nl(nodes, pos);
    if !expect(nodes, names, pos, ")", errors) {
        return None;
    }
    let end = node_end(nodes, *pos - 1);
    Some(alloc_expr(nodes, EXPR_CALL, file, start, end, &[callee, targs, args]))
}

fn parse_struct_lit_fields(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, path_expr: i64) -> Option<i64> {
    let file = node_file(nodes, path_expr);
    let start = node_start(nodes, path_expr);
    let segs = node_b(nodes, path_expr);
    let field_names = alloc_list(lists);
    let field_vals = alloc_list(lists);
    while !is_sym(nodes, names, *pos, ")") && !at_eof(nodes, *pos) {
        let field_name = expect_word(pos, nodes, errors)?;
        if !expect(nodes, names, pos, ":", errors) {
            return None;
        }
        skip_nl(nodes, pos);
        let value = parse_expr(pos, names, nodes, lists, errors, 1)?;
        list_push(lists, field_names, field_name);
        list_push(lists, field_vals, value);
        if !accept(nodes, names, pos, ",") {
            break;
        }
        skip_nl(nodes, pos);
    }
    skip_nl(nodes, pos);
    if !expect(nodes, names, pos, ")", errors) {
        return None;
    }
    let end = node_end(nodes, *pos - 1);
    Some(alloc_expr(nodes, EXPR_STRUCT_LIT, file, start, end, &[segs, field_names, field_vals]))
}

fn parse_type_args_tail(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let targs = alloc_list(lists);
    *pos += 1;
    skip_nl(nodes, pos);
    while !is_sym(nodes, names, *pos, "]") && !at_eof(nodes, *pos) {
        let ty = parse_type(pos, names, nodes, lists, errors)?;
        list_push(lists, targs, ty);
        if !accept(nodes, names, pos, ",") {
            break;
        }
        skip_nl(nodes, pos);
    }
    skip_nl(nodes, pos);
    if !expect(nodes, names, pos, "]", errors) {
        return None;
    }
    Some(targs)
}

fn parse_match_expr(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let scrutinee = parse_expr(pos, names, nodes, lists, errors, 0)?;
    let arms = parse_match_arms(pos, names, nodes, lists, errors)?;
    let end = expect_end(pos, names, nodes, errors)?;
    Some(alloc_expr(nodes, EXPR_MATCH, file, start, end, &[scrutinee, arms, NONE]))
}

fn parse_match_arms(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let arms = alloc_list(lists);
    skip_nl(nodes, pos);
    while !is_name(nodes, names, *pos, "end") && !at_eof(nodes, *pos) {
        let file = node_file(nodes, *pos);
        let pattern = parse_pattern(pos, names, nodes, lists, errors)?;
        if !expect(nodes, names, pos, "=>", errors) {
            recover_line(nodes, pos);
            continue;
        }
        skip_nl(nodes, pos);
        match parse_stmt(pos, names, nodes, lists, errors) {
            Some(body) => {
                let start = node_start(nodes, pattern);
                let end = node_end(nodes, body);
                let arm = alloc_arm(nodes, file, start, end, pattern, body);
                list_push(lists, arms, arm);
                let mut lookahead = *pos;
                skip_nl(nodes, &mut lookahead);
                if !at_eof(nodes, lookahead)
                    && !is_name(nodes, names, lookahead, "end")
                    && !match_arm_delimiter_on_line(nodes, names, lookahead)
                {
                    let overflow_file = node_file(nodes, lookahead);
                    let overflow_start = node_start(nodes, lookahead);
                    let overflow_end = node_end(nodes, lookahead);
                    push_syntax(
                        errors,
                        "match arm body must be a single expression; move multi-statement blocks into a helper function",
                        overflow_file,
                        overflow_start,
                        overflow_end,
                    );
                    recover_match_arm(nodes, names, &mut lookahead);
                }
                *pos = lookahead;
            }
            None => {
                recover_match_arm(nodes, names, pos);
            }
        }
        skip_nl(nodes, pos);
    }
    Some(arms)
}

fn match_arm_delimiter_on_line(nodes: &[i64], names: &[String], mut pos: i64) -> bool {
    while !at_eof(nodes, pos) && node_a(nodes, pos) != TOK_NL {
        if is_sym(nodes, names, pos, "=>") {
            return true;
        }
        pos += 1;
    }
    false
}

fn recover_match_arm(nodes: &[i64], names: &[String], pos: &mut i64) {
    loop {
        skip_nl(nodes, pos);
        if at_eof(nodes, *pos) || is_name(nodes, names, *pos, "end") || match_arm_delimiter_on_line(nodes, names, *pos) {
            return;
        }
        recover_line(nodes, pos);
    }
}

fn parse_pattern(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    if is_sym(nodes, names, *pos, "[") {
        parse_array_pattern(pos, names, nodes, lists, errors)
    } else if node_a(nodes, *pos) == TOK_INT || node_a(nodes, *pos) == TOK_HEX {
        let file = node_file(nodes, *pos);
        let start = node_start(nodes, *pos);
        let end = node_end(nodes, *pos);
        let kind = if node_a(nodes, *pos) == TOK_INT { LIT_INT } else { LIT_HEX };
        let value = node_c(nodes, *pos);
        *pos += 1;
        Some(alloc_pat(nodes, PAT_LIT, file, start, end, &[kind, value]))
    } else if is_name(nodes, names, *pos, "true") {
        let file = node_file(nodes, *pos);
        let start = node_start(nodes, *pos);
        let end = node_end(nodes, *pos);
        *pos += 1;
        Some(alloc_pat(nodes, PAT_LIT, file, start, end, &[LIT_TRUE, 1]))
    } else if is_name(nodes, names, *pos, "false") {
        let file = node_file(nodes, *pos);
        let start = node_start(nodes, *pos);
        let end = node_end(nodes, *pos);
        *pos += 1;
        Some(alloc_pat(nodes, PAT_LIT, file, start, end, &[LIT_FALSE, 0]))
    } else if is_word(nodes, *pos) {
        parse_name_pattern(pos, names, nodes, lists, errors)
    } else {
        let file = node_file(nodes, *pos);
        let start = node_start(nodes, *pos);
        let end = node_end(nodes, *pos);
        push_syntax(errors, "expected a pattern", file, start, end);
        None
    }
}

fn parse_name_pattern(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    let segs = parse_path(pos, names, nodes, lists, errors)?;
    let end = node_end(nodes, *pos - 1);
    if is_sym(nodes, names, *pos, "(") {
        let payload = parse_payload_patterns(pos, names, nodes, lists, errors)?;
        let end = node_end(nodes, *pos - 1);
        Some(alloc_pat(nodes, PAT_VARIANT, file, start, end, &[segs, payload]))
    } else if list_len(lists, segs) == 1 {
        let name = list_first(lists, segs);
        Some(alloc_pat(nodes, PAT_BIND, file, start, end, &[name, NONE]))
    } else {
        Some(alloc_pat(nodes, PAT_PATH, file, start, end, &[segs, NONE]))
    }
}

fn parse_payload_patterns(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let payload = alloc_list(lists);
    *pos += 1;
    skip_nl(nodes, pos);
    while !is_sym(nodes, names, *pos, ")") && !at_eof(nodes, *pos) {
        let pattern = parse_pattern(pos, names, nodes, lists, errors)?;
        list_push(lists, payload, pattern);
        if !accept(nodes, names, pos, ",") {
            break;
        }
        skip_nl(nodes, pos);
    }
    skip_nl(nodes, pos);
    if !expect(nodes, names, pos, ")", errors) {
        return None;
    }
    Some(payload)
}

fn parse_array_pattern(pos: &mut i64, names: &[String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>) -> Option<i64> {
    let file = node_file(nodes, *pos);
    let start = node_start(nodes, *pos);
    *pos += 1;
    let elements = alloc_list(lists);
    let mut rest = NONE;
    skip_nl(nodes, pos);
    while !is_sym(nodes, names, *pos, "]") && !at_eof(nodes, *pos) {
        let element = parse_pattern(pos, names, nodes, lists, errors)?;
        if is_sym(nodes, names, *pos, "@") {
            if node_a(nodes, element) != PAT_BIND {
                let file = node_file(nodes, *pos);
                let start = node_start(nodes, *pos);
                let end = node_end(nodes, *pos);
                push_syntax(errors, "rest pattern must bind a name", file, start, end);
                return None;
            }
            *pos += 1;
            if !expect(nodes, names, pos, "..", errors) {
                return None;
            }
            rest = node_b(nodes, element);
            // A rest pattern must be the last element: the fixed prefix ends
            // here.  An optional trailing comma is allowed, but any element
            // after the rest would match at the wrong index.
            if accept(nodes, names, pos, ",") {
                skip_nl(nodes, pos);
            }
            if !expect(nodes, names, pos, "]", errors) {
                return None;
            }
            let end = node_end(nodes, *pos - 1);
            return Some(alloc_pat(nodes, PAT_ARRAY, file, start, end, &[elements, rest]));
        } else {
            list_push(lists, elements, element);
        }
        if !accept(nodes, names, pos, ",") {
            break;
        }
        skip_nl(nodes, pos);
    }
    skip_nl(nodes, pos);
    if !expect(nodes, names, pos, "]", errors) {
        return None;
    }
    let end = node_end(nodes, *pos - 1);
    Some(alloc_pat(nodes, PAT_ARRAY, file, start, end, &[elements, rest]))
}

fn bin_op_at(nodes: &[i64], names: &[String], pos: i64) -> Option<(i64, i64)> {
    logical_op_at(nodes, names, pos)
        .or(comparison_op_at(nodes, names, pos))
        .or(bitwise_op_at(nodes, names, pos))
        .or(shift_op_at(nodes, names, pos))
        .or(arith_op_at(nodes, names, pos))
}

fn logical_op_at(nodes: &[i64], names: &[String], pos: i64) -> Option<(i64, i64)> {
    if is_sym(nodes, names, pos, "||") {
        Some((BIN_OR, 1))
    } else if is_sym(nodes, names, pos, "&&") {
        Some((BIN_AND, 2))
    } else {
        None
    }
}

fn comparison_op_at(nodes: &[i64], names: &[String], pos: i64) -> Option<(i64, i64)> {
    if is_sym(nodes, names, pos, "==") {
        Some((BIN_EQ, 3))
    } else if is_sym(nodes, names, pos, "!=") {
        Some((BIN_NE, 3))
    } else if is_sym(nodes, names, pos, "<") {
        Some((BIN_LT, 3))
    } else if is_sym(nodes, names, pos, ">") {
        Some((BIN_GT, 3))
    } else if is_sym(nodes, names, pos, "<=") {
        Some((BIN_LE, 3))
    } else if is_sym(nodes, names, pos, ">=") {
        Some((BIN_GE, 3))
    } else {
        None
    }
}

fn bitwise_op_at(nodes: &[i64], names: &[String], pos: i64) -> Option<(i64, i64)> {
    if is_sym(nodes, names, pos, "|") {
        Some((BIN_BOR, 4))
    } else if is_sym(nodes, names, pos, "^") {
        Some((BIN_BXOR, 5))
    } else if is_sym(nodes, names, pos, "&") {
        Some((BIN_BAND, 6))
    } else {
        None
    }
}

fn shift_op_at(nodes: &[i64], names: &[String], pos: i64) -> Option<(i64, i64)> {
    if is_sym(nodes, names, pos, "<<") {
        Some((BIN_SHL, 7))
    } else if is_sym(nodes, names, pos, ">>") {
        Some((BIN_SHR, 7))
    } else {
        None
    }
}

fn arith_op_at(nodes: &[i64], names: &[String], pos: i64) -> Option<(i64, i64)> {
    if is_sym(nodes, names, pos, "+") {
        Some((BIN_ADD, 8))
    } else if is_sym(nodes, names, pos, "-") {
        Some((BIN_SUB, 8))
    } else if is_sym(nodes, names, pos, "*") {
        Some((BIN_MUL, 9))
    } else if is_sym(nodes, names, pos, "/") {
        Some((BIN_DIV, 9))
    } else if is_sym(nodes, names, pos, "%") {
        Some((BIN_MOD, 9))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(source: &str) -> Vec<Diag> {
        let mut names: Vec<String> = Vec::new();
        let mut nodes: Vec<i64> = Vec::new();
        let mut lists: Vec<Vec<i64>> = Vec::new();
        let mut errors: Vec<Diag> = Vec::new();
        let ok = crate::lexer::lex(&mut names, &mut nodes, source, 0, &mut errors);
        let root = alloc_list(&mut lists);
        let parsed = parse(&mut names, &mut nodes, &mut lists, &mut errors, root, 0);
        assert!(ok || errors.len() > 0);
        assert!(parsed || errors.len() > 0);
        errors
    }

    #[test]
    fn parses_simple_fun() {
        let errors = parse_all("pub fun add(a: I64, b: I64) I64\n  return a + b\nend\n");
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn parses_match_and_rest() {
        let errors = parse_all(
            "pub fun split_first(view: &[U8]) Option(SplitFirst)\n  match view\n    [] => return None\n    [first, rest @ ..] => return Some(SplitFirst(first: first, rest_len: 3))\n  end\nend\n",
        );
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn parses_generic_nat() {
        let errors = parse_all(
            "pub mod Collections\n  pub nat type Vec(T)\n  pub nat fun vec_new<T>() impure Result(Vec(T), Error)\nend\n",
        );
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn parses_trait_bound() {
        let errors = parse_all("pub trait Checksum\n  pub fun checksum(value: &Self) U32\nend\n\nfun checksum_value<T: Checksum>(value: &T) U32\n  return Checksum.checksum(value)\nend\n");
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn parses_multiline_lists() {
        let errors = parse_all(
            "pub nat fun write_u8(\n  block: &Block,\n  offset: Usize,\n  value: U8\n) impure Result(Unit, Error)\n\nfun make() MagicHeader\n  return MagicHeader(\n    bytes: [MAGIC_BYTE_0, MAGIC_BYTE_1],\n    expected: MAGIC_U32\n  )\nend\n",
        );
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn parses_try_typed_call_then_ref_mut_call() {
        let errors = parse_all(
            "fun vec_demo() impure Result(Unit, Error)\n  val vec = try vec_new[U8]()\n  val push_result = push_all_magic(&mut vec)\n  return Ok(Unit)\nend\n",
        );
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn rejects_mixed_type() {
        let errors = parse_all("pub type BadMixedType\n  pub x: I64\n  pub BadVariant(U32)\nend\n");
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn rejects_nat_on_const() {
        let errors = parse_all("nat const BAD: I64 = 1\n");
        assert_eq!(errors.len(), 1);
    }
}
