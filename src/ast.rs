//! Cinnabar compiler — single flat arena.
//!
//! The whole compiler is written as small functions over one flat node
//! arena: a `Vec<i64>` of fixed-width records, each of `NODE_STRIDE`
//! slots.  Trees are stored as integer ids; there are no pointers, no
//! boxes, no per-node allocation.  This is exactly the shape a Cinnabar
//! program would use to hold a tree: a `Vec(Int)` arena with stride and
//! tag constants.
//!
//! A node row is `(tag, file, start, end, a, b, c, d, e, f)`.  The tag
//! selects the node kind; the meaning of `a`..`f` depends on the kind.
//! `file`, `start`, `end` are the node's real source span.
//!
//! Facts are attached once by the stage that owns them (resolved symbol
//! ids by the resolver, inferred type ids by the typechecker) and
//! consumed, never recomputed, downstream.
//!
//! This module defines only constants and functions.

/// (message, file, start, end).  A file id below zero means the fact has
/// no Cinnabar source origin (an internal invariant failure or a
/// toolchain failure) and is rendered without a source label.
pub type Diag = (String, i64, i64, i64);

pub const NONE: i64 = -1;
pub const NO_FILE: i64 = -1;

// ---------------------------------------------------------------------------
// Node arena.
// ---------------------------------------------------------------------------

pub const NODE_STRIDE: i64 = 10;
pub const NODE_TAG: i64 = 0;
pub const NODE_FILE: i64 = 1;
pub const NODE_START: i64 = 2;
pub const NODE_END: i64 = 3;
pub const NODE_A: i64 = 4;
pub const NODE_B: i64 = 5;
pub const NODE_C: i64 = 6;
pub const NODE_D: i64 = 7;
pub const NODE_E: i64 = 8;
pub const NODE_F: i64 = 9;

// Node tags.
pub const NODE_TOKEN: i64 = 0;
pub const NODE_ITEM: i64 = 1;
pub const NODE_FN: i64 = 2;
pub const NODE_PARAM: i64 = 3;
pub const NODE_FIELD: i64 = 4;
pub const NODE_VARIANT: i64 = 5;
pub const NODE_ARM: i64 = 6;
pub const NODE_TY: i64 = 7;
pub const NODE_EXPR: i64 = 8;
pub const NODE_STMT: i64 = 9;
pub const NODE_PAT: i64 = 10;
pub const NODE_SYM: i64 = 11;
pub const NODE_TYINFO: i64 = 12;
pub const NODE_INST: i64 = 13;
pub const NODE_CONSTVAL: i64 = 14;

// ---------------------------------------------------------------------------
// Token rows.  (tag=NODE_TOKEN, a=kind, b=name, c=value).
// ---------------------------------------------------------------------------

pub const TOK_IDENT: i64 = 0; // identifier or keyword, by interned name
pub const TOK_INT: i64 = 1; // decimal integer literal
pub const TOK_HEX: i64 = 2; // hexadecimal literal (0x...)
pub const TOK_EOF: i64 = 3;
pub const TOK_NL: i64 = 4; // newline (statement boundary)
pub const TOK_SYM: i64 = 5; // operator or punctuation symbol, by interned name

// ---------------------------------------------------------------------------
// Item rows.  (tag=NODE_ITEM, a=kind, b=is_pub, c=sym, d..f kind-specific).
// ---------------------------------------------------------------------------

pub const ITEM_MODULE: i64 = 0; // d: name, e: child item id list
pub const ITEM_USE: i64 = 1; // d: path segments name-id list, e: alias name (NONE)
pub const ITEM_STRUCT: i64 = 2; // d: name, e: field id list, f: type param name-id list
pub const ITEM_ENUM: i64 = 3; // d: name, e: variant id list, f: type param name-id list
pub const ITEM_TRAIT: i64 = 4; // d: name, e: method fn id list, f: type param name-id list
pub const ITEM_IMPL: i64 = 5; // d: trait path name-id list, e: for-type id, f: method fn id list
pub const ITEM_FUN: i64 = 6; // d: fn id
pub const ITEM_NATIVE_FUN: i64 = 7; // d: fn id
pub const ITEM_CONST: i64 = 8; // d: name, e: type id, f: value expr id
pub const ITEM_NATIVE_TYPE: i64 = 9; // d: name, e: type param name-id list

// ---------------------------------------------------------------------------
// Function rows.  (tag=NODE_FN, a=name, b=type_param_list, c=param_list,
// d=ret_ty, e=is_impure, f=body_stmt_list).  Trait method signatures have
// a NONE body list.  Native-ness comes from the wrapping item kind.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Parameter rows.  (tag=NODE_PARAM, a=name, b=ty).
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Struct-field rows.  (tag=NODE_FIELD, a=name, b=ty, c=is_pub).
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Enum-variant rows.  (tag=NODE_VARIANT, a=name, b=payload_type_list, c=is_pub).
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Match-arm rows.  (tag=NODE_ARM, a=pattern, b=body_stmt_list).
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Type rows.  (tag=NODE_TY, a=kind, b..c kind-specific).
// ---------------------------------------------------------------------------

pub const TY_NAMED: i64 = 0; // b: name id
pub const TY_PATH: i64 = 1; // b: segments name-id list
pub const TY_GENERIC: i64 = 2; // b: segments name-id list, c: argument type-id list
pub const TY_REF: i64 = 3; // b: inner type id
pub const TY_REF_MUT: i64 = 4; // b: inner type id
pub const TY_SLICE: i64 = 5; // b: element type id; `[T]`
pub const TY_ARRAY: i64 = 6; // b: element type id, c: length; `[T; N]`
pub const TY_SELF: i64 = 7; // `Self` inside a trait signature
pub const TY_PARAM: i64 = 8; // b: name id, c: trait-bound path segments list (NONE)

// ---------------------------------------------------------------------------
// Expression rows.  (tag=NODE_EXPR, a=kind, b..d kind-specific, e=ty, f=sym).
// `e` is the inferred type id attached by the typechecker; `f` is the
// resolved symbol id attached by the resolver.  Both are NONE until their
// owning stage runs.
// ---------------------------------------------------------------------------

pub const EXPR_LIT: i64 = 0; // b: literal kind, c: value
pub const EXPR_PATH: i64 = 1; // b: segments name-id list; name references, callees, field chains
pub const EXPR_UNARY: i64 = 2; // b: unary op, c: operand expr id
pub const EXPR_BINARY: i64 = 3; // b: binary op, c: lhs expr id, d: rhs expr id
pub const EXPR_CALL: i64 = 4; // b: callee expr id, c: type-arg type-id list (NONE), d: arg expr-id list
pub const EXPR_STRUCT_LIT: i64 = 5; // b: path segments name-id list, c: field name-id list, d: field value expr-id list
pub const EXPR_ARRAY: i64 = 6; // b: element expr-id list
pub const EXPR_MATCH: i64 = 7; // b: scrutinee expr id, c: arm ids list
pub const EXPR_TRY: i64 = 8; // b: inner expr id

// Literal kinds.
pub const LIT_INT: i64 = 0;
pub const LIT_HEX: i64 = 1;
pub const LIT_TRUE: i64 = 2;
pub const LIT_FALSE: i64 = 3;

// Unary operators.
pub const UN_NEG: i64 = 0;
pub const UN_NOT: i64 = 1;
pub const UN_REF: i64 = 2;
pub const UN_REF_MUT: i64 = 3;

// Binary operators.
pub const BIN_ADD: i64 = 0;
pub const BIN_SUB: i64 = 1;
pub const BIN_MUL: i64 = 2;
pub const BIN_DIV: i64 = 3;
pub const BIN_SHL: i64 = 4;
pub const BIN_SHR: i64 = 5;
pub const BIN_BAND: i64 = 6;
pub const BIN_BOR: i64 = 7;
pub const BIN_BXOR: i64 = 8;
pub const BIN_EQ: i64 = 9;
pub const BIN_NE: i64 = 10;
pub const BIN_LT: i64 = 11;
pub const BIN_GT: i64 = 12;
pub const BIN_LE: i64 = 13;
pub const BIN_GE: i64 = 14;
pub const BIN_AND: i64 = 15;
pub const BIN_OR: i64 = 16;
pub const BIN_MOD: i64 = 17;

// ---------------------------------------------------------------------------
// Statement rows.  (tag=NODE_STMT, a=kind, b..e kind-specific, f=ty).
// ---------------------------------------------------------------------------

pub const STMT_LET: i64 = 0; // b: is_mut, c: name id, d: type id (NONE), e: init expr id
pub const STMT_ASSIGN: i64 = 1; // b: target name id, c: value expr id
pub const STMT_WHILE: i64 = 2; // b: cond expr id, c: body stmt-id list
pub const STMT_IF: i64 = 3; // b: cond expr id, c: then stmt-id list, d: else stmt-id list (NONE)
pub const STMT_RETURN: i64 = 4; // b: value expr id (NONE)
pub const STMT_BREAK: i64 = 5;
pub const STMT_CONTINUE: i64 = 6;
pub const STMT_EXPR: i64 = 7; // b: expr id; includes `match`, `try`, and bare calls as statements

// ---------------------------------------------------------------------------
// Pattern rows.  (tag=NODE_PAT, a=kind, b..c kind-specific, d=ty).
// ---------------------------------------------------------------------------

pub const PAT_BIND: i64 = 0; // b: bound name id
pub const PAT_PATH: i64 = 1; // b: segments name-id list; unit variant such as `None`
pub const PAT_VARIANT: i64 = 2; // b: segments name-id list, c: payload pattern-id list
pub const PAT_ARRAY: i64 = 3; // b: element pattern-id list, c: rest binder name id (NONE)
pub const PAT_LIT: i64 = 4; // b: literal kind, c: value

// ---------------------------------------------------------------------------
// Symbol rows.  (tag=NODE_SYM, a=kind, b=name, c=decl).  `name` is the
// fully qualified path interned as a single name (for example
// "Collections.vec_new").  `decl` is the id of the declaration the symbol
// names: a fn id for functions and impl methods, an item id for
// types/consts/modules, a variant id for variants.
// ---------------------------------------------------------------------------

pub const SYM_MODULE: i64 = 0;
pub const SYM_STRUCT: i64 = 1;
pub const SYM_ENUM: i64 = 2;
pub const SYM_TRAIT: i64 = 3;
pub const SYM_TYPE: i64 = 4; // native types
pub const SYM_VARIANT: i64 = 5;
pub const SYM_FUN: i64 = 6;
pub const SYM_NATIVE_FUN: i64 = 7;
pub const SYM_CONST: i64 = 8;
pub const SYM_IMPL_METHOD: i64 = 9;
pub const SYM_TRAIT_METHOD: i64 = 10;

// ---------------------------------------------------------------------------
// Name arena.  Identifiers, paths, operators, and qualified symbol names
// are interned here.  This is the only String storage in the compiler.
// ---------------------------------------------------------------------------

/// Interning returns the id of the name, pushing it when new.
pub fn intern(names: &mut Vec<String>, text: &str) -> i64 {
    let mut idx = 0i64;
    while idx < names.len() as i64 {
        match names.get(idx as usize) {
            Some(existing) => {
                if *existing == text {
                    return idx;
                }
            }
            None => break,
        }
        idx += 1;
    }
    names.push(text.to_string());
    names.len() as i64 - 1
}

/// True when the interned name id names the given text.
pub fn name_is(names: &[String], id: i64, text: &str) -> bool {
    match names.get(id as usize) {
        Some(existing) => *existing == text,
        None => false,
    }
}

/// Returns the text of an interned name, or an empty string for an
/// invalid id.  Used only for diagnostics and the AST dump.
pub fn name_text(names: &[String], id: i64) -> String {
    match names.get(id as usize) {
        Some(text) => text.clone(),
        None => String::new(),
    }
}

/// Joins interned name ids into a dotted path.
pub fn join_path(names: &[String], ids: &[i64]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut idx = 0i64;
    while idx < ids.len() as i64 {
        match ids.get(idx as usize) {
            Some(id) => parts.push(name_text(names, *id)),
            None => break,
        }
        idx += 1;
    }
    parts.join(".")
}

// ---------------------------------------------------------------------------
// List arena.  Variable-length child lists.  A list id indexes this table.
// ---------------------------------------------------------------------------

/// Allocates an empty list and returns its id.
pub fn alloc_list(lists: &mut Vec<Vec<i64>>) -> i64 {
    lists.push(Vec::new());
    lists.len() as i64 - 1
}

/// Appends a value to a list.  Returns false when the list id is invalid.
pub fn list_push(lists: &mut [Vec<i64>], list_id: i64, value: i64) -> bool {
    match lists.get_mut(list_id as usize) {
        Some(list) => {
            list.push(value);
            true
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Checked arena access.  Every access goes through these functions so no
// node id can index out of bounds.  An invalid id yields NONE, and callers
// report an internal-invariant diagnostic rather than fabricating a value.
// ---------------------------------------------------------------------------

pub fn node_get(nodes: &[i64], id: i64, offset: i64) -> i64 {
    let slot = match id.checked_mul(NODE_STRIDE).and_then(|base| base.checked_add(offset)) {
        Some(slot) => slot,
        None => return NONE,
    };
    if slot < 0 {
        return NONE;
    }
    match nodes.get(slot as usize) {
        Some(value) => *value,
        None => NONE,
    }
}

pub fn node_set(nodes: &mut [i64], id: i64, offset: i64, value: i64) -> bool {
    let slot = match id.checked_mul(NODE_STRIDE).and_then(|base| base.checked_add(offset)) {
        Some(slot) => slot,
        None => return false,
    };
    if slot < 0 {
        return false;
    }
    match nodes.get_mut(slot as usize) {
        Some(cell) => {
            *cell = value;
            true
        }
        None => false,
    }
}

/// Allocates a node row and returns its id.  The fields slice may hold
/// fewer than NODE_STRIDE values; missing slots are filled with NONE.
pub fn alloc_node(nodes: &mut Vec<i64>, fields: &[i64]) -> i64 {
    let id = nodes.len() as i64 / NODE_STRIDE;
    let mut idx = 0i64;
    while idx < NODE_STRIDE {
        let value = match fields.get(idx as usize) {
            Some(field) => *field,
            None => NONE,
        };
        nodes.push(value);
        idx += 1;
    }
    id
}

// ---------------------------------------------------------------------------
// Uniform accessors.
// ---------------------------------------------------------------------------

pub fn node_tag(nodes: &[i64], id: i64) -> i64 {
    node_get(nodes, id, NODE_TAG)
}

pub fn node_file(nodes: &[i64], id: i64) -> i64 {
    node_get(nodes, id, NODE_FILE)
}

pub fn node_start(nodes: &[i64], id: i64) -> i64 {
    node_get(nodes, id, NODE_START)
}

pub fn node_end(nodes: &[i64], id: i64) -> i64 {
    node_get(nodes, id, NODE_END)
}

pub fn node_a(nodes: &[i64], id: i64) -> i64 {
    node_get(nodes, id, NODE_A)
}

pub fn node_b(nodes: &[i64], id: i64) -> i64 {
    node_get(nodes, id, NODE_B)
}

pub fn node_c(nodes: &[i64], id: i64) -> i64 {
    node_get(nodes, id, NODE_C)
}

pub fn node_d(nodes: &[i64], id: i64) -> i64 {
    node_get(nodes, id, NODE_D)
}

pub fn node_e(nodes: &[i64], id: i64) -> i64 {
    node_get(nodes, id, NODE_E)
}

pub fn node_f(nodes: &[i64], id: i64) -> i64 {
    node_get(nodes, id, NODE_F)
}

pub fn node_set_a(nodes: &mut [i64], id: i64, value: i64) -> bool {
    node_set(nodes, id, NODE_A, value)
}

pub fn node_set_b(nodes: &mut [i64], id: i64, value: i64) -> bool {
    node_set(nodes, id, NODE_B, value)
}

pub fn node_set_c(nodes: &mut [i64], id: i64, value: i64) -> bool {
    node_set(nodes, id, NODE_C, value)
}

pub fn node_set_d(nodes: &mut [i64], id: i64, value: i64) -> bool {
    node_set(nodes, id, NODE_D, value)
}

pub fn node_set_e(nodes: &mut [i64], id: i64, value: i64) -> bool {
    node_set(nodes, id, NODE_E, value)
}

pub fn node_set_f(nodes: &mut [i64], id: i64, value: i64) -> bool {
    node_set(nodes, id, NODE_F, value)
}

// ---------------------------------------------------------------------------
// Token helpers.
// ---------------------------------------------------------------------------

/// True when the token at `id` is the word (identifier or keyword) `text`.
pub fn tok_is_name(nodes: &[i64], names: &[String], id: i64, text: &str) -> bool {
    node_tag(nodes, id) == NODE_TOKEN
        && node_a(nodes, id) == TOK_IDENT
        && name_is(names, node_b(nodes, id), text)
}

/// True when the token at `id` is the symbol `text`.
pub fn tok_is_sym(nodes: &[i64], names: &[String], id: i64, text: &str) -> bool {
    node_tag(nodes, id) == NODE_TOKEN
        && node_a(nodes, id) == TOK_SYM
        && name_is(names, node_b(nodes, id), text)
}

// ---------------------------------------------------------------------------
// Kind-specific helpers.
// ---------------------------------------------------------------------------

pub fn item_is_pub(nodes: &[i64], id: i64) -> i64 {
    node_b(nodes, id)
}

pub fn item_sym_of(nodes: &[i64], id: i64) -> i64 {
    node_c(nodes, id)
}

pub fn item_set_sym(nodes: &mut [i64], id: i64, sym: i64) -> bool {
    node_set_c(nodes, id, sym)
}

pub fn expr_ty_of(nodes: &[i64], id: i64) -> i64 {
    node_e(nodes, id)
}

pub fn expr_set_ty(nodes: &mut [i64], id: i64, ty: i64) -> bool {
    node_set_e(nodes, id, ty)
}

pub fn expr_sym_of(nodes: &[i64], id: i64) -> i64 {
    node_f(nodes, id)
}

pub fn expr_set_sym(nodes: &mut [i64], id: i64, sym: i64) -> bool {
    node_set_f(nodes, id, sym)
}

pub fn stmt_ty_of(nodes: &[i64], id: i64) -> i64 {
    node_f(nodes, id)
}

pub fn stmt_set_ty(nodes: &mut [i64], id: i64, ty: i64) -> bool {
    node_set_f(nodes, id, ty)
}

pub fn pat_ty_of(nodes: &[i64], id: i64) -> i64 {
    node_d(nodes, id)
}

pub fn pat_set_ty(nodes: &mut [i64], id: i64, ty: i64) -> bool {
    node_set_d(nodes, id, ty)
}

pub fn pat_sym_of(nodes: &[i64], id: i64) -> i64 {
    node_e(nodes, id)
}

pub fn pat_set_sym(nodes: &mut [i64], id: i64, sym: i64) -> bool {
    node_set_e(nodes, id, sym)
}

/// The rest binder's key on an array pattern (slot f), attached by the
/// typechecker so codegen never re-derives it.
pub fn pat_rest_key_of(nodes: &[i64], id: i64) -> i64 {
    node_f(nodes, id)
}

pub fn pat_set_rest_key(nodes: &mut [i64], id: i64, key: i64) -> bool {
    node_set_f(nodes, id, key)
}

// ---------------------------------------------------------------------------
// Type-node helpers.  The resolver attaches the resolved symbol id in slot
// e; the typechecker attaches the canonical type key in slot d.  Both are
// NONE until their owning stage runs.
// ---------------------------------------------------------------------------

pub fn ty_key_of(nodes: &[i64], id: i64) -> i64 {
    node_d(nodes, id)
}

pub fn ty_set_key(nodes: &mut [i64], id: i64, key: i64) -> bool {
    node_set_d(nodes, id, key)
}

pub fn ty_sym_of(nodes: &[i64], id: i64) -> i64 {
    node_e(nodes, id)
}

pub fn ty_set_sym(nodes: &mut [i64], id: i64, sym: i64) -> bool {
    node_set_e(nodes, id, sym)
}

// ---------------------------------------------------------------------------
// Type-descriptor rows.  (tag=NODE_TYINFO, a=key, b=kind, c=sym, d=args
// list, e=element key, f=len/bound).  One row per canonical type key,
// built by the typechecker from the program's declarations; consumed by
// the typechecker and codegen.  Compiler-internal facts have no Cinnabar
// source origin and carry NO_FILE spans.
// ---------------------------------------------------------------------------

pub const TYD_UNKNOWN: i64 = 0;
pub const TYD_BUILTIN: i64 = 1;
pub const TYD_STRUCT: i64 = 2;
pub const TYD_ENUM: i64 = 3;
pub const TYD_NATIVE: i64 = 4;
pub const TYD_REF: i64 = 5;
pub const TYD_REF_MUT: i64 = 6;
pub const TYD_SLICE: i64 = 7;
pub const TYD_ARRAY: i64 = 8;
pub const TYD_PARAM: i64 = 9;
pub const TYD_MONO: i64 = 10; // (fn node, type-arg keys): one key per monomorphized function

pub fn alloc_tyinfo(nodes: &mut Vec<i64>, key: i64, kind: i64, sym: i64, args: i64, elem: i64, len: i64) -> i64 {
    alloc_node(nodes, &[NODE_TYINFO, NO_FILE, NO_FILE, NO_FILE, key, kind, sym, args, elem, len])
}

/// Returns the descriptor row id for `key`, or NONE.
pub fn find_tyinfo(nodes: &[i64], key: i64) -> i64 {
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_TYINFO && node_a(nodes, idx) == key {
            return idx;
        }
        idx += 1;
    }
    NONE
}

// ---------------------------------------------------------------------------
// Instance rows.  (tag=NODE_INST, a=fn node, b=type-arg key list,
// c=return key, d=param key list, e=mono key, f=sym kind).  One row per
// monomorphized function body; call expressions carry the row id in their
// symbol slot.  Compiler-internal facts: NO_FILE spans.
// ---------------------------------------------------------------------------

pub fn inst_fn_of(nodes: &[i64], id: i64) -> i64 {
    node_a(nodes, id)
}

pub fn inst_args_of(nodes: &[i64], id: i64) -> i64 {
    node_b(nodes, id)
}

pub fn inst_set_args(nodes: &mut [i64], id: i64, list: i64) -> bool {
    node_set_b(nodes, id, list)
}

pub fn inst_ret_of(nodes: &[i64], id: i64) -> i64 {
    node_c(nodes, id)
}

pub fn inst_params_of(nodes: &[i64], id: i64) -> i64 {
    node_d(nodes, id)
}

pub fn inst_mono_of(nodes: &[i64], id: i64) -> i64 {
    node_e(nodes, id)
}

pub fn inst_set_ret(nodes: &mut [i64], id: i64, key: i64) -> bool {
    node_set_c(nodes, id, key)
}

pub fn inst_set_params(nodes: &mut [i64], id: i64, list: i64) -> bool {
    node_set_d(nodes, id, list)
}

pub fn inst_set_mono(nodes: &mut [i64], id: i64, key: i64) -> bool {
    node_set_e(nodes, id, key)
}

/// Returns the instance row whose mono key matches, or NONE.
pub fn find_instance(nodes: &[i64], mono: i64) -> i64 {
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_INST && node_e(nodes, idx) == mono {
            return idx;
        }
        idx += 1;
    }
    NONE
}

// ---------------------------------------------------------------------------
// Constant-value rows.  (tag=NODE_CONSTVAL, a=sym, b=value).  One row per
// folded constant, built by the typechecker.  NO_FILE spans.
// ---------------------------------------------------------------------------

/// Whether the constant symbol `sym` has a folded-value row.  The row's
/// existence is the source of truth for "is folded": a folded value of
/// `-1` is indistinguishable from the `NONE` sentinel otherwise.
pub fn has_const_value(nodes: &[i64], sym: i64) -> bool {
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_CONSTVAL && node_a(nodes, idx) == sym {
            return true;
        }
        idx += 1;
    }
    false
}

/// The folded value of the constant symbol `sym`.  Callers must check
/// `has_const_value` first; a folded value of `-1` is a legal value, not
/// an absence marker.
pub fn find_const_value(nodes: &[i64], sym: i64) -> i64 {
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_CONSTVAL && node_a(nodes, idx) == sym {
            return node_b(nodes, idx);
        }
        idx += 1;
    }
    NONE
}

// ---------------------------------------------------------------------------
// Trait-dispatch rows.  (tag=NODE_TRAIT, a=call expr, b=method instance id,
// c=trait symbol, d=method name id).  One row per trait method call,
// created by the typechecker.  When the receiver type is concrete the
// method instance is resolved here and stored in `b`; when the receiver
// is a type parameter (a generic body), the row is deferred (`b` NONE)
// and codegen reads the impl table (a fact the typechecker stored) with
// the substituted receiver type.  NO_FILE spans: compiler-internal facts.
// ---------------------------------------------------------------------------

pub const NODE_TRAIT: i64 = 15;

pub fn trait_call_inst(nodes: &[i64], id: i64) -> i64 {
    node_b(nodes, id)
}

pub fn trait_call_trait(nodes: &[i64], id: i64) -> i64 {
    node_c(nodes, id)
}

pub fn trait_call_method(nodes: &[i64], id: i64) -> i64 {
    node_d(nodes, id)
}

pub fn alloc_trait_call(nodes: &mut Vec<i64>, expr: i64, inst: i64, trait_sym: i64, method: i64) -> i64 {
    alloc_node(nodes, &[NODE_TRAIT, NO_FILE, NO_FILE, NO_FILE, expr, inst, trait_sym, method, NONE, NONE])
}

/// The trait-dispatch row for `expr`, or NONE.
pub fn find_trait_call(nodes: &[i64], expr: i64) -> i64 {
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_TRAIT && node_a(nodes, idx) == expr {
            return idx;
        }
        idx += 1;
    }
    NONE
}

// ---------------------------------------------------------------------------
// Canonical type construction and substitution.
// ---------------------------------------------------------------------------

fn list_eq(lists: &[Vec<i64>], a: i64, b: i64) -> bool {
    let na = list_len(lists, a);
    let nb = list_len(lists, b);
    if na != nb {
        return false;
    }
    let mut idx = 0i64;
    while idx < na {
        if list_get(lists, a, idx) != list_get(lists, b, idx) {
            return false;
        }
        idx += 1;
    }
    true
}

fn count_tyinfo(nodes: &[i64]) -> i64 {
    let mut count = 0i64;
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_TYINFO {
            count += 1;
        }
        idx += 1;
    }
    count
}

/// Finds the descriptor row whose structure matches, or NONE.
pub fn find_tyinfo_by(nodes: &[i64], lists: &[Vec<i64>], kind: i64, sym: i64, args: i64, elem: i64, len: i64) -> i64 {
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_TYINFO
            && node_b(nodes, idx) == kind
            && node_c(nodes, idx) == sym
            && list_eq(lists, node_d(nodes, idx), args)
            && node_e(nodes, idx) == elem
            && node_f(nodes, idx) == len
        {
            return idx;
        }
        idx += 1;
    }
    NONE
}

/// The key of the descriptor with the given structure, allocating a fresh
/// key when the structure is new.  Every layout fact is derived from
/// these descriptors; nothing is hardcoded.
pub fn canon_tyinfo(nodes: &mut Vec<i64>, lists: &mut [Vec<i64>], kind: i64, sym: i64, args: i64, elem: i64, len: i64) -> i64 {
    let existing = find_tyinfo_by(nodes, lists, kind, sym, args, elem, len);
    if existing != NONE {
        return node_a(nodes, existing);
    }
    let key = count_tyinfo(nodes);
    alloc_tyinfo(nodes, key, kind, sym, args, elem, len);
    key
}

fn subst_list(nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, list: i64, from: &[i64], to: &[i64]) -> i64 {
    let count = list_len(lists, list);
    if count == 0 {
        return list;
    }
    let fresh = alloc_list(lists);
    let mut changed = false;
    let mut idx = 0i64;
    while idx < count {
        let old = list_get(lists, list, idx);
        let new = subst_key(nodes, lists, old, from, to);
        list_push(lists, fresh, new);
        if new != old {
            changed = true;
        }
        idx += 1;
    }
    if changed {
        fresh
    } else {
        list
    }
}

/// Substitutes type keys: any descriptor equal to a key in `from` is
/// replaced by the corresponding key in `to`.  Used by the typechecker to
/// instantiate declared types and by codegen to lower monomorphized
/// bodies.  Both maps are facts the typechecker computed; this is a
/// mechanical rewrite shared by the two consumers.
pub fn subst_key(nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, key: i64, from: &[i64], to: &[i64]) -> i64 {
    if key < 0 {
        return key;
    }
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        return key;
    }
    let kind = node_b(nodes, row);
    let sym = node_c(nodes, row);
    if kind == TYD_PARAM {
        let mut idx = 0usize;
        loop {
            match from.get(idx) {
                Some(cand) => {
                    if *cand == key {
                        match to.get(idx) {
                            Some(repl) => return *repl,
                            None => return key,
                        }
                    }
                }
                None => return key,
            }
            idx += 1;
        }
    }
    let args = node_d(nodes, row);
    let elem = node_e(nodes, row);
    let len = node_f(nodes, row);
    let new_args = subst_list(nodes, lists, args, from, to);
    let new_elem = subst_key(nodes, lists, elem, from, to);
    if new_args == args && new_elem == elem {
        return key;
    }
    canon_tyinfo(nodes, lists, kind, sym, new_args, new_elem, len)
}

// ---------------------------------------------------------------------------
// Divergence.  A statement or expression diverges when control flow never
// falls through (a return, break, continue, or a match every arm of
// which diverges).  Purely syntactic; read by the typechecker (arm type
// merging), the borrow checker (branch state merging), and codegen
// (match lowering).
// ---------------------------------------------------------------------------

pub fn expr_diverges(nodes: &[i64], lists: &[Vec<i64>], expr: i64) -> i64 {
    if node_tag(nodes, expr) != NODE_EXPR || node_a(nodes, expr) != EXPR_MATCH {
        return 0;
    }
    let arms = node_c(nodes, expr);
    let count = list_len(lists, arms);
    if count == 0 {
        return 0;
    }
    let mut idx = 0i64;
    while idx < count {
        let arm = list_get(lists, arms, idx);
        if stmt_diverges(nodes, lists, node_b(nodes, arm)) == 0 {
            return 0;
        }
        idx += 1;
    }
    1
}

pub fn stmt_diverges(nodes: &[i64], lists: &[Vec<i64>], stmt: i64) -> i64 {
    if node_tag(nodes, stmt) != NODE_STMT {
        return 0;
    }
    let kind = node_a(nodes, stmt);
    if kind == STMT_RETURN || kind == STMT_BREAK || kind == STMT_CONTINUE {
        return 1;
    }
    if kind == STMT_EXPR {
        return expr_diverges(nodes, lists, node_b(nodes, stmt));
    }
    if kind == STMT_IF {
        if node_d(nodes, stmt) == NONE {
            return 0;
        }
        if stmt_list_diverges(nodes, lists, node_c(nodes, stmt)) == 0 {
            return 0;
        }
        return stmt_list_diverges(nodes, lists, node_d(nodes, stmt));
    }
    0
}

fn stmt_list_diverges(nodes: &[i64], lists: &[Vec<i64>], list: i64) -> i64 {
    let count = list_len(lists, list);
    if count == 0 {
        return 0;
    }
    stmt_diverges(nodes, lists, list_get(lists, list, count - 1))
}

// ---------------------------------------------------------------------------
// List helpers (shared by every later stage).
// ---------------------------------------------------------------------------

pub fn list_len(lists: &[Vec<i64>], id: i64) -> i64 {
    match lists.get(id as usize) {
        Some(items) => items.len() as i64,
        None => 0,
    }
}

pub fn list_first(lists: &[Vec<i64>], id: i64) -> i64 {
    match lists.get(id as usize) {
        Some(items) => match items.first() {
            Some(value) => *value,
            None => NONE,
        },
        None => NONE,
    }
}

pub fn list_get(lists: &[Vec<i64>], id: i64, idx: i64) -> i64 {
    match lists.get(id as usize) {
        Some(items) => match items.get(idx as usize) {
            Some(value) => *value,
            None => NONE,
        },
        None => NONE,
    }
}

// ---------------------------------------------------------------------------
// Diagnostics helpers.
// ---------------------------------------------------------------------------

/// Pushes a source-less diagnostic (an internal invariant failure or a
/// toolchain failure).  The absence of a Cinnabar source origin is
/// represented by NO_FILE; no fabricated span is ever invented.
pub fn push_internal(errors: &mut Vec<Diag>, message: &str) {
    errors.push((message.to_string(), NO_FILE, 0, 0));
}

/// Pushes a diagnostic with a real span.
pub fn push_error(errors: &mut Vec<Diag>, message: &str, file: i64, start: i64, end: i64) {
    errors.push((message.to_string(), file, start, end));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_dedups_names() {
        let mut names: Vec<String> = Vec::new();
        let a = intern(&mut names, "sum_to");
        let b = intern(&mut names, "sum_to");
        let c = intern(&mut names, "other");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(name_is(&names, a, "sum_to"));
    }

    #[test]
    fn node_roundtrip() {
        let mut nodes: Vec<i64> = Vec::new();
        let id = alloc_node(
            &mut nodes,
            &[NODE_EXPR, 0, 10, 20, EXPR_LIT, LIT_INT, 42, NONE, NONE, NONE],
        );
        // Row layout is (tag, file, start, end, a, b, c, d, e, f): the
        // literal kind sits in `b` and the literal value in `c`, and the
        // type slot `e` starts unset.
        assert_eq!(node_tag(&nodes, id), NODE_EXPR);
        assert_eq!(node_a(&nodes, id), EXPR_LIT);
        assert_eq!(node_b(&nodes, id), LIT_INT);
        assert_eq!(node_c(&nodes, id), 42);
        assert_eq!(expr_ty_of(&nodes, id), NONE);
        assert!(expr_set_ty(&mut nodes, id, 7));
        assert_eq!(expr_ty_of(&nodes, id), 7);
        assert_eq!(node_tag(&nodes, 999), NONE);
    }

    #[test]
    fn lists_grow() {
        let mut lists: Vec<Vec<i64>> = Vec::new();
        let list = alloc_list(&mut lists);
        assert!(list_push(&mut lists, list, 1));
        assert!(list_push(&mut lists, list, 2));
        match lists.get(list as usize) {
            Some(items) => assert_eq!(items.len(), 2),
            None => assert!(false),
        }
    }
}
