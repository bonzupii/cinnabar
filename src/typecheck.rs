//! Cinnabar typechecker.
//!
//! Consumes the resolver's symbol attachments, canonicalizes every type
//! into the type-descriptor table (one row per canonical key), infers the
//! type of every expression, statement, and pattern and attaches it to
//! the tree, monomorphizes generic functions into instance rows, resolves
//! trait dispatch per instance, folds constants, checks match
//! exhaustiveness, and rejects unhandled Result/Option values.  Facts
//! computed here are consumed, never recomputed, by the borrow checker
//! and codegen.
//!
//! Inference variables are negative keys; a table maps each variable to
//! the key it was unified with.  After the whole program is checked, one
//! pass substitutes every variable in every attached type, so downstream
//! stages only ever see concrete keys (or a generic function's own
//! declared parameter keys).

use crate::ast::*;

/// (key, key, key) triples of `(trait_sym, for_key, methods_list)` for
/// every `impl` in the program.
const IMPL_STRIDE: i64 = 3;

// ---------------------------------------------------------------------------
// Entry point.
// ---------------------------------------------------------------------------

/// Typechecks the whole program.  Returns whether no error was reported
/// and the impl table (a flat `(trait_sym, for_key, methods)` triple
/// list) that codegen reads for deferred trait dispatch.  The entry root
/// and every external module root are checked.
/// The typechecker's shared tables, threaded through every check as one
/// explicit tuple (the same shape as the emitter's session): source
/// arenas, diagnostics, the scope stack, the impl table, the inference
/// constraint tables, and the five seeded primitive-enum symbols
/// (Unit/Result/Option/DivError/IndexError) found once at setup.
/// `(names, nodes, lists, errors, env, impls, vars, origins, unit_sym,
/// result_sym, option_sym, div_err_sym, index_err_sym)`.
type State<'a> = (
    &'a mut Vec<String>,
    &'a mut Vec<i64>,
    &'a mut Vec<Vec<i64>>,
    &'a mut Vec<Diag>,
    &'a mut Vec<Vec<i64>>,
    &'a mut Vec<i64>,
    &'a mut Vec<(i64, i64)>,
    &'a mut Vec<(i64, i64, i64)>,
    i64,
    i64,
    i64,
    i64,
    i64,
);

pub fn typecheck(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    errors: &mut Vec<Diag>,
    root: i64,
    ext_mods: &[(i64, i64)],
) -> (bool, i64) {
    seed_builtins(names, nodes, lists);
    // The seeded primitive-enum symbols, found once at setup and stored on
    // the state so unit_key_of and division_result_key never scan the
    // arena for them again.
    let unit_sym = find_type_sym_by_name(nodes, intern(names, "Unit"));
    let result_sym = find_type_sym_by_name(nodes, intern(names, "Result"));
    let option_sym = find_type_sym_by_name(nodes, intern(names, "Option"));
    let div_err_sym = find_type_sym_by_name(nodes, intern(names, "DivError"));
    let index_err_sym = find_type_sym_by_name(nodes, intern(names, "IndexError"));
    let mut impls: Vec<i64> = Vec::new();
    let mut vars: Vec<(i64, i64)> = Vec::new();
    let mut origins: Vec<(i64, i64, i64)> = Vec::new();
    let mut env: Vec<Vec<i64>> = Vec::new();
    push_scope(&mut env);
    let mut state: State = (
        names,
        nodes,
        lists,
        errors,
        &mut env,
        &mut impls,
        &mut vars,
        &mut origins,
        unit_sym,
        result_sym,
        option_sym,
        div_err_sym,
        index_err_sym,
    );

    collect_types(&mut state, root);
    let mut idx = 0usize;
    while idx < ext_mods.len() {
        match ext_mods.get(idx) {
            Some(pair) => collect_types(&mut state, pair.1),
            None => break,
        }
        idx += 1;
    }

    // Canonicalize every function signature (all roots) before any body
    // is checked, so a call in one file reads the callee's attached
    // parameter and return keys no matter which file is checked first.
    check_fn_sigs_list(&mut state, root);
    idx = 0;
    while idx < ext_mods.len() {
        match ext_mods.get(idx) {
            Some(pair) => check_fn_sigs_list(&mut state, pair.1),
            None => break,
        }
        idx += 1;
    }

    // Constants before impls: impl method bodies may reference consts
    // (e.g. `value.value ^ CHECKSUM_SALT`), and their type and value must
    // already be attached when those bodies are checked.
    collect_consts(&mut state, root);
    idx = 0;
    while idx < ext_mods.len() {
        match ext_mods.get(idx) {
            Some(pair) => collect_consts(&mut state, pair.1),
            None => break,
        }
        idx += 1;
    }

    collect_impls(&mut state, root);
    idx = 0;
    while idx < ext_mods.len() {
        match ext_mods.get(idx) {
            Some(pair) => collect_impls(&mut state, pair.1),
            None => break,
        }
        idx += 1;
    }

    check_fn_list(&mut state, root);
    idx = 0;
    while idx < ext_mods.len() {
        match ext_mods.get(idx) {
            Some(pair) => check_fn_list(&mut state, pair.1),
            None => break,
        }
        idx += 1;
    }

    let impls_list = store_impls(state.2, state.5);
    pop_scope(state.4);
    resolve_all_vars(state.0, state.1, state.2, state.3, state.6, state.7);
    attach_type_facts(state.0, state.1, state.2);
    (state.3.is_empty(), impls_list)
}

/// Stores the impl table into the list arena as flat triples so codegen
/// can resolve deferred trait dispatch from the same facts the
/// typechecker used.
/// Bounds-checked read of one impl-table slot.  The table is a flat
/// sequence of rows pushed three slots at a time, so every row is
/// complete; the NONE arm guards reads past the table's end.
fn impl_at(impls: &[i64], idx: usize) -> i64 {
    match impls.get(idx) {
        Some(value) => *value,
        None => NONE,
    }
}

fn store_impls(lists: &mut Vec<Vec<i64>>, impls: &[i64]) -> i64 {
    let list = alloc_list(lists);
    let mut idx = 0i64;
    while idx < impls.len() as i64 / IMPL_STRIDE {
        list_push(lists, list, impl_at(impls, (idx * IMPL_STRIDE) as usize));
        list_push(lists, list, impl_at(impls, (idx * IMPL_STRIDE + 1) as usize));
        list_push(lists, list, impl_at(impls, (idx * IMPL_STRIDE + 2) as usize));
        idx += 1;
    }
    list
}

// ---------------------------------------------------------------------------
// Symbols.  The resolver's symbol rows are public arena data; these are
// structural reads, not recomputation.
// ---------------------------------------------------------------------------

fn sym_kind(nodes: &[i64], sym: i64) -> i64 {
    node_a(nodes, sym)
}

fn sym_name(nodes: &[i64], sym: i64) -> i64 {
    node_b(nodes, sym)
}

fn sym_decl(nodes: &[i64], sym: i64) -> i64 {
    node_c(nodes, sym)
}

fn sym_home(nodes: &[i64], sym: i64) -> i64 {
    node_d(nodes, sym)
}

/// The key of the builtin whose sub-kind is `sub`, or NONE.  The sub-kind
/// was stored in the descriptor row at seed time.
fn builtin_key_of(nodes: &[i64], sub: i64) -> i64 {
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_TYINFO && node_b(nodes, idx) == TYD_BUILTIN && node_f(nodes, idx) == sub {
            return node_a(nodes, idx);
        }
        idx += 1;
    }
    NONE
}

/// The key of the builtin symbol `sym` (which has no declaration).
fn builtin_key_of_sym(nodes: &[i64], sym: i64) -> i64 {
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_TYINFO && node_b(nodes, idx) == TYD_BUILTIN && node_c(nodes, idx) == sym {
            return node_a(nodes, idx);
        }
        idx += 1;
    }
    NONE
}

/// Allocates canonical builtin keys for Int, U8, U32, Usize, and Bool,
/// each carrying its scalar sub-kind in the descriptor's slot `f` so that
/// every later stage classifies scalars from the stored integer, never
/// from the symbol's name.  Unit, Result, and Option are declared enums
/// synthesized by the resolver; the typechecker reads them through the
/// same declaration path a user enum uses.  The scalar keys mirror the
/// resolver's builtin seeding exactly: the resolver enters the type
/// symbol into its scopes and the typechecker allocates the matching key.
fn seed_builtins(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut [Vec<i64>]) {
    let ints = [
        (intern(names, "Int"), BUILTIN_INT),
        (intern(names, "U8"), BUILTIN_U8),
        (intern(names, "U32"), BUILTIN_U32),
        (intern(names, "Usize"), BUILTIN_USIZE),
    ];
    let mut idx = 0usize;
    while idx < ints.len() {
        let (name_id, sub) = match ints.get(idx) {
            Some(pair) => *pair,
            None => break,
        };
        seed_builtin(nodes, lists, name_id, sub);
        idx += 1;
    }
    let bool_name = intern(names, "Bool");
    seed_builtin(nodes, lists, bool_name, BUILTIN_BOOL);
}

/// Allocates one builtin key with its scalar sub-kind, looking up the
/// symbol the resolver seeded.
fn seed_builtin(nodes: &mut Vec<i64>, lists: &mut [Vec<i64>], name: i64, sub: i64) {
    let sym = find_type_sym_by_name(nodes, name);
    if sym != NONE {
        canon_tyinfo(nodes, lists, TYD_BUILTIN, sym, NONE, NONE, sub);
    }
}

/// Finds a type symbol (struct, enum, trait, native, or builtin) whose
/// fully qualified name is `name`.
fn find_type_sym_by_name(nodes: &[i64], name: i64) -> i64 {
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_SYM {
            let kind = node_a(nodes, idx);
            if (kind == SYM_TYPE || kind == SYM_STRUCT || kind == SYM_ENUM || kind == SYM_TRAIT)
                && node_b(nodes, idx) == name
            {
                return idx;
            }
        }
        idx += 1;
    }
    NONE
}

// ---------------------------------------------------------------------------
// Local environment.  A stack of scopes; each scope is a flat array of
// (name, key, is_mut) entries.  The current scope is always the top.
// ---------------------------------------------------------------------------

fn push_scope(env: &mut Vec<Vec<i64>>) {
    env.push(Vec::new());
}

fn pop_scope(env: &mut Vec<Vec<i64>>) {
    env.pop();
}

fn entry_at(scope: &[i64], idx: i64, slot: i64) -> i64 {
    match scope.get((idx * 3 + slot) as usize) {
        Some(value) => *value,
        None => NONE,
    }
}

fn bind(env: &mut [Vec<i64>], name: i64, key: i64, is_mut: i64) {
    if let Some(scope) = env.last_mut() {
        scope.push(name);
        scope.push(key);
        scope.push(is_mut);
    }
}

/// Looks up `name` from the innermost scope outwards.  Returns
/// `(key, is_mut)` or `(NONE, 0)`.
fn lookup(env: &[Vec<i64>], name: i64) -> (i64, i64) {
    let mut depth = env.len();
    while depth > 0 {
        depth -= 1;
        match env.get(depth) {
            Some(scope) => {
                let mut idx = 0i64;
                while idx < scope.len() as i64 / 3 {
                    if entry_at(scope, idx, 0) == name {
                        return (entry_at(scope, idx, 1), entry_at(scope, idx, 2));
                    }
                    idx += 1;
                }
            }
            None => break,
        }
    }
    (NONE, 0)
}

// ---------------------------------------------------------------------------
// Type keys.
// ---------------------------------------------------------------------------

/// True when `key` is an inference variable (a negative key).
fn is_var(key: i64) -> bool {
    key < NONE
}

/// The key a variable resolves to, following chains.  An unbound variable
/// resolves to itself.
fn resolve_var(vars: &[(i64, i64)], var: i64) -> i64 {
    let mut current = var;
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 64 {
            return current;
        }
        let mut bound = current;
        let mut found = false;
        let mut idx = 0usize;
        while idx < vars.len() {
            match vars.get(idx) {
                Some(pair) => {
                    if pair.0 == current {
                        bound = pair.1;
                        found = true;
                    }
                }
                None => break,
            }
            idx += 1;
        }
        if !found || bound == current {
            return current;
        }
        if is_var(bound) {
            current = bound;
        } else {
            return bound;
        }
    }
}

fn bind_var(vars: &mut [(i64, i64)], var: i64, key: i64) {
    let mut idx = 0usize;
    while idx < vars.len() {
        match vars.get_mut(idx) {
            Some(pair) => {
                if pair.0 == var {
                    pair.1 = key;
                    return;
                }
            }
            None => break,
        }
        idx += 1;
    }
}

/// Allocates a fresh inference variable recorded with its origin (the
/// expression it was created for and the type parameter it stands for).
fn fresh_var(vars: &mut Vec<(i64, i64)>, origins: &mut Vec<(i64, i64, i64)>, expr: i64, name: i64) -> i64 {
    let var = -(vars.len() as i64) - 2;
    vars.push((var, var));
    origins.push((var, expr, name));
    var
}

/// The declared-parameter key for `(owner, name)` with `bound` (a trait
/// symbol or NONE).  Deduplicated so every reference to the same
/// parameter is the same key.
fn param_decl_key(nodes: &mut Vec<i64>, lists: &mut [Vec<i64>], owner: i64, name: i64, bound: i64) -> i64 {
    canon_tyinfo(nodes, lists, TYD_PARAM, name, NONE, owner, bound)
}

/// Binds a declaration's type parameters (TY_PARAM nodes) into `env`,
/// attaching each parameter's key to its node.
fn bind_type_params(nodes: &mut Vec<i64>, lists: &mut [Vec<i64>], env: &mut [Vec<i64>], owner: i64, params: i64) {
    let count = list_len(lists, params);
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(lists, params, idx);
        if node_tag(nodes, param) == NODE_TY && node_a(nodes, param) == TY_PARAM {
            let name = node_b(nodes, param);
            let bound = node_c(nodes, param);
            let key = param_decl_key(nodes, lists, owner, name, bound);
            ty_set_key(nodes, param, key);
            bind(env, name, key, 0);
        }
        idx += 1;
    }
}

/// The declared type-parameter count of a type declaration item.
fn declared_param_count(nodes: &[i64], lists: &[Vec<i64>], item: i64) -> i64 {
    if node_a(nodes, item) == ITEM_NATIVE_TYPE {
        list_len(lists, node_e(nodes, item))
    } else {
        list_len(lists, node_f(nodes, item))
    }
}

/// The declared-parameter keys of a type declaration item, in order.
fn declared_param_keys(nodes: &[i64], lists: &[Vec<i64>], item: i64) -> Vec<i64> {
    let params = if node_a(nodes, item) == ITEM_NATIVE_TYPE {
        node_e(nodes, item)
    } else {
        node_f(nodes, item)
    };
    let mut keys: Vec<i64> = Vec::new();
    let count = list_len(lists, params);
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(lists, params, idx);
        if node_tag(nodes, param) == NODE_TY && node_a(nodes, param) == TY_PARAM {
            keys.push(ty_key_of(nodes, param));
        }
        idx += 1;
    }
    keys
}

/// The key of a named type symbol with no type arguments.  Generic types
/// referenced without arguments are an error.
fn named_key(names: &[String], nodes: &mut Vec<i64>, lists: &mut [Vec<i64>], errors: &mut Vec<Diag>, sym: i64, span: (i64, i64, i64)) -> i64 {
    let (file, start, end) = span;
    let kind = sym_kind(nodes, sym);
    if kind == SYM_TYPE {
        let decl = sym_decl(nodes, sym);
        if decl == NONE {
            return builtin_key_of_sym(nodes, sym);
        }
        if declared_param_count(nodes, lists, decl) > 0 {
            push_error(errors, &format!("type '{}' requires type arguments", name_text(names, sym_name(nodes, sym))), file, start, end);
            return unknown_key(nodes, lists);
        }
        return canon_tyinfo(nodes, lists, TYD_NATIVE, sym, NONE, NONE, NONE);
    }
    let kind_of = if kind == SYM_STRUCT { TYD_STRUCT } else { TYD_ENUM };
    let decl = sym_decl(nodes, sym);
    if declared_param_count(nodes, lists, decl) > 0 {
        push_error(errors, &format!("type '{}' requires type arguments", name_text(names, sym_name(nodes, sym))), file, start, end);
        return unknown_key(nodes, lists);
    }
    canon_tyinfo(nodes, lists, kind_of, sym, NONE, NONE, NONE)
}

/// The single shared unknown key, for expressions that already failed.
fn unknown_key(nodes: &mut Vec<i64>, lists: &mut [Vec<i64>]) -> i64 {
    canon_tyinfo(nodes, lists, TYD_UNKNOWN, NONE, NONE, NONE, NONE)
}

/// Canonicalizes a type node under `env` and `self_key`, attaching the
/// key to the node when `write` is 1.  Call sites that must not disturb
/// an already-attached key pass `write` 0.
fn canon_ty(state: &mut State, ty: i64, self_key: i64, write: i64) -> i64 {
    if node_tag(state.1, ty) != NODE_TY {
        return unknown_key(state.1, state.2);
    }
    let kind = node_a(state.1, ty);
    let key = canon_ty_kind(state, ty, kind, self_key, write);
    if write == 1 {
        ty_set_key(state.1, ty, key);
    }
    key
}

fn canon_ty_kind(state: &mut State, ty: i64, kind: i64, self_key: i64, write: i64) -> i64 {
    let file = node_file(state.1, ty);
    let start = node_start(state.1, ty);
    let end = node_end(state.1, ty);
    if kind == TY_NAMED {
        let name = node_b(state.1, ty);
        let found = lookup(state.4, name);
        if found.0 != NONE {
            return found.0;
        }
        let sym = ty_sym_of(state.1, ty);
        if sym == NONE {
            push_error(state.3, &format!("unknown type '{}'", name_text(state.0, name)), file, start, end);
            return unknown_key(state.1, state.2);
        }
        return named_key(state.0, state.1, state.2, state.3, sym, (file, start, end));
    }
    if kind == TY_PATH {
        let sym = ty_sym_of(state.1, ty);
        if sym == NONE {
            push_error(state.3, "cannot resolve type", file, start, end);
            return unknown_key(state.1, state.2);
        }
        return named_key(state.0, state.1, state.2, state.3, sym, (file, start, end));
    }
    if kind == TY_GENERIC {
        let sym = ty_sym_of(state.1, ty);
        if sym == NONE {
            push_error(state.3, "cannot resolve type", file, start, end);
            return unknown_key(state.1, state.2);
        }
        if sym_kind(state.1, sym) == SYM_TYPE && sym_decl(state.1, sym) == NONE {
            push_error(state.3, &format!("builtin type '{}' does not take type arguments", name_text(state.0, sym_name(state.1, sym))), file, start, end);
            return unknown_key(state.1, state.2);
        }
        let targs = node_c(state.1, ty);
        let args = canon_ty_list(state, targs, self_key, write);
        let kind_of = if sym_kind(state.1, sym) == SYM_STRUCT { TYD_STRUCT } else if sym_kind(state.1, sym) == SYM_ENUM { TYD_ENUM } else { TYD_NATIVE };
        return canon_tyinfo(state.1, state.2, kind_of, sym, args, NONE, NONE);
    }
    if kind == TY_REF || kind == TY_REF_MUT {
        let inner_ty = node_b(state.1, ty);
        let inner = canon_ty(state, inner_ty, self_key, write);
        let kind_of = if kind == TY_REF { TYD_REF } else { TYD_REF_MUT };
        return canon_tyinfo(state.1, state.2, kind_of, NONE, NONE, inner, NONE);
    }
    if kind == TY_SLICE {
        let elem_ty = node_b(state.1, ty);
        let elem = canon_ty(state, elem_ty, self_key, write);
        return canon_tyinfo(state.1, state.2, TYD_SLICE, NONE, NONE, elem, NONE);
    }
    if kind == TY_ARRAY {
        let elem_ty = node_b(state.1, ty);
        let elem = canon_ty(state, elem_ty, self_key, write);
        return canon_tyinfo(state.1, state.2, TYD_ARRAY, NONE, NONE, elem, node_c(state.1, ty));
    }
    if kind == TY_SELF {
        return self_key;
    }
    if kind == TY_PARAM {
        let name = node_b(state.1, ty);
        let found = lookup(state.4, name);
        if found.0 != NONE {
            return found.0;
        }
        push_error(state.3, &format!("unknown type parameter '{}'", name_text(state.0, name)), file, start, end);
        return unknown_key(state.1, state.2);
    }
    push_error(state.3, "malformed type", file, start, end);
    unknown_key(state.1, state.2)
}

fn canon_ty_list(state: &mut State, list: i64, self_key: i64, write: i64) -> i64 {
    let fresh = alloc_list(state.2);
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        let item = list_get(state.2, list, idx);
        let k = canon_ty(state, item, self_key, write);
        list_push(state.2, fresh, k);
        idx += 1;
    }
    fresh
}

/// A fresh inference variable that is not recorded with an origin; used
/// inside signature verification where the variable only needs to unify
/// with a concrete key within one call.
fn fresh_var_local(vars: &mut Vec<(i64, i64)>) -> i64 {
    let var = -(vars.len() as i64) - 2;
    vars.push((var, var));
    var
}

// ---------------------------------------------------------------------------
// Kind predicates over keys.
// ---------------------------------------------------------------------------

fn key_kind(nodes: &[i64], key: i64) -> i64 {
    if is_var(key) {
        return TYD_UNKNOWN;
    }
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        TYD_UNKNOWN
    } else {
        node_b(nodes, row)
    }
}

fn key_sym(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        NONE
    } else {
        node_c(nodes, row)
    }
}

fn key_args(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        NONE
    } else {
        node_d(nodes, row)
    }
}

fn key_elem(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        NONE
    } else {
        node_e(nodes, row)
    }
}

fn key_len(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        NONE
    } else {
        node_f(nodes, row)
    }
}

/// Whether the key is one of the four integer builtins (Int, U8, U32,
/// Usize).  Classification reads the sub-kind the typechecker stored in
/// the builtin descriptor at seed time; no name matching.
fn is_int_key(nodes: &[i64], key: i64) -> bool {
    if key_kind(nodes, key) != TYD_BUILTIN {
        return false;
    }
    let sub = tyinfo_builtin_kind(nodes, key);
    sub == BUILTIN_INT || sub == BUILTIN_U8 || sub == BUILTIN_U32 || sub == BUILTIN_USIZE
}

/// Whether the key is the Bool builtin, from its stored sub-kind.
fn is_bool_key(nodes: &[i64], key: i64) -> bool {
    key_kind(nodes, key) == TYD_BUILTIN && tyinfo_builtin_kind(nodes, key) == BUILTIN_BOOL
}

/// Whether `key` is the seeded Result enum, read from the primitive
/// sub-kind the resolver stored on the enum's symbol — never by name.
fn is_result_key(nodes: &[i64], key: i64) -> bool {
    key_kind(nodes, key) == TYD_ENUM && sym_prim_kind(nodes, key_sym(nodes, key)) == PRIM_RESULT
}

/// Whether `key` is the seeded Option enum, read from the primitive
/// sub-kind the resolver stored on the enum's symbol — never by name.
fn is_option_key(nodes: &[i64], key: i64) -> bool {
    key_kind(nodes, key) == TYD_ENUM && sym_prim_kind(nodes, key_sym(nodes, key)) == PRIM_OPTION
}

// ---------------------------------------------------------------------------
// Type-fact attachment (after all checking): variant facts and linearity.
//
// Both facts are computed once here, stored on arena rows, and only read
// downstream: codegen resolves variant tags from the recorded variant
// symbols (never by re-searching an enum's variant list by name), and the
// borrow checker reads the is_linear flag (never by re-matching handle
// names).
// ---------------------------------------------------------------------------

/// Fills the variant-fact rows codegen reads.  For every canonical enum
/// key, each variant's symbol (attached to its declaration by the
/// resolver) is recorded under (key, variant name id), so codegen
/// resolves a variant's declared-order tag from the symbol.
fn attach_variant_facts(nodes: &mut Vec<i64>, lists: &[Vec<i64>]) {
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_TYINFO && node_b(nodes, idx) == TYD_ENUM {
            let key = node_a(nodes, idx);
            let sym = node_c(nodes, idx);
            if sym != NONE {
                let decl = sym_decl(nodes, sym);
                if decl != NONE && node_tag(nodes, decl) == NODE_ITEM && node_a(nodes, decl) == ITEM_ENUM {
                    let variants = node_e(nodes, decl);
                    let count = list_len(lists, variants);
                    let mut v = 0i64;
                    while v < count {
                        let variant = list_get(lists, variants, v);
                        let vsym = variant_sym_of(nodes, variant);
                        if vsym != NONE {
                            alloc_varfact(nodes, key, node_a(nodes, variant), vsym);
                        }
                        v += 1;
                    }
                }
            }
        }
        idx += 1;
    }
}

/// The four native linear handles: the single definition of the
/// language's linear surfaces.  Their interned ids are matched at
/// collection time only; every later stage reads the stored flag.
const LINEAR_HANDLES: [&str; 4] = [
    "Memory.Block",
    "Collections.Vec",
    "Collections.String",
    "Collections.HashMap",
];

/// Computes the is_linear flag of every canonical key and stores it in
/// the descriptor row, once, after all checking (so every key the borrow
/// checker and codegen will query already exists).  Native handles are
/// linear by definition; structs and enums are linear when any declared
/// member (substituted against the key's own type arguments) is; arrays
/// follow their element.  A cycle (a recursive type) is not linear.  The
/// computation is memoized in the row's flag slot, so a key created on
/// the fly by substitution is computed recursively and never twice.
fn attach_linearity(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>) {
    let handles: Vec<i64> = LINEAR_HANDLES.iter().map(|text| intern(names, text)).collect();
    let mut seen: Vec<i64> = Vec::new();
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_TYINFO {
            seen.clear();
            linear_of(nodes, lists, node_a(nodes, idx), &handles, &mut seen);
        }
        idx += 1;
    }
}

fn has_value(list: &[i64], value: i64) -> bool {
    let mut idx = 0usize;
    while idx < list.len() {
        match list.get(idx) {
            Some(cell) => {
                if *cell == value {
                    return true;
                }
            }
            None => break,
        }
        idx += 1;
    }
    false
}

/// The linearity of one canonical key, memoized in the row's file slot
/// (-1 uncomputed, 0 not linear, 1 linear).  Dependencies are older keys
/// (arguments and elements are canonicalized before the containing key),
/// so the flag is well-founded; `seen` guards recursive type graphs.
fn linear_of(
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    key: i64,
    handles: &[i64],
    seen: &mut Vec<i64>,
) -> i64 {
    if key < 0 {
        return 0;
    }
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        return 0;
    }
    let stored = node_get(nodes, row, NODE_FILE);
    if stored == 0 || stored == 1 {
        return stored;
    }
    if has_value(seen, key) {
        return 0;
    }
    seen.push(key);
    let kind = node_b(nodes, row);
    let flag = if kind == TYD_NATIVE {
        let sym = node_c(nodes, row);
        if sym != NONE && has_value(handles, node_b(nodes, sym)) {
            1
        } else {
            0
        }
    } else if kind == TYD_ARRAY {
        linear_of(nodes, lists, node_e(nodes, row), handles, seen)
    } else if kind == TYD_STRUCT || kind == TYD_ENUM {
        linear_members_of(nodes, lists, node_c(nodes, row), key, handles, seen)
    } else {
        0
    };
    node_set(nodes, row, NODE_FILE, flag);
    flag
}

/// Whether a struct or enum declaration transitively contains a linear
/// member.  Each declared member type is substituted against the key's
/// own type arguments before the recursion, so a `T`-typed member counts
/// as linear exactly when its instantiated type is linear.
fn linear_members_of(
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    sym: i64,
    key: i64,
    handles: &[i64],
    seen: &mut Vec<i64>,
) -> i64 {
    if sym == NONE {
        return 0;
    }
    let decl = sym_decl(nodes, sym);
    if decl == NONE || node_tag(nodes, decl) != NODE_ITEM {
        return 0;
    }
    let kind = node_a(nodes, decl);
    let row = find_tyinfo(nodes, key);
    let args = if row == NONE { NONE } else { node_d(nodes, row) };
    if kind == ITEM_STRUCT {
        let fields = node_e(nodes, decl);
        let count = list_len(lists, fields);
        let mut idx = 0i64;
        while idx < count {
            let fty_node = node_b(nodes, list_get(lists, fields, idx));
            let fty = subst_declared_key(nodes, lists, decl, args, ty_key_of(nodes, fty_node));
            if linear_of(nodes, lists, fty, handles, seen) == 1 {
                return 1;
            }
            idx += 1;
        }
    } else if kind == ITEM_ENUM {
        let variants = node_e(nodes, decl);
        let count = list_len(lists, variants);
        let mut idx = 0i64;
        while idx < count {
            let payload = node_b(nodes, list_get(lists, variants, idx));
            let pcount = list_len(lists, payload);
            let mut pidx = 0i64;
            while pidx < pcount {
                let pty_node = list_get(lists, payload, pidx);
                let pty = subst_declared_key(nodes, lists, decl, args, ty_key_of(nodes, pty_node));
                if linear_of(nodes, lists, pty, handles, seen) == 1 {
                    return 1;
                }
                pidx += 1;
            }
            idx += 1;
        }
    }
    0
}

/// Substitutes a declared member type against the concrete type arguments
/// of `key`, matching the declaration's type parameters by key.
fn subst_declared_key(
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    decl: i64,
    args: i64,
    declared: i64,
) -> i64 {
    if declared == NONE {
        return declared;
    }
    let params = node_f(nodes, decl);
    let pcount = list_len(lists, params);
    let acount = list_len(lists, args);
    if pcount == 0 || pcount != acount {
        return declared;
    }
    let mut from: Vec<i64> = Vec::new();
    let mut to: Vec<i64> = Vec::new();
    let mut idx = 0i64;
    while idx < pcount {
        let param = list_get(lists, params, idx);
        if node_tag(nodes, param) == NODE_TY && node_a(nodes, param) == TY_PARAM {
            from.push(ty_key_of(nodes, param));
            to.push(list_get(lists, args, idx));
        }
        idx += 1;
    }
    if from.is_empty() {
        return declared;
    }
    subst_key(nodes, lists, declared, &from, &to)
}

/// Attaches the post-check type facts downstream stages consume: the
/// variant facts codegen needs and the is_linear flag the borrow checker
/// reads.  Runs after all checking and after inference variables are
/// substituted, so every canonical key that will be queried exists.
fn attach_type_facts(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>) {
    attach_variant_facts(nodes, lists);
    attach_linearity(names, nodes, lists);
}

// ---------------------------------------------------------------------------
// Unification.
// ---------------------------------------------------------------------------

/// Unifies two keys, binding any variables.  Returns whether the
/// unification succeeded; callers report mismatches with context.  The
/// merged key is never consumed anywhere, so it is not produced.
fn unify_key(nodes: &[i64], lists: &[Vec<i64>], vars: &mut Vec<(i64, i64)>, a: i64, b: i64) -> bool {
    let ra = resolve_var(vars, a);
    let rb = resolve_var(vars, b);
    if ra == rb {
        return true;
    }
    if is_var(ra) {
        bind_var(vars, ra, rb);
        return true;
    }
    if is_var(rb) {
        bind_var(vars, rb, ra);
        return true;
    }
    let row_a = find_tyinfo(nodes, ra);
    let row_b = find_tyinfo(nodes, rb);
    if row_a == NONE || row_b == NONE {
        return false;
    }
    if node_b(nodes, row_a) != node_b(nodes, row_b) {
        return false;
    }
    let kind = node_b(nodes, row_a);
    if kind == TYD_PARAM {
        return false;
    }
    if node_c(nodes, row_a) != node_c(nodes, row_b) {
        return false;
    }
    if node_f(nodes, row_a) != node_f(nodes, row_b) {
        return false;
    }
    let args_a = node_d(nodes, row_a);
    let args_b = node_d(nodes, row_b);
    let na = list_len(lists, args_a);
    let nb = list_len(lists, args_b);
    if na != nb {
        return false;
    }
    let mut idx = 0i64;
    while idx < na {
        if !unify_key(nodes, lists, vars, list_get(lists, args_a, idx), list_get(lists, args_b, idx)) {
            return false;
        }
        idx += 1;
    }
    let elem_a = node_e(nodes, row_a);
    let elem_b = node_e(nodes, row_b);
    if (elem_a != NONE || elem_b != NONE)
        && !unify_key(nodes, lists, vars, elem_a, elem_b) {
            return false;
        }
    true
}

/// Renders a key for diagnostics: `Int`, `Result(Int, RangeError)`,
/// `&[U8]`, `Vec(U8)`.
fn render_key(names: &[String], nodes: &[i64], lists: &[Vec<i64>], key: i64) -> String {
    if is_var(key) {
        return "?".to_string();
    }
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        return "?".to_string();
    }
    let kind = node_b(nodes, row);
    if kind == TYD_UNKNOWN {
        return "?".to_string();
    }
    if kind == TYD_PARAM {
        return name_text(names, node_c(nodes, row));
    }
    let sym = node_c(nodes, row);
    let elem = node_e(nodes, row);
    let args = node_d(nodes, row);
    if kind == TYD_REF {
        return format!("&{}", render_key(names, nodes, lists, elem));
    }
    if kind == TYD_REF_MUT {
        return format!("&mut {}", render_key(names, nodes, lists, elem));
    }
    if kind == TYD_SLICE {
        return format!("[{}]", render_key(names, nodes, lists, elem));
    }
    if kind == TYD_ARRAY {
        return format!("[{}; {}]", render_key(names, nodes, lists, elem), node_f(nodes, row));
    }
    let mut text = name_text(names, sym_name(nodes, sym));
    let count = list_len(lists, args);
    if count > 0 {
        let mut parts: Vec<String> = Vec::new();
        let mut idx = 0i64;
        while idx < count {
            parts.push(render_key(names, nodes, lists, list_get(lists, args, idx)));
            idx += 1;
        }
        text = format!("{}({})", text, parts.join(", "));
    }
    text
}

// ---------------------------------------------------------------------------
// Declaration collection (phase A): canonicalize every declared type.
// ---------------------------------------------------------------------------

fn collect_types(state: &mut State, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        collect_type_item(state, list_get(state.2, list, idx));
        idx += 1;
    }
}

fn collect_type_item(state: &mut State, item: i64) {
    if node_tag(state.1, item) != NODE_ITEM {
        return;
    }
    let kind = node_a(state.1, item);
    if kind == ITEM_MODULE {
        collect_types(state, node_e(state.1, item));
    } else if kind == ITEM_STRUCT {
        push_scope(state.4);
        bind_type_params(state.1, state.2, state.4, item, node_f(state.1, item));
        let fields = node_e(state.1, item);
        let count = list_len(state.2, fields);
        let mut idx = 0i64;
        while idx < count {
            let fty = node_b(state.1, list_get(state.2, fields, idx));
            canon_ty(state, fty, NONE, 1);
            idx += 1;
        }
        pop_scope(state.4);
    } else if kind == ITEM_ENUM {
        push_scope(state.4);
        bind_type_params(state.1, state.2, state.4, item, node_f(state.1, item));
        let variants = node_e(state.1, item);
        let count = list_len(state.2, variants);
        let mut idx = 0i64;
        while idx < count {
            let vty = node_b(state.1, list_get(state.2, variants, idx));
            canon_ty_list(state, vty, NONE, 1);
            idx += 1;
        }
        pop_scope(state.4);
    } else if kind == ITEM_NATIVE_TYPE {
        push_scope(state.4);
        bind_type_params(state.1, state.2, state.4, item, node_e(state.1, item));
        pop_scope(state.4);
    }
}

// ---------------------------------------------------------------------------
// Impls (phase B): record every impl and check its method bodies.
// ---------------------------------------------------------------------------

fn collect_impls(state: &mut State, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        collect_impl_item(state, list_get(state.2, list, idx));
        idx += 1;
    }
}

fn collect_impl_item(state: &mut State, item: i64) {
    if node_tag(state.1, item) != NODE_ITEM {
        return;
    }
    let kind = node_a(state.1, item);
    if kind == ITEM_MODULE {
        collect_impls(state, node_e(state.1, item));
        return;
    }
    if kind != ITEM_IMPL {
        return;
    }
    let trait_sym = item_sym_of(state.1, item);
    if trait_sym == NONE {
        return;
    }
    let for_ty = node_e(state.1, item);
    let for_key = canon_ty(state, for_ty, NONE, 1);
    let methods = node_f(state.1, item);
    let method_count = list_len(state.2, methods);
    let mut idx = 0i64;
    while idx < method_count {
        let method = list_get(state.2, methods, idx);
        check_fn(state, method, for_key);
        verify_impl_method(state, trait_sym, for_key, method);
        idx += 1;
    }
    state.5.push(trait_sym);
    state.5.push(for_key);
    state.5.push(methods);
}

/// Checks an impl method's signature against the trait declaration
/// (with Self replaced by the impl's for-type).
fn verify_impl_method(state: &mut State, trait_sym: i64, for_key: i64, method: i64) {
    let trait_item = sym_decl(state.1, trait_sym);
    if trait_item == NONE {
        return;
    }
    let trait_method = find_method_by_name(state.1, state.2, node_e(state.1, trait_item), node_a(state.1, method));
    if trait_method == NONE {
        let file = node_file(state.1, method);
        let start = node_start(state.1, method);
        let end = node_end(state.1, method);
        push_error(state.3, &format!("impl method '{}' does not match any trait method", name_text(state.0, node_a(state.1, method))), file, start, end);
        return;
    }
    push_scope(state.4);
    let mut t_vars: Vec<(i64, i64)> = Vec::new();
    let t_params = node_b(state.1, trait_method);
    let t_count = list_len(state.2, t_params);
    let mut pidx = 0i64;
    while pidx < t_count {
        let t_param = list_get(state.2, t_params, pidx);
        if node_tag(state.1, t_param) == NODE_TY && node_a(state.1, t_param) == TY_PARAM {
            bind(state.4, node_b(state.1, t_param), fresh_var_local(&mut t_vars), 0);
        }
        pidx += 1;
    }
    let self_var = fresh_var_local(&mut t_vars);
    bind(state.4, intern(state.0, "Self"), self_var, 0);
    let t_params = node_c(state.1, trait_method);
    let i_params = node_c(state.1, method);
    let tn = list_len(state.2, t_params);
    let in_count = list_len(state.2, i_params);
    let mut ok = tn == in_count;
    let mut idx = 0i64;
    while idx < tn {
        let t_param = list_get(state.2, t_params, idx);
        let t_ty = node_b(state.1, t_param);
        let t_key = canon_ty(state, t_ty, self_var, 0);
        let i_param = list_get(state.2, i_params, idx);
        let i_ty = node_b(state.1, i_param);
        let i_key = canon_ty(state, i_ty, for_key, 0);
        let unify_ok = unify_key(state.1, state.2, &mut t_vars, t_key, i_key);
        if !unify_ok {
            ok = false;
        }
        idx += 1;
    }
    let t_ret_ty = node_d(state.1, trait_method);
    let t_ret = canon_ty(state, t_ret_ty, self_var, 0);
    let i_ret_ty = node_d(state.1, method);
    let i_ret = canon_ty(state, i_ret_ty, for_key, 0);
    let ret_ok = unify_key(state.1, state.2, &mut t_vars, t_ret, i_ret);
    if !ret_ok {
        ok = false;
    }
    if in_count != tn {
        ok = false;
    }
    if !ok {
        let file = node_file(state.1, method);
        let start = node_start(state.1, method);
        let end = node_end(state.1, method);
        push_error(state.3, &format!("impl method '{}' signature does not match the trait declaration", name_text(state.0, node_a(state.1, method))), file, start, end);
    }
    pop_scope(state.4);
}

/// Finds a method function by name inside a method list.
fn find_method_by_name(nodes: &[i64], lists: &[Vec<i64>], methods: i64, name: i64) -> i64 {
    let count = list_len(lists, methods);
    let mut idx = 0i64;
    while idx < count {
        let method = list_get(lists, methods, idx);
        if node_a(nodes, method) == name {
            return method;
        }
        idx += 1;
    }
    NONE
}

/// Finds the impl row for `(trait_sym, for_key)`, or NONE.
fn impl_find(impls: &[i64], trait_sym: i64, for_key: i64) -> i64 {
    let mut idx = 0i64;
    while idx < impls.len() as i64 / IMPL_STRIDE {
        if impl_at(impls, (idx * IMPL_STRIDE) as usize) == trait_sym
            && impl_at(impls, (idx * IMPL_STRIDE + 1) as usize) == for_key
        {
            return idx;
        }
        idx += 1;
    }
    NONE
}

fn impl_methods(impls: &[i64], idx: i64) -> i64 {
    impl_at(impls, (idx * IMPL_STRIDE + 2) as usize)
}

/// Whether `key` is the seeded Unit enum, read from the primitive
/// sub-kind the resolver stored on the enum's symbol — never by name.
fn is_unit_key(nodes: &[i64], key: i64) -> bool {
    key_kind(nodes, key) == TYD_ENUM && sym_prim_kind(nodes, key_sym(nodes, key)) == PRIM_UNIT
}

/// The Unit key: the seeded Unit enum's canonical key, built from the
/// symbol id the checker stored on the state at setup.
fn unit_key_of(state: &mut State) -> i64 {
    let sym = state.8;
    if sym == NONE {
        unknown_key(state.1, state.2)
    } else {
        canon_tyinfo(state.1, state.2, TYD_ENUM, sym, NONE, NONE, NONE)
    }
}

// ---------------------------------------------------------------------------
// Function bodies (phase D).
// ---------------------------------------------------------------------------

fn check_fn_list(state: &mut State, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        check_fn_item(state, list_get(state.2, list, idx));
        idx += 1;
    }
}

fn check_fn_item(state: &mut State, item: i64) {
    if node_tag(state.1, item) != NODE_ITEM {
        return;
    }
    let kind = node_a(state.1, item);
    if kind == ITEM_MODULE {
        check_fn_list(state, node_e(state.1, item));
    } else if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
        check_fn(state, node_d(state.1, item), NONE);
    }
}

/// Walks an item list, canonicalizing every function signature inside.
fn check_fn_sigs_list(state: &mut State, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        check_fn_sigs_item(state, list_get(state.2, list, idx));
        idx += 1;
    }
}

fn check_fn_sigs_item(state: &mut State, item: i64) {
    if node_tag(state.1, item) != NODE_ITEM {
        return;
    }
    let kind = node_a(state.1, item);
    if kind == ITEM_MODULE {
        check_fn_sigs_list(state, node_e(state.1, item));
    } else if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
        check_fn_sigs(state, node_d(state.1, item));
    }
}

/// Canonicalizes one function's parameter and return type nodes under
/// its own type parameters, attaching the keys to the nodes.  Signature
/// facts are computed here once, before any call site or body reads
/// them; `check_fn` re-reads the attached keys instead of recomputing.
fn check_fn_sigs(state: &mut State, fn_node: i64) {
    if node_tag(state.1, fn_node) != NODE_FN {
        return;
    }
    push_scope(state.4);
    bind_type_params(state.1, state.2, state.4, fn_node, node_b(state.1, fn_node));
    let params = node_c(state.1, fn_node);
    let count = list_len(state.2, params);
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(state.2, params, idx);
        canon_ty(state, node_b(state.1, param), NONE, 1);
        idx += 1;
    }
    canon_ty(state, node_d(state.1, fn_node), NONE, 1);
    pop_scope(state.4);
}

/// Checks one function: binds its type parameters and parameters, infers
/// every body expression (a NONE body means a signature-only check, as
/// for native functions), and attaches every key.
fn check_fn(state: &mut State, fn_node: i64, self_key: i64) {
    if node_tag(state.1, fn_node) != NODE_FN {
        return;
    }
    push_scope(state.4);
    bind_type_params(state.1, state.2, state.4, fn_node, node_b(state.1, fn_node));
    let params = node_c(state.1, fn_node);
    let count = list_len(state.2, params);
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(state.2, params, idx);
        let param_ty = node_b(state.1, param);
        let key = canon_ty(state, param_ty, self_key, 1);
        bind(state.4, node_a(state.1, param), key, 0);
        idx += 1;
    }
    let ret_ty = node_d(state.1, fn_node);
    let ret = canon_ty(state, ret_ty, self_key, 1);
    let impure = node_e(state.1, fn_node);
    let body = node_f(state.1, fn_node);
    if body != NONE {
        check_stmt_list(state, body, ret, impure, self_key, 0);
    }
    pop_scope(state.4);
}

fn check_stmt_list(state: &mut State, list: i64, ret: i64, impure: i64, self_key: i64, loop_depth: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        check_stmt(state, list_get(state.2, list, idx), ret, impure, self_key, loop_depth);
        idx += 1;
    }
}

/// Checks one statement, returning its value key.
fn check_stmt(state: &mut State, stmt: i64, ret: i64, impure: i64, self_key: i64, loop_depth: i64) -> i64 {
    if node_tag(state.1, stmt) != NODE_STMT {
        return unit_key_of(state);
    }
    let kind = node_a(state.1, stmt);
    let file = node_file(state.1, stmt);
    let start = node_start(state.1, stmt);
    let end = node_end(state.1, stmt);
    if kind == STMT_LET {
        let name = node_c(state.1, stmt);
        let is_mut = node_b(state.1, stmt);
        let declared = node_d(state.1, stmt);
        let init = node_e(state.1, stmt);
        let binding_key = if declared != NONE {
            let dkey = canon_ty(state, declared, self_key, 1);
            let ikey = check_expr(state, init, dkey, ret, impure, self_key);
            let ok = unify_key(state.1, state.2, state.6, ikey, dkey);
            if !ok {
                push_error(state.3, &format!("cannot assign '{}' to '{}'", render_key(state.0, state.1, state.2, ikey), render_key(state.0, state.1, state.2, dkey)), file, start, end);
            }
            dkey
        } else {
            check_expr(state, init, NONE, ret, impure, self_key)
        };
        bind(state.4, name, binding_key, is_mut);
        stmt_set_ty(state.1, stmt, binding_key);
        return binding_key;
    }
    if kind == STMT_ASSIGN {
        let target = node_b(state.1, stmt);
        let value = node_c(state.1, stmt);
        // The target is a place expression; its type is the type the
        // value must have.  `check_assign_target` types it and enforces
        // the assignment rules (mutable local or `&mut` base; a shared
        // `&T` base is a hard error).
        let tkey = check_assign_target(state, target, ret, impure, self_key);
        let vkey = check_expr(state, value, tkey, ret, impure, self_key);
        let ok = unify_key(state.1, state.2, state.6, vkey, tkey);
        if !ok {
            push_error(state.3, &format!("cannot assign '{}' to '{}'", render_key(state.0, state.1, state.2, vkey), render_key(state.0, state.1, state.2, tkey)), file, start, end);
        }
        stmt_set_ty(state.1, stmt, tkey);
        return tkey;
    }
    if kind == STMT_WHILE {
        let cond = check_expr(state, node_b(state.1, stmt), NONE, ret, impure, self_key);
        if !is_bool_key(state.1, cond) {
            push_error(state.3, "while condition must be Bool", file, start, end);
        }
        push_scope(state.4);
        check_stmt_list(state, node_c(state.1, stmt), ret, impure, self_key, loop_depth + 1);
        pop_scope(state.4);
        let unit = unit_key_of(state);
        stmt_set_ty(state.1, stmt, unit);
        return unit;
    }
    if kind == STMT_IF {
        let cond = check_expr(state, node_b(state.1, stmt), NONE, ret, impure, self_key);
        if !is_bool_key(state.1, cond) {
            push_error(state.3, "if condition must be Bool", file, start, end);
        }
        push_scope(state.4);
        check_stmt_list(state, node_c(state.1, stmt), ret, impure, self_key, loop_depth);
        pop_scope(state.4);
        if node_d(state.1, stmt) != NONE {
            push_scope(state.4);
            check_stmt_list(state, node_d(state.1, stmt), ret, impure, self_key, loop_depth);
            pop_scope(state.4);
        }
        let unit = unit_key_of(state);
        stmt_set_ty(state.1, stmt, unit);
        return unit;
    }
    if kind == STMT_RETURN {
        let value = node_b(state.1, stmt);
        let key;
        if value == NONE {
            if !is_unit_key(state.1, ret) {
                push_error(state.3, &format!("return with no value in a function returning '{}'", render_key(state.0, state.1, state.2, ret)), file, start, end);
            }
            key = unit_key_of(state);
        } else {
            key = check_expr(state, value, ret, ret, impure, self_key);
            let ok = unify_key(state.1, state.2, state.6, key, ret);
            if !ok {
                push_error(state.3, &format!("return type mismatch: expected '{}', found '{}'", render_key(state.0, state.1, state.2, ret), render_key(state.0, state.1, state.2, key)), file, start, end);
            }
        }
        stmt_set_ty(state.1, stmt, key);
        return key;
    }
    if kind == STMT_BREAK || kind == STMT_CONTINUE {
        if loop_depth == 0 {
            let what = if kind == STMT_BREAK { "break" } else { "continue" };
            push_error(state.3, &format!("{} outside of a loop", what), file, start, end);
        }
        let unit = unit_key_of(state);
        stmt_set_ty(state.1, stmt, unit);
        return unit;
    }
    let expr = node_b(state.1, stmt);
    let key = check_expr(state, expr, NONE, ret, impure, self_key);
    if is_result_key(state.1, key) {
        push_error(state.3, "unhandled Result value: use try or match", file, start, end);
    } else if is_option_key(state.1, key) {
        push_error(state.3, "unhandled Option value: use try or match", file, start, end);
    }
    stmt_set_ty(state.1, stmt, key);
    key
}

// ---------------------------------------------------------------------------
// Constant folding (phase C).
// ---------------------------------------------------------------------------

fn collect_consts(state: &mut State, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        collect_const_item(state, list_get(state.2, list, idx));
        idx += 1;
    }
}

fn collect_const_item(state: &mut State, item: i64) {
    if node_tag(state.1, item) != NODE_ITEM {
        return;
    }
    let kind = node_a(state.1, item);
    if kind == ITEM_MODULE {
        collect_consts(state, node_e(state.1, item));
        return;
    }
    if kind != ITEM_CONST {
        return;
    }
    let sym = item_sym_of(state.1, item);
    let decl_ty = node_e(state.1, item);
    let declared = canon_ty(state, decl_ty, NONE, 1);
    let value_expr = node_f(state.1, item);
    let file = node_file(state.1, item);
    let start = node_start(state.1, item);
    let end = node_end(state.1, item);
    let (value, key) = fold_const(state, value_expr, declared, 0);
    let ok = unify_key(state.1, state.2, state.6, key, declared);
    if !ok {
        push_error(state.3, &format!("constant initializer type mismatch: expected '{}', found '{}'", render_key(state.0, state.1, state.2, declared), render_key(state.0, state.1, state.2, key)), file, start, end);
    }
    alloc_node(state.1, &[NODE_CONSTVAL, NO_FILE, NO_FILE, NO_FILE, sym, value, NONE, NONE, NONE, NONE]);
    expr_set_ty(state.1, value_expr, key);
}

/// Folds a constant expression, returning `(value, key)`.  On failure an
/// error is reported and the unknown key is returned.  With `quiet` set
/// (the statically-known-zero-divisor probe) failures are silent: the
/// unknown key means "not statically known".
fn fold_const(state: &mut State, expr: i64, declared: i64, quiet: i64) -> (i64, i64) {
    if node_tag(state.1, expr) != NODE_EXPR {
        if quiet == 0 {
            push_error(state.3, "constant expression required", node_file(state.1, expr), node_start(state.1, expr), node_end(state.1, expr));
        }
        return (0, unknown_key(state.1, state.2));
    }
    let kind = node_a(state.1, expr);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    if kind == EXPR_LIT {
        let lit = node_b(state.1, expr);
        let value = node_c(state.1, expr);
        if lit == LIT_TRUE || lit == LIT_FALSE {
            return (value, builtin_key_of(state.1, BUILTIN_BOOL));
        }
        let key = if is_int_key(state.1, declared) {
            declared
        } else {
            builtin_key_of(state.1, BUILTIN_INT)
        };
        return (value, key);
    }
    if kind == EXPR_UNARY && node_b(state.1, expr) == UN_NEG {
        let (value, key) = fold_const(state, node_c(state.1, expr), declared, quiet);
        if key_kind(state.1, key) == TYD_UNKNOWN {
            return (0, key);
        }
        return (value.wrapping_neg(), key);
    }
    if kind == EXPR_PATH {
        let sym = expr_sym_of(state.1, expr);
        if sym != NONE && sym_kind(state.1, sym) == SYM_CONST {
            let item = sym_decl(state.1, sym);
            if !has_const_value(state.1, sym) {
                if quiet == 0 {
                    push_error(state.3, &format!("constant '{}' must be declared before use", name_text(state.0, node_d(state.1, item))), file, start, end);
                }
                return (0, unknown_key(state.1, state.2));
            }
            let value = find_const_value(state.1, sym);
            let key = ty_key_of(state.1, node_e(state.1, item));
            return (value, key);
        }
        if quiet == 0 {
            push_error(state.3, "constant expression required", file, start, end);
        }
        return (0, unknown_key(state.1, state.2));
    }
    if kind == EXPR_BINARY {
        let (lv, lk) = fold_const(state, node_c(state.1, expr), declared, quiet);
        if key_kind(state.1, lk) == TYD_UNKNOWN {
            return (0, lk);
        }
        let (rv, rk) = fold_const(state, node_d(state.1, expr), declared, quiet);
        if key_kind(state.1, rk) == TYD_UNKNOWN {
            return (0, rk);
        }
        let ok = unify_key(state.1, state.2, state.6, lk, rk);
        if !ok {
            if quiet == 0 {
                push_error(state.3, "constant operands have different types", file, start, end);
            }
            return (0, unknown_key(state.1, state.2));
        }
        return fold_bin(state, node_b(state.1, expr), lv, rv, lk, (file, start, end), quiet);
    }
    if quiet == 0 {
        push_error(state.3, "constant expression required", file, start, end);
    }
    (0, unknown_key(state.1, state.2))
}

/// Euclidean division on i64, computed in wrapping arithmetic so no input
/// can panic (matching the emitted runtime IR; the spec defines both `/`
/// and `%` to keep the remainder in `[0, |divisor|)` regardless of the
/// operands' signs).
fn euclid_div_i64(lv: i64, rv: i64) -> i64 {
    let rem = lv.wrapping_rem(rv);
    let euclid_rem = if rem < 0 {
        rem.wrapping_add(rv.wrapping_abs())
    } else {
        rem
    };
    lv.wrapping_sub(euclid_rem).wrapping_div(rv)
}

/// The Euclidean remainder of `lv mod rv`, in wrapping arithmetic.
fn euclid_rem_i64(lv: i64, rv: i64) -> i64 {
    let rem = lv.wrapping_rem(rv);
    if rem < 0 {
        rem.wrapping_add(rv.wrapping_abs())
    } else {
        rem
    }
}

fn fold_bin(state: &mut State, op: i64, lv: i64, rv: i64, key: i64, span: (i64, i64, i64), quiet: i64) -> (i64, i64) {
    let (file, start, end) = span;
    let bool_key = builtin_key_of(state.1, BUILTIN_BOOL);
    if op == BIN_ADD {
        return (lv.wrapping_add(rv), key);
    }
    if op == BIN_SUB {
        return (lv.wrapping_sub(rv), key);
    }
    if op == BIN_MUL {
        return (lv.wrapping_mul(rv), key);
    }
    if op == BIN_DIV {
        if rv == 0 {
            if quiet == 0 {
                push_error(state.3, "division by zero in constant", file, start, end);
            }
            return (0, unknown_key(state.1, state.2));
        }
        return (euclid_div_i64(lv, rv), key);
    }
    if op == BIN_MOD {
        if rv == 0 {
            if quiet == 0 {
                push_error(state.3, "modulo by zero in constant", file, start, end);
            }
            return (0, unknown_key(state.1, state.2));
        }
        return (euclid_rem_i64(lv, rv), key);
    }
    if op == BIN_SHL {
        return (lv.wrapping_shl(rv as u32), key);
    }
    if op == BIN_SHR {
        return (lv.wrapping_shr(rv as u32), key);
    }
    if op == BIN_BAND {
        return (lv & rv, key);
    }
    if op == BIN_BOR {
        return (lv | rv, key);
    }
    if op == BIN_BXOR {
        return (lv ^ rv, key);
    }
    if op == BIN_EQ {
        return ((lv == rv) as i64, bool_key);
    }
    if op == BIN_NE {
        return ((lv != rv) as i64, bool_key);
    }
    if op == BIN_LT {
        return ((lv < rv) as i64, bool_key);
    }
    if op == BIN_GT {
        return ((lv > rv) as i64, bool_key);
    }
    if op == BIN_LE {
        return ((lv <= rv) as i64, bool_key);
    }
    if op == BIN_GE {
        return ((lv >= rv) as i64, bool_key);
    }
    if op == BIN_AND {
        return ((lv & rv), bool_key);
    }
    if op == BIN_OR {
        return ((lv | rv), bool_key);
    }
    if quiet == 0 {
        push_error(state.3, "unknown constant operator", file, start, end);
    }
    (0, unknown_key(state.1, state.2))
}

// ---------------------------------------------------------------------------
// Expressions.
// ---------------------------------------------------------------------------

/// Checks an expression against an expected type (NONE when none is
/// known), returning its key.
fn check_expr(state: &mut State, expr: i64, expected: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    if node_tag(state.1, expr) != NODE_EXPR {
        return unknown_key(state.1, state.2);
    }
    let kind = node_a(state.1, expr);
    if kind == EXPR_LIT {
        return check_lit(state.1, expr, expected);
    }
    if kind == EXPR_PATH {
        return check_path(state, expr);
    }
    if kind == EXPR_UNARY {
        return check_unary(state, expr, ret, impure, self_key);
    }
    if kind == EXPR_BINARY {
        return check_binary(state, expr, ret, impure, self_key);
    }
    if kind == EXPR_CALL {
        return check_call(state, expr, expected, ret, impure, self_key);
    }
    if kind == EXPR_STRUCT_LIT {
        return check_struct_lit(state, expr, expected, ret, impure, self_key);
    }
    if kind == EXPR_ARRAY {
        return check_array(state, expr, expected, ret, impure, self_key);
    }
    if kind == EXPR_MATCH {
        return check_match(state, expr, ret, impure, self_key);
    }
    if kind == EXPR_TRY {
        return check_try(state, expr, ret, impure, self_key);
    }
    if kind == EXPR_INDEX {
        return check_index(state, expr, 0, ret, impure, self_key);
    }
    if kind == EXPR_FIELD_ACCESS {
        return check_field_access(state, expr, ret, impure, self_key);
    }
    push_error(state.3, "malformed expression", node_file(state.1, expr), node_start(state.1, expr), node_end(state.1, expr));
    let key = unknown_key(state.1, state.2);
    expr_set_ty(state.1, expr, key);
    key
}

fn check_lit(nodes: &mut [i64], expr: i64, expected: i64) -> i64 {
    let lit = node_b(nodes, expr);
    if lit == LIT_TRUE || lit == LIT_FALSE {
        let key = builtin_key_of(nodes, BUILTIN_BOOL);
        expr_set_ty(nodes, expr, key);
        return key;
    }
    let key = if is_int_key(nodes, expected) {
        expected
    } else {
        builtin_key_of(nodes, BUILTIN_INT)
    };
    expr_set_ty(nodes, expr, key);
    key
}

fn check_unary(state: &mut State, expr: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let op = node_b(state.1, expr);
    let operand = node_c(state.1, expr);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    let key;
    if (op == UN_REF || op == UN_REF_MUT) && node_tag(state.1, operand) == NODE_EXPR && node_a(state.1, operand) == EXPR_INDEX {
        // A borrow of an indexed element is checked inside the index
        // expression itself: its type is the borrowed element type
        // (`&T`, or `Result(&T, IndexError)` when the access is
        // dynamically checked), never a reference to the Result.  The
        // borrow applies to the element, not to the bounds check.
        let borrow = if op == UN_REF { 1 } else { 2 };
        key = check_index(state, operand, borrow, ret, impure, self_key);
    } else {
        let inner = check_expr(state, operand, NONE, ret, impure, self_key);
        if op == UN_REF {
            key = canon_tyinfo(state.1, state.2, TYD_REF, NONE, NONE, inner, NONE);
        } else if op == UN_REF_MUT {
            key = canon_tyinfo(state.1, state.2, TYD_REF_MUT, NONE, NONE, inner, NONE);
        } else if op == UN_NEG {
            if !is_int_key(state.1, inner) {
                push_error(state.3, "unary '-' requires an integer operand", file, start, end);
            }
            key = inner;
        } else {
            if !is_bool_key(state.1, inner) {
                push_error(state.3, "unary '!' requires a Bool operand", file, start, end);
            }
            key = inner;
        }
    }
    expr_set_ty(state.1, expr, key);
    key
}

fn op_text(op: i64) -> &'static str {
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
    } else {
        "||"
    }
}

/// Reports a division or modulo whose divisor is statically known to be
/// zero — a literal, a folded const reference, or any arithmetic
/// combination of them — wherever the expression appears.  The numerator
/// is irrelevant: `N / 0`, `5 / 0`, and `x / (3 - 3)` are all compile
/// errors.  The constant fold is the single source of truth: this probe
/// folds the divisor with errors suppressed and only reports when the
/// fold proves zero.  Everything else is a runtime `Result`, never a
/// trap.
fn check_static_zero_divisor(state: &mut State, op: i64, rhs: i64) {
    if op != BIN_DIV && op != BIN_MOD {
        return;
    }
    let (value, key) = fold_const(state, rhs, NONE, 1);
    if key_kind(state.1, key) != TYD_UNKNOWN && value == 0 {
        let message = if op == BIN_DIV {
            "division by zero"
        } else {
            "modulo by zero"
        };
        push_error(state.3, message, node_file(state.1, rhs), node_start(state.1, rhs), node_end(state.1, rhs));
    }
}

/// The result key of a division or modulo expression: `Result(T,
/// DivError)` where T is the operand type.  Both enums are synthesized
/// builtins read through the same declaration path as any user enum, so
/// the variant order and layout are derived, never hardcoded.
fn division_result_key(state: &mut State, payload: i64) -> i64 {
    let err_sym = state.11;
    if err_sym == NONE {
        return unknown_key(state.1, state.2);
    }
    let err_key = canon_tyinfo(state.1, state.2, TYD_ENUM, err_sym, NONE, NONE, NONE);
    let result_sym = state.9;
    if result_sym == NONE {
        return unknown_key(state.1, state.2);
    }
    let args = alloc_list(state.2);
    list_push(state.2, args, payload);
    list_push(state.2, args, err_key);
    canon_tyinfo(state.1, state.2, TYD_ENUM, result_sym, args, NONE, NONE)
}

fn check_binary(state: &mut State, expr: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let op = node_b(state.1, expr);
    let lhs = node_c(state.1, expr);
    let rhs = node_d(state.1, expr);
    check_static_zero_divisor(state, op, rhs);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    let bool_key = builtin_key_of(state.1, BUILTIN_BOOL);
    if op == BIN_AND || op == BIN_OR {
        let l = check_expr(state, lhs, NONE, ret, impure, self_key);
        let r = check_expr(state, rhs, NONE, ret, impure, self_key);
        if !is_bool_key(state.1, l) {
            push_error(state.3, &format!("logical operator '{}' requires Bool operands", op_text(op)), file, start, end);
        }
        let ok = unify_key(state.1, state.2, state.6, l, r);
        if !ok {
            push_error(state.3, "logical operands have different types", file, start, end);
        }
        expr_set_ty(state.1, expr, bool_key);
        return bool_key;
    }
    if (BIN_EQ..=BIN_GE).contains(&op) {
        let l = check_expr(state, lhs, NONE, ret, impure, self_key);
        let r = check_expr(state, rhs, NONE, ret, impure, self_key);
        let ok = unify_key(state.1, state.2, state.6, l, r);
        if !ok {
            push_error(state.3, &format!("comparison '{}' requires operands of the same type", op_text(op)), file, start, end);
        }
        expr_set_ty(state.1, expr, bool_key);
        return bool_key;
    }
    let l = check_expr(state, lhs, NONE, ret, impure, self_key);
    if !is_int_key(state.1, l) {
        push_error(state.3, &format!("binary operator '{}' requires integer operands", op_text(op)), file, start, end);
    }
    let r = check_expr(state, rhs, l, ret, impure, self_key);
    let ok = unify_key(state.1, state.2, state.6, l, r);
    if !ok {
        push_error(state.3, &format!("binary operator '{}' requires operands of the same type", op_text(op)), file, start, end);
    }
    if op == BIN_DIV || op == BIN_MOD {
        let key = division_result_key(state, l);
        expr_set_ty(state.1, expr, key);
        return key;
    }
    expr_set_ty(state.1, expr, l);
    l
}

fn check_array(state: &mut State, expr: i64, expected: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let elems = node_b(state.1, expr);
    let count = list_len(state.2, elems);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    if count == 0 {
        if expected != NONE && (key_kind(state.1, expected) == TYD_ARRAY || key_kind(state.1, expected) == TYD_SLICE) {
            expr_set_ty(state.1, expr, expected);
            return expected;
        }
        push_error(state.3, "cannot infer the element type of an empty array", file, start, end);
        let key = unknown_key(state.1, state.2);
        expr_set_ty(state.1, expr, key);
        return key;
    }
    let first = check_expr(state, list_get(state.2, elems, 0), NONE, ret, impure, self_key);
    let mut idx = 1i64;
    while idx < count {
        let key = check_expr(state, list_get(state.2, elems, idx), first, ret, impure, self_key);
        let ok = unify_key(state.1, state.2, state.6, key, first);
        if !ok {
            push_error(state.3, "array elements must have the same type", file, start, end);
        }
        idx += 1;
    }
    let key = canon_tyinfo(state.1, state.2, TYD_ARRAY, NONE, NONE, first, count);
    expr_set_ty(state.1, expr, key);
    key
}

/// Whether `key` is (or transitively contains) a linear handle, computed
/// on demand during checking.  The full linearity pass runs after all
/// checking; a key queried here is memoized into its descriptor row the
/// same way, so the pass reads the stored flag and never recomputes it.
fn key_is_linear_now(state: &mut State, key: i64) -> bool {
    if key == NONE {
        return false;
    }
    let mut handles: Vec<i64> = Vec::new();
    let mut h_idx = 0usize;
    while h_idx < LINEAR_HANDLES.len() {
        match LINEAR_HANDLES.get(h_idx) {
            Some(text) => handles.push(intern(state.0, text)),
            None => break,
        }
        h_idx += 1;
    }
    let mut seen: Vec<i64> = Vec::new();
    linear_of(state.1, state.2, key, &handles, &mut seen) == 1
}

/// The result key of an index expression: `Result(T, IndexError)` where
/// T is the element (or borrowed element) type.  IndexError is the
/// seeded primitive enum carrying `IndexOutOfBounds(Usize, Usize)`, read
/// through the same declaration path as any user enum.
fn index_result_key(state: &mut State, payload: i64) -> i64 {
    let err_sym = state.12;
    if err_sym == NONE {
        return unknown_key(state.1, state.2);
    }
    let err_key = canon_tyinfo(state.1, state.2, TYD_ENUM, err_sym, NONE, NONE, NONE);
    let result_sym = state.9;
    if result_sym == NONE {
        return unknown_key(state.1, state.2);
    }
    let args = alloc_list(state.2);
    list_push(state.2, args, payload);
    list_push(state.2, args, err_key);
    canon_tyinfo(state.1, state.2, TYD_ENUM, result_sym, args, NONE, NONE)
}

/// Checks an array/slice index expression `base[index]`.  `borrow` is 0
/// for a value position, 1 under `&`, 2 under `&mut`.  A fixed-size
/// array with a statically-known constant index is proven in range (or
/// rejected) at compile time and evaluates directly to the element type
/// `T` (or `&T`/`&mut T` when borrowed), with no `Result` wrapper.
/// Every dynamic index, and every index into a slice, evaluates to
/// `Result(T, IndexError)` (or the borrowed-element variant when under a
/// borrow).  A value-position index of a linear-element array or slice
/// is a compile error: an indexed element is never moved out.
fn check_index(state: &mut State, expr: i64, borrow: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let base = node_b(state.1, expr);
    let index = node_c(state.1, expr);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    let usize_key = builtin_key_of(state.1, BUILTIN_USIZE);
    let base_key = check_expr(state, base, NONE, ret, impure, self_key);
    let idx_key = check_expr(state, index, usize_key, ret, impure, self_key);
    let idx_ok = unify_key(state.1, state.2, state.6, usize_key, idx_key);
    if !idx_ok {
        push_error(state.3, "array index must be Usize", file, start, end);
    }
    let base_kind = key_kind(state.1, base_key);
    let (elem_key, fixed_len) = if base_kind == TYD_ARRAY {
        (key_elem(state.1, base_key), key_len(state.1, base_key))
    } else if base_kind == TYD_REF || base_kind == TYD_REF_MUT || base_kind == TYD_SLICE {
        let inner = key_elem(state.1, base_key);
        let slice_elem = if key_kind(state.1, inner) == TYD_SLICE {
            key_elem(state.1, inner)
        } else {
            NONE
        };
        if slice_elem == NONE {
            push_error(state.3, "cannot index a value that is not an array or slice", file, start, end);
        }
        (slice_elem, NONE)
    } else {
        push_error(state.3, "cannot index a value that is not an array or slice", file, start, end);
        (unknown_key(state.1, state.2), NONE)
    };
    if elem_key != NONE && borrow == 0 && key_is_linear_now(state, elem_key) {
        push_error(state.3, "cannot move linear element out of array by index: borrow with & or &mut instead", file, start, end);
    }
    let payload = if borrow == 1 {
        canon_tyinfo(state.1, state.2, TYD_REF, NONE, NONE, elem_key, NONE)
    } else if borrow == 2 {
        canon_tyinfo(state.1, state.2, TYD_REF_MUT, NONE, NONE, elem_key, NONE)
    } else {
        elem_key
    };
    if base_kind == TYD_ARRAY && fixed_len != NONE {
        let (value, ckey) = fold_const(state, index, NONE, 1);
        if key_kind(state.1, ckey) != TYD_UNKNOWN {
            if value < 0 || value >= fixed_len {
                push_error(state.3, &format!("array index out of bounds: index is {} but array length is {}", value, fixed_len), file, start, end);
            }
            expr_set_ty(state.1, expr, payload);
            return payload;
        }
    }
    let key = index_result_key(state, payload);
    expr_set_ty(state.1, expr, key);
    key
}

/// Checks a field access on a non-path base (`(expr).field`): the base
/// is typed (references dereferenced) and the substituted field key is
/// returned.
fn check_field_access(state: &mut State, expr: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let base = node_b(state.1, expr);
    let field = node_c(state.1, expr);
    let base_key = check_expr(state, base, NONE, ret, impure, self_key);
    let key = field_access_key(state.0, state.1, state.2, state.3, expr, base_key, field);
    expr_set_ty(state.1, expr, key);
    key
}

/// Checks an assignment target expression (a place) and returns the key
/// of the value it accepts.  The rules: a plain name target must be a
/// mutable local (`var`); a field chain may be rooted at a mutable local
/// or at a `&mut T` reference (writing through it); writing through a
/// shared `&T` reference is a hard error, as is any target that is not a
/// place.
fn check_assign_target(state: &mut State, target: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    if node_tag(state.1, target) != NODE_EXPR {
        return unknown_key(state.1, state.2);
    }
    let file = node_file(state.1, target);
    let start = node_start(state.1, target);
    let end = node_end(state.1, target);
    let kind = node_a(state.1, target);
    if kind == EXPR_PATH {
        let segs = node_b(state.1, target);
        let count = list_len(state.2, segs);
        let first = list_get(state.2, segs, 0);
        let found = lookup(state.4, first);
        if found.0 == NONE {
            push_error(state.3, &format!("unknown symbol '{}'", name_text(state.0, first)), file, start, end);
            let key = unknown_key(state.1, state.2);
            expr_set_ty(state.1, target, key);
            return key;
        }
        if count == 1 {
            if found.1 == 0 {
                push_error(state.3, &format!("cannot assign to '{}': assignment requires var", name_text(state.0, first)), file, start, end);
            }
            expr_set_ty(state.1, target, found.0);
            return found.0;
        }
        check_field_target_base(state, found, first, file, start, end);
        let mut current = found.0;
        let mut idx = 1i64;
        while idx < count {
            current = field_access_key(state.0, state.1, state.2, state.3, target, current, list_get(state.2, segs, idx));
            idx += 1;
        }
        expr_set_ty(state.1, target, current);
        return current;
    }
    if kind == EXPR_FIELD_ACCESS {
        let base = node_b(state.1, target);
        let field = node_c(state.1, target);
        let base_key = check_expr(state, base, NONE, ret, impure, self_key);
        let bkind = key_kind(state.1, base_key);
        if bkind == TYD_REF {
            push_error(state.3, &format!("cannot assign to field '{}' through shared reference '{}': assignment requires &mut", name_text(state.0, field), render_key(state.0, state.1, state.2, base_key)), file, start, end);
        } else if bkind == TYD_REF_MUT {
            // Writable through the exclusive reference.
        } else if node_tag(state.1, base) == NODE_EXPR && node_a(state.1, base) == EXPR_PATH {
            let segs = node_b(state.1, base);
            let first = list_get(state.2, segs, 0);
            let found = lookup(state.4, first);
            if found.0 == NONE {
                push_error(state.3, &format!("unknown symbol '{}'", name_text(state.0, first)), file, start, end);
            } else if found.1 == 0 {
                push_error(state.3, &format!("cannot assign to field '{}' of '{}': assignment requires var", name_text(state.0, field), name_text(state.0, first)), file, start, end);
            }
        } else {
            push_error(state.3, &format!("cannot assign to field '{}': the target is not a mutable place", name_text(state.0, field)), file, start, end);
        }
        let key = field_access_key(state.0, state.1, state.2, state.3, target, base_key, field);
        expr_set_ty(state.1, target, key);
        return key;
    }
    push_error(state.3, "invalid assignment target", file, start, end);
    let key = unknown_key(state.1, state.2);
    expr_set_ty(state.1, target, key);
    key
}

/// The assignability of a field-chain target rooted at the local binding
/// `found` (`(key, is_mut)` with name `name`): a mutable local is
/// writable, a `&mut T` reference is writable through, and a `&T` shared
/// reference is the hard error.
fn check_field_target_base(state: &mut State, found: (i64, i64), name: i64, file: i64, start: i64, end: i64) {
    let bkind = key_kind(state.1, found.0);
    if bkind == TYD_REF {
        push_error(state.3, &format!("cannot assign to a field through shared reference '{}': assignment requires &mut", render_key(state.0, state.1, state.2, found.0)), file, start, end);
    } else if bkind != TYD_REF_MUT && found.1 == 0 {
        push_error(state.3, &format!("cannot assign to field of '{}': assignment requires var", name_text(state.0, name)), file, start, end);
    }
}

fn check_try(state: &mut State, expr: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let inner = node_b(state.1, expr);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    let key = check_expr(state, inner, NONE, ret, impure, self_key);
    let result;
    if is_result_key(state.1, key) {
        if !is_result_key(state.1, ret) {
            push_error(state.3, "try on Result requires the enclosing function to return Result", file, start, end);
            result = unknown_key(state.1, state.2);
        } else {
            let args = key_args(state.1, key);
            let ret_args = key_args(state.1, ret);
            let err_key = list_get(state.2, args, 1);
            let ret_err = list_get(state.2, ret_args, 1);
            let err_ok = unify_key(state.1, state.2, state.6, err_key, ret_err);
            if !err_ok {
                push_error(state.3, "try error type does not match the function's return type", file, start, end);
            }
            result = list_get(state.2, args, 0);
        }
    } else if is_option_key(state.1, key) {
        if !is_option_key(state.1, ret) {
            push_error(state.3, "try on Option requires the enclosing function to return Option", file, start, end);
            result = unknown_key(state.1, state.2);
        } else {
            let args = key_args(state.1, key);
            result = list_get(state.2, args, 0);
        }
    } else {
        push_error(state.3, "try requires a Result or Option operand", file, start, end);
        result = unknown_key(state.1, state.2);
    }
    expr_set_ty(state.1, expr, result);
    result
}

// ---------------------------------------------------------------------------
// Paths.  A path either names a declaration (a symbol the resolver
// attached) or is a local-variable chain (`self.field`): the first segment
// is a local binding and every further segment is a struct field.
// ---------------------------------------------------------------------------

fn check_path(state: &mut State, expr: i64) -> i64 {
    let sym = expr_sym_of(state.1, expr);
    if sym != NONE {
        return check_path_sym(state, expr, NONE, sym);
    }
    check_local_chain(state.0, state.1, state.2, state.3, state.4, expr)
}

fn check_path_sym(state: &mut State, expr: i64, expected: i64, sym: i64) -> i64 {
    let kind = sym_kind(state.1, sym);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    if kind == SYM_CONST {
        let item = sym_decl(state.1, sym);
        let key = ty_key_of(state.1, node_e(state.1, item));
        expr_set_ty(state.1, expr, key);
        return key;
    }
    if kind == SYM_VARIANT {
        return variant_value_key(state, expr, expected, sym);
    }
    if kind == SYM_FUN || kind == SYM_NATIVE_FUN || kind == SYM_TRAIT_METHOD || kind == SYM_IMPL_METHOD {
        push_error(state.3, "function used as a value", file, start, end);
    } else {
        push_error(state.3, "type or module used as a value", file, start, end);
    }
    let key = unknown_key(state.1, state.2);
    expr_set_ty(state.1, expr, key);
    key
}

/// The key of a unit-variant value expression (`None`, `Unit`).  The
/// enum's type parameters are fresh inference variables unified with the
/// expected type when one is known.
fn variant_value_key(state: &mut State, expr: i64, expected: i64, sym: i64) -> i64 {
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    let decl = sym_decl(state.1, sym);
    let key;
    if decl == NONE {
        key = unit_key_of(state);
    } else {
        let enum_sym = enum_sym_of_variant(state.1, sym);
        if enum_sym == NONE {
            push_error(state.3, "cannot find the enum of this variant", file, start, end);
            key = unknown_key(state.1, state.2);
        } else {
            let item = sym_decl(state.1, enum_sym);
            key = enum_key_with_fresh(state.1, state.2, state.6, state.7, expr, enum_sym, item);
        }
    }
    if expected != NONE {
        let ok = unify_key(state.1, state.2, state.6, key, expected);
        if !ok {
            push_error(state.3, "variant value type mismatch", file, start, end);
        }
    }
    expr_set_ty(state.1, expr, key);
    key
}

fn check_local_chain(names: &mut [String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, env: &mut [Vec<i64>], expr: i64) -> i64 {
    let segs = node_b(nodes, expr);
    let count = list_len(lists, segs);
    let file = node_file(nodes, expr);
    let start = node_start(nodes, expr);
    let end = node_end(nodes, expr);
    let first = list_get(lists, segs, 0);
    let found = lookup(env, first);
    if found.0 == NONE {
        push_error(errors, &format!("unknown symbol '{}'", name_text(names, first)), file, start, end);
        let key = unknown_key(nodes, lists);
        expr_set_ty(nodes, expr, key);
        return key;
    }
    let mut current = found.0;
    let mut idx = 1i64;
    while idx < count {
        current = field_access_key(names, nodes, lists, errors, expr, current, list_get(lists, segs, idx));
        idx += 1;
    }
    expr_set_ty(nodes, expr, current);
    current
}

/// The key of `base.field`, dereferencing shared/mutable references and
/// substituting the struct's type arguments into the declared field key.
fn field_access_key(names: &mut [String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, expr: i64, base: i64, field: i64) -> i64 {
    let file = node_file(nodes, expr);
    let start = node_start(nodes, expr);
    let end = node_end(nodes, expr);
    let mut eff = base;
    if key_kind(nodes, eff) == TYD_REF || key_kind(nodes, eff) == TYD_REF_MUT {
        eff = key_elem(nodes, eff);
    }
    if key_kind(nodes, eff) != TYD_STRUCT {
        push_error(errors, &format!("cannot access field '{}' of a non-struct type", name_text(names, field)), file, start, end);
        return unknown_key(nodes, lists);
    }
    let item = sym_decl(nodes, key_sym(nodes, eff));
    let (found_idx, declared_key) = struct_field_of(nodes, lists, item, field);
    if found_idx == NONE {
        push_error(errors, &format!("no field '{}' on type '{}'", name_text(names, field), render_key(names, nodes, lists, eff)), file, start, end);
        return unknown_key(nodes, lists);
    }
    let from = declared_param_keys(nodes, lists, item);
    let to = list_to_vec(lists, key_args(nodes, eff));
    subst_key(nodes, lists, declared_key, &from, &to)
}

/// Returns `(index, declared-key)` of the named field, or (NONE, NONE).
fn struct_field_of(nodes: &[i64], lists: &[Vec<i64>], item: i64, name: i64) -> (i64, i64) {
    let fields = node_e(nodes, item);
    let count = list_len(lists, fields);
    let mut idx = 0i64;
    while idx < count {
        let field = list_get(lists, fields, idx);
        if node_a(nodes, field) == name {
            return (idx, ty_key_of(nodes, node_b(nodes, field)));
        }
        idx += 1;
    }
    (NONE, NONE)
}

/// The symbol of the enum that declares `variant_sym`, or NONE.
fn enum_sym_of_variant(nodes: &[i64], variant_sym: i64) -> i64 {
    let home = sym_home(nodes, variant_sym);
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_SYM && node_a(nodes, idx) == SYM_ENUM && node_e(nodes, idx) == home {
            return idx;
        }
        idx += 1;
    }
    NONE
}

/// The symbol of the trait that declares `method_sym`, or NONE.
fn trait_sym_of_method(nodes: &[i64], method_sym: i64) -> i64 {
    let home = sym_home(nodes, method_sym);
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_SYM && node_a(nodes, idx) == SYM_TRAIT && node_e(nodes, idx) == home {
            return idx;
        }
        idx += 1;
    }
    NONE
}

/// The enum key of `enum_sym` with one fresh inference variable per
/// declared type parameter.
fn enum_key_with_fresh(nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, vars: &mut Vec<(i64, i64)>, origins: &mut Vec<(i64, i64, i64)>, expr: i64, enum_sym: i64, item: i64) -> i64 {
    let args = fresh_args_for(nodes, lists, vars, origins, expr, item);
    canon_tyinfo(nodes, lists, TYD_ENUM, enum_sym, args, NONE, NONE)
}

/// The struct key of `sym` with one fresh inference variable per declared
/// type parameter.
fn struct_key_with_fresh(nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, vars: &mut Vec<(i64, i64)>, origins: &mut Vec<(i64, i64, i64)>, expr: i64, sym: i64, item: i64) -> i64 {
    let args = fresh_args_for(nodes, lists, vars, origins, expr, item);
    canon_tyinfo(nodes, lists, TYD_STRUCT, sym, args, NONE, NONE)
}

/// A fresh list with one inference variable per declared type parameter.
fn fresh_args_for(nodes: &mut [i64], lists: &mut Vec<Vec<i64>>, vars: &mut Vec<(i64, i64)>, origins: &mut Vec<(i64, i64, i64)>, expr: i64, item: i64) -> i64 {
    let count = declared_param_count(nodes, lists, item);
    if count == 0 {
        return NONE;
    }
    let args = alloc_list(lists);
    let params = if node_a(nodes, item) == ITEM_NATIVE_TYPE {
        node_e(nodes, item)
    } else {
        node_f(nodes, item)
    };
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(lists, params, idx);
        if node_tag(nodes, param) == NODE_TY && node_a(nodes, param) == TY_PARAM {
            let name = node_b(nodes, param);
            let var = fresh_var(vars, origins, expr, name);
            list_push(lists, args, var);
        }
        idx += 1;
    }
    args
}

fn list_to_vec(lists: &[Vec<i64>], id: i64) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    let count = list_len(lists, id);
    let mut idx = 0i64;
    while idx < count {
        out.push(list_get(lists, id, idx));
        idx += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Calls.
// ---------------------------------------------------------------------------

fn check_call(state: &mut State, expr: i64, expected: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let callee = node_b(state.1, expr);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    let result;
    if node_tag(state.1, callee) == NODE_EXPR && node_a(state.1, callee) == EXPR_PATH {
        let sym = expr_sym_of(state.1, callee);
        if sym != NONE {
            let kind = sym_kind(state.1, sym);
            if kind == SYM_FUN || kind == SYM_NATIVE_FUN {
                result = check_direct_call(state, expr, sym, ret, impure, self_key);
            } else if kind == SYM_TRAIT_METHOD {
                result = check_trait_call(state, expr, sym, ret, impure, self_key);
            } else if kind == SYM_IMPL_METHOD {
                push_error(state.3, "impl methods cannot be called directly", file, start, end);
                result = unknown_key(state.1, state.2);
            } else {
                push_error(state.3, "cannot call this symbol", file, start, end);
                result = unknown_key(state.1, state.2);
            }
        } else {
            result = check_unresolved_callee(state, expr);
        }
    } else {
        push_error(state.3, "cannot call this expression", file, start, end);
        result = unknown_key(state.1, state.2);
    }
    if expected != NONE {
        let ok = unify_key(state.1, state.2, state.6, result, expected);
        if !ok {
            push_error(state.3, &format!("call result type mismatch: expected '{}', found '{}'", render_key(state.0, state.1, state.2, expected), render_key(state.0, state.1, state.2, result)), file, start, end);
        }
    }
    expr_set_ty(state.1, expr, result);
    result
}

/// Reports why a callee path resolved to no symbol.
fn check_unresolved_callee(state: &mut State, expr: i64) -> i64 {
    let callee = node_b(state.1, expr);
    let segs = node_b(state.1, callee);
    let count = list_len(state.2, segs);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    if count > 1 {
        let first = list_get(state.2, segs, 0);
        let found = lookup(state.4, first);
        if found.0 == NONE {
            push_error(state.3, &format!("unknown symbol '{}'", name_text(state.0, first)), file, start, end);
        } else {
            push_error(state.3, "cannot call a field", file, start, end);
        }
    } else {
        let first = list_get(state.2, segs, 0);
        push_error(state.3, &format!("unknown function '{}'", name_text(state.0, first)), file, start, end);
    }
    unknown_key(state.1, state.2)
}

/// Checks a call to a declared function (or native function), creating
/// the monomorphized instance row.
fn check_direct_call(state: &mut State, expr: i64, sym: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let decl = sym_decl(state.1, sym);
    if decl == NONE {
        return check_from_u8(state, expr, sym, ret, impure, self_key);
    }
    let fn_node = fn_node_of(state.1, decl);
    let kind = sym_kind(state.1, sym);
    let from = fn_declared_param_keys(state.1, state.2, fn_node);
    let (args_list, to) = call_type_args(state, expr, fn_node, &from);
    let params = node_c(state.1, fn_node);
    let arg_exprs = node_d(state.1, expr);
    let pcount = list_len(state.2, params);
    let acount = list_len(state.2, arg_exprs);
    if pcount != acount {
        push_error(state.3, &format!("function '{}' expects {} arguments, found {}", name_text(state.0, node_a(state.1, fn_node)), pcount, acount), node_file(state.1, expr), node_start(state.1, expr), node_end(state.1, expr));
    }
    let param_keys = alloc_list(state.2);
    let mut idx = 0i64;
    while idx < pcount {
        let param = list_get(state.2, params, idx);
        let declared = ty_key_of(state.1, node_b(state.1, param));
        let concrete = subst_key(state.1, state.2, declared, &from, &to);
        list_push(state.2, param_keys, concrete);
        let arg = list_get(state.2, arg_exprs, idx);
        let akey = check_expr(state, arg, concrete, ret, impure, self_key);
        let ok = unify_key(state.1, state.2, state.6, akey, concrete);
        if !ok {
            push_error(state.3, &format!("argument {} of '{}' has type '{}', expected '{}'", idx + 1, name_text(state.0, node_a(state.1, fn_node)), render_key(state.0, state.1, state.2, akey), render_key(state.0, state.1, state.2, concrete)), node_file(state.1, arg), node_start(state.1, arg), node_end(state.1, arg));
        }
        idx += 1;
    }
    let declared_ret = ty_key_of(state.1, node_d(state.1, fn_node));
    let result = subst_key(state.1, state.2, declared_ret, &from, &to);
    let mono = canon_tyinfo(state.1, state.2, TYD_MONO, fn_node, args_list, NONE, NONE);
    // Native functions have no body to emit: the instance carries the
    // symbol so codegen routes the call to the runtime surface, exactly
    // as the builtin `from_u8` methods already do.
    let slot = if kind == SYM_NATIVE_FUN { sym } else { fn_node };
    let inst = instance_of(state.1, mono, slot, args_list, result, param_keys, kind);
    expr_set_sym(state.1, expr, inst);
    result
}

/// Finds or creates the instance row for a monomorphization and returns
/// its id.  The row is created once per mono key; every call site with
/// the same mono reads the same row, so the attached result and param
/// keys are the first creation's (deterministic for a given mono).
fn instance_of(nodes: &mut Vec<i64>, mono: i64, fn_slot: i64, args_list: i64, result: i64, param_keys: i64, kind: i64) -> i64 {
    let existing = find_instance(nodes, mono);
    if existing != NONE {
        return existing;
    }
    alloc_node(nodes, &[NODE_INST, NO_FILE, NO_FILE, NO_FILE, fn_slot, args_list, result, param_keys, mono, kind])
}

/// The fn node behind a symbol declaration (a fn node directly, or the
/// fn slot of an item).
fn fn_node_of(nodes: &[i64], decl: i64) -> i64 {
    if node_tag(nodes, decl) == NODE_FN {
        decl
    } else {
        node_d(nodes, decl)
    }
}

/// The declared-parameter keys of a function node, in declared order.
fn fn_declared_param_keys(nodes: &mut Vec<i64>, lists: &mut [Vec<i64>], fn_node: i64) -> Vec<i64> {
    let tparams = node_b(nodes, fn_node);
    let count = list_len(lists, tparams);
    let mut keys: Vec<i64> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(lists, tparams, idx);
        if node_tag(nodes, param) == NODE_TY && node_a(nodes, param) == TY_PARAM {
            let name = node_b(nodes, param);
            let bound = node_c(nodes, param);
            keys.push(param_decl_key(nodes, lists, fn_node, name, bound));
        }
        idx += 1;
    }
    keys
}

/// The concrete type arguments of a call: explicit `f[T]()` arguments,
/// or one fresh inference variable per declared parameter.  Returns the
/// argument list id and its contents as a vector.
fn call_type_args(state: &mut State, expr: i64, fn_node: i64, from: &[i64]) -> (i64, Vec<i64>) {
    let tcount = from.len() as i64;
    if tcount == 0 {
        return (NONE, Vec::new());
    }
    let targs_expr = node_c(state.1, expr);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    let args = alloc_list(state.2);
    if targs_expr != NONE {
        let explicit = canon_ty_list(state, targs_expr, NONE, 1);
        let ec = list_len(state.2, explicit);
        if ec != tcount {
            push_error(state.3, &format!("type arguments: expected {}, found {}", tcount, ec), file, start, end);
        }
        let mut idx = 0i64;
        while idx < ec && idx < tcount {
            let item = list_get(state.2, explicit, idx);
            list_push(state.2, args, item);
            idx += 1;
        }
    } else {
        let tparams = node_b(state.1, fn_node);
        let mut idx = 0i64;
        while idx < tcount {
            let param = list_get(state.2, tparams, idx);
            let name = if node_tag(state.1, param) == NODE_TY {
                node_b(state.1, param)
            } else {
                NONE
            };
            let var = fresh_var(state.6, state.7, expr, name);
            list_push(state.2, args, var);
            idx += 1;
        }
    }
    let to = list_to_vec(state.2, args);
    (args, to)
}

/// The builtin `from_u8` conversion: the receiver type is the builtin
/// type whose scope owns the method, the argument is U8, the result is
/// the receiver type.
fn check_from_u8(state: &mut State, expr: i64, sym: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let home = sym_home(state.1, sym);
    let receiver_sym = builtin_type_of_scope(state.1, home);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    if receiver_sym == NONE {
        push_error(state.3, "cannot resolve the receiver type of 'from_u8'", file, start, end);
        let key = unknown_key(state.1, state.2);
        expr_set_ty(state.1, expr, key);
        return key;
    }
    let receiver_key = builtin_key_of_sym(state.1, receiver_sym);
    let u8_key = builtin_key_of(state.1, BUILTIN_U8);
    let arg_exprs = node_d(state.1, expr);
    let acount = list_len(state.2, arg_exprs);
    if acount != 1 {
        push_error(state.3, "'from_u8' expects exactly one argument", file, start, end);
    }
    let arg = list_get(state.2, arg_exprs, 0);
    let akey = check_expr(state, arg, u8_key, ret, impure, self_key);
    let ok = unify_key(state.1, state.2, state.6, akey, u8_key);
    if !ok {
        push_error(state.3, &format!("'from_u8' argument must be U8, found '{}'", render_key(state.0, state.1, state.2, akey)), node_file(state.1, arg), node_start(state.1, arg), node_end(state.1, arg));
    }
    let args_list = alloc_list(state.2);
    list_push(state.2, args_list, receiver_key);
    let mono = canon_tyinfo(state.1, state.2, TYD_MONO, sym, args_list, NONE, NONE);
    let param_keys = alloc_list(state.2);
    list_push(state.2, param_keys, u8_key);
    let inst = instance_of(state.1, mono, sym, args_list, receiver_key, param_keys, SYM_NATIVE_FUN);
    expr_set_sym(state.1, expr, inst);
    receiver_key
}

/// The builtin type symbol whose scope is `scope` (the receiver of a
/// builtin method such as `from_u8`).
fn builtin_type_of_scope(nodes: &[i64], scope: i64) -> i64 {
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        if node_tag(nodes, idx) == NODE_SYM && node_a(nodes, idx) == SYM_TYPE && node_e(nodes, idx) == scope {
            return idx;
        }
        idx += 1;
    }
    NONE
}

/// A trait method call (`Checksum.checksum(value)`).  When the receiver
/// type is concrete the impl method is resolved here; when it is a type
/// parameter the dispatch is deferred for codegen, which reads the impl
/// table with the substituted receiver type.
fn check_trait_call(state: &mut State, expr: i64, sym: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let trait_sym = trait_sym_of_method(state.1, sym);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    if trait_sym == NONE {
        push_error(state.3, "cannot find the trait of this method", file, start, end);
        let key = unknown_key(state.1, state.2);
        expr_set_ty(state.1, expr, key);
        return key;
    }
    let trait_item = sym_decl(state.1, trait_sym);
    let method_name = node_a(state.1, sym_decl(state.1, sym));
    let trait_method = find_method_by_name(state.1, state.2, node_e(state.1, trait_item), method_name);
    if trait_method == NONE {
        push_error(state.3, "trait method not found", file, start, end);
        let key = unknown_key(state.1, state.2);
        expr_set_ty(state.1, expr, key);
        return key;
    }
    let arg_exprs = node_d(state.1, expr);
    let acount = list_len(state.2, arg_exprs);
    if acount == 0 {
        push_error(state.3, "trait method call requires a receiver argument", file, start, end);
    }
    let receiver = list_get(state.2, arg_exprs, 0);
    let rkey = check_expr(state, receiver, NONE, ret, impure, self_key);
    let recv = deref_key(state.1, rkey);
    let result = if key_kind(state.1, recv) == TYD_PARAM {
        trait_call_deferred(state, expr, trait_sym, trait_method, recv, arg_exprs, (ret, impure, self_key))
    } else {
        trait_call_concrete(state, expr, trait_sym, trait_method, recv, arg_exprs, (ret, impure, self_key))
    };
    expr_set_ty(state.1, expr, result);
    result
}

/// Resolves a trait call whose receiver type is concrete: finds the impl,
/// creates the impl-method instance, and records the dispatch row.
fn trait_call_concrete(state: &mut State, expr: i64, trait_sym: i64, trait_method: i64, recv: i64, arg_exprs: i64, fctx: (i64, i64, i64)) -> i64 {
    let (ret, impure, self_key) = fctx;
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    let impl_idx = impl_find(state.5, trait_sym, recv);
    if impl_idx == NONE {
        push_error(state.3, &format!("type '{}' does not implement trait '{}'", render_key(state.0, state.1, state.2, recv), name_text(state.0, sym_name(state.1, trait_sym))), file, start, end);
        return unknown_key(state.1, state.2);
    }
    let methods = impl_methods(state.5, impl_idx);
    let method_name = node_a(state.1, trait_method);
    let method = find_method_by_name(state.1, state.2, methods, method_name);
    if method == NONE {
        push_error(state.3, "impl method not found", file, start, end);
        return unknown_key(state.1, state.2);
    }
    let fn_node = method;
    let params = node_c(state.1, fn_node);
    let pcount = list_len(state.2, params);
    let param_keys = alloc_list(state.2);
    let mut idx = 0i64;
    while idx < pcount {
        let param = list_get(state.2, params, idx);
        let key = ty_key_of(state.1, node_b(state.1, param));
        list_push(state.2, param_keys, key);
        let arg = list_get(state.2, arg_exprs, idx);
        let akey = check_expr(state, arg, key, ret, impure, self_key);
        let ok = unify_key(state.1, state.2, state.6, akey, key);
        if !ok {
            push_error(state.3, &format!("argument {} has type '{}', expected '{}'", idx + 1, render_key(state.0, state.1, state.2, akey), render_key(state.0, state.1, state.2, key)), node_file(state.1, arg), node_start(state.1, arg), node_end(state.1, arg));
        }
        idx += 1;
    }
    let result = ty_key_of(state.1, node_d(state.1, fn_node));
    let mono = canon_tyinfo(state.1, state.2, TYD_MONO, fn_node, NONE, NONE, NONE);
    let inst = instance_of(state.1, mono, fn_node, NONE, result, param_keys, SYM_IMPL_METHOD);
    alloc_trait_call(state.1, expr, inst, trait_sym, method_name);
    expr_set_sym(state.1, expr, inst);
    result
}

/// Verifies a trait call whose receiver is a type parameter (the bound
/// must name the trait) and records a deferred dispatch row.
fn trait_call_deferred(state: &mut State, expr: i64, trait_sym: i64, trait_method: i64, recv: i64, arg_exprs: i64, fctx: (i64, i64, i64)) -> i64 {
    let (ret, impure, self_key) = fctx;
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    if !param_has_bound(state.1, recv, trait_sym) {
        push_error(state.3, &format!("type parameter '{}' does not implement trait '{}'", name_text(state.0, key_sym(state.1, recv)), name_text(state.0, sym_name(state.1, trait_sym))), file, start, end);
    }
    push_scope(state.4);
    let tparams = node_b(state.1, trait_method);
    bind_type_params(state.1, state.2, state.4, trait_method, tparams);
    let params = node_c(state.1, trait_method);
    let pcount = list_len(state.2, params);
    let mut idx = 0i64;
    while idx < pcount {
        let param = list_get(state.2, params, idx);
        let param_ty = node_b(state.1, param);
        let key = canon_ty(state, param_ty, recv, 0);
        let arg = list_get(state.2, arg_exprs, idx);
        let akey = check_expr(state, arg, key, ret, impure, self_key);
        let ok = unify_key(state.1, state.2, state.6, akey, key);
        if !ok {
            push_error(state.3, &format!("argument {} has type '{}', expected '{}'", idx + 1, render_key(state.0, state.1, state.2, akey), render_key(state.0, state.1, state.2, key)), node_file(state.1, arg), node_start(state.1, arg), node_end(state.1, arg));
        }
        idx += 1;
    }
    let ret_ty = node_d(state.1, trait_method);
    let result = canon_ty(state, ret_ty, recv, 0);
    pop_scope(state.4);
    let method_name = node_a(state.1, trait_method);
    alloc_trait_call(state.1, expr, NONE, trait_sym, method_name);
    result
}

/// True when a type parameter's bound names `trait_sym`.
fn param_has_bound(nodes: &[i64], key: i64, trait_sym: i64) -> bool {
    if key_kind(nodes, key) != TYD_PARAM {
        return false;
    }
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        return false;
    }
    node_f(nodes, row) == trait_sym
}

/// The dereferenced key of a reference key, unchanged otherwise.
fn deref_key(nodes: &[i64], key: i64) -> i64 {
    let kind = key_kind(nodes, key);
    if kind == TYD_REF || kind == TYD_REF_MUT {
        key_elem(nodes, key)
    } else {
        key
    }
}

// ---------------------------------------------------------------------------
// Struct literals and variant construction.
// ---------------------------------------------------------------------------

/// Checks `Name(field: value, ...)` struct literals and `Variant(args)`
/// constructions (the resolver rewrites variant calls into this shape).
fn check_struct_lit(state: &mut State, expr: i64, expected: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let sym = expr_sym_of(state.1, expr);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    let result;
    if sym == NONE {
        push_error(state.3, "cannot resolve the type of this literal", file, start, end);
        result = unknown_key(state.1, state.2);
    } else {
        let kind = sym_kind(state.1, sym);
        if kind == SYM_STRUCT {
            result = check_struct_construct(state, expr, sym, ret, impure, self_key);
        } else if kind == SYM_VARIANT {
            result = check_variant_construct(state, expr, sym, ret, impure, self_key);
        } else {
            push_error(state.3, "cannot construct a value of this symbol", file, start, end);
            result = unknown_key(state.1, state.2);
        }
    }
    if expected != NONE {
        let ok = unify_key(state.1, state.2, state.6, result, expected);
        if !ok {
            push_error(state.3, &format!("constructed value type mismatch: expected '{}', found '{}'", render_key(state.0, state.1, state.2, expected), render_key(state.0, state.1, state.2, result)), file, start, end);
        }
    }
    expr_set_ty(state.1, expr, result);
    result
}

/// Checks a named-field struct literal against the struct declaration.
fn check_struct_construct(state: &mut State, expr: i64, sym: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    let item = sym_decl(state.1, sym);
    let key = struct_key_with_fresh(state.1, state.2, state.6, state.7, expr, sym, item);
    let args = key_args(state.1, key);
    let from = declared_param_keys(state.1, state.2, item);
    let to = list_to_vec(state.2, args);
    let field_names = node_c(state.1, expr);
    let values = node_d(state.1, expr);
    let fcount = list_len(state.2, field_names);
    let vcount = list_len(state.2, values);
    if fcount != vcount {
        push_error(state.3, "struct literal field/value count mismatch", file, start, end);
    }
    let mut idx = 0i64;
    while idx < fcount {
        let name = list_get(state.2, field_names, idx);
        let (found_idx, declared) = struct_field_of(state.1, state.2, item, name);
        if found_idx == NONE {
            push_error(state.3, &format!("no field '{}' on struct '{}'", name_text(state.0, name), name_text(state.0, sym_name(state.1, sym))), file, start, end);
        } else {
            let concrete = subst_key(state.1, state.2, declared, &from, &to);
            let value = list_get(state.2, values, idx);
            let vkey = check_expr(state, value, concrete, ret, impure, self_key);
            let ok = unify_key(state.1, state.2, state.6, vkey, concrete);
            if !ok {
                push_error(state.3, &format!("field '{}' has type '{}', expected '{}'", name_text(state.0, name), render_key(state.0, state.1, state.2, vkey), render_key(state.0, state.1, state.2, concrete)), node_file(state.1, value), node_start(state.1, value), node_end(state.1, value));
            }
        }
        idx += 1;
    }
    key
}

/// Checks a variant construction against the enum declaration.  The enum
/// type parameters are fresh inference variables unified with the payload
/// values and the expected type.
fn check_variant_construct(state: &mut State, expr: i64, sym: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    let decl = sym_decl(state.1, sym);
    if decl == NONE {
        let key = unit_key_of(state);
        return key;
    }
    let enum_sym = enum_sym_of_variant(state.1, sym);
    if enum_sym == NONE {
        push_error(state.3, "cannot find the enum of this variant", file, start, end);
        return unknown_key(state.1, state.2);
    }
    let enum_item = sym_decl(state.1, enum_sym);
    let key = enum_key_with_fresh(state.1, state.2, state.6, state.7, expr, enum_sym, enum_item);
    let args = key_args(state.1, key);
    let from = declared_param_keys(state.1, state.2, enum_item);
    let to = list_to_vec(state.2, args);
    let payload_decl = node_b(state.1, decl);
    let pcount = list_len(state.2, payload_decl);
    let values = node_d(state.1, expr);
    let vcount = list_len(state.2, values);
    if pcount != vcount {
        push_error(state.3, &format!("variant '{}' expects {} payload values, found {}", name_text(state.0, node_a(state.1, decl)), pcount, vcount), file, start, end);
    }
    let mut idx = 0i64;
    while idx < pcount {
        let declared = ty_key_of(state.1, list_get(state.2, payload_decl, idx));
        let concrete = subst_key(state.1, state.2, declared, &from, &to);
        let value = list_get(state.2, values, idx);
        let vkey = check_expr(state, value, concrete, ret, impure, self_key);
        let ok = unify_key(state.1, state.2, state.6, vkey, concrete);
        if !ok {
            push_error(state.3, &format!("payload {} of '{}' has type '{}', expected '{}'", idx + 1, name_text(state.0, node_a(state.1, decl)), render_key(state.0, state.1, state.2, vkey), render_key(state.0, state.1, state.2, concrete)), node_file(state.1, value), node_start(state.1, value), node_end(state.1, value));
        }
        idx += 1;
    }
    key
}

// ---------------------------------------------------------------------------
// Match expressions.
// ---------------------------------------------------------------------------

fn check_match(state: &mut State, expr: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    let scrutinee = node_b(state.1, expr);
    let arms = node_c(state.1, expr);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    let s_key = check_expr(state, scrutinee, NONE, ret, impure, self_key);
    let count = list_len(state.2, arms);
    let mut merged = NONE;
    let mut first = true;
    let mut idx = 0i64;
    while idx < count {
        let arm = list_get(state.2, arms, idx);
        let arm_key = check_arm(state, arm, s_key, ret, impure, self_key);
        let div = stmt_diverges(state.1, state.2, node_b(state.1, arm));
        if div == 0 {
            if first {
                merged = arm_key;
                first = false;
            } else {
                let ok = unify_key(state.1, state.2, state.6, merged, arm_key);
                if !ok {
                    push_error(state.3, &format!("match arms have different types: '{}' and '{}'", render_key(state.0, state.1, state.2, merged), render_key(state.0, state.1, state.2, arm_key)), node_file(state.1, arm), node_start(state.1, arm), node_end(state.1, arm));
                }
            }
        }
        idx += 1;
    }
    if merged == NONE {
        merged = unit_key_of(state);
    }
    check_exhaustive(state, s_key, arms, file, start, end);
    expr_set_ty(state.1, expr, merged);
    merged
}

/// Checks one match arm: binds its pattern variables and checks the body,
/// returning the body's value key.  A body that is a bare expression
/// statement is checked in expression position — its value is the arm's
/// value, so the unhandled-Result/Option check (which guards discarded
/// statement values) never applies to it.  Compound bodies (let, if,
/// return) are checked as statements.
fn check_arm(state: &mut State, arm: i64, s_key: i64, ret: i64, impure: i64, self_key: i64) -> i64 {
    push_scope(state.4);
    check_pattern(state, node_a(state.1, arm), s_key);
    let body = node_b(state.1, arm);
    let key = if node_tag(state.1, body) == NODE_STMT && node_a(state.1, body) == STMT_EXPR {
        check_expr(state, node_b(state.1, body), NONE, ret, impure, self_key)
    } else {
        check_stmt(state, body, ret, impure, self_key, 0)
    };
    pop_scope(state.4);
    key
}

/// Checks a pattern against the scrutinee key, binding any names.  Facts
/// (payload keys, element keys) are read from the keys the earlier stages
/// attached to the declarations.
fn check_pattern(state: &mut State, pat: i64, s_key: i64) -> i64 {
    if node_tag(state.1, pat) != NODE_PAT {
        return unknown_key(state.1, state.2);
    }
    let kind = node_a(state.1, pat);
    let file = node_file(state.1, pat);
    let start = node_start(state.1, pat);
    let end = node_end(state.1, pat);
    if kind == PAT_BIND {
        let name = node_b(state.1, pat);
        bind(state.4, name, s_key, 0);
        pat_set_ty(state.1, pat, s_key);
        return s_key;
    }
    if kind == PAT_LIT {
        let lit = node_b(state.1, pat);
        let key = if lit == LIT_TRUE || lit == LIT_FALSE {
            builtin_key_of(state.1, BUILTIN_BOOL)
        } else {
            builtin_key_of(state.1, BUILTIN_INT)
        };
        let ok = unify_key(state.1, state.2, state.6, key, s_key);
        if !ok {
            push_error(state.3, &format!("literal pattern type mismatch: expected '{}'", render_key(state.0, state.1, state.2, s_key)), file, start, end);
        }
        pat_set_ty(state.1, pat, key);
        return key;
    }
    if kind == PAT_PATH || kind == PAT_VARIANT {
        let sym = pat_sym_of(state.1, pat);
        if sym == NONE {
            push_error(state.3, "cannot resolve pattern", file, start, end);
            let key = unknown_key(state.1, state.2);
            pat_set_ty(state.1, pat, key);
            return key;
        }
        if !variant_matches(state.1, s_key, sym) {
            push_error(state.3, "this pattern does not match the scrutinee type", file, start, end);
        }
        if kind == PAT_VARIANT {
            let decl = sym_decl(state.1, sym);
            let payload_decl = node_b(state.1, decl);
            let pcount = list_len(state.2, payload_decl);
            let args = key_args(state.1, s_key);
            let enum_sym = enum_sym_of_variant(state.1, sym);
            let enum_item = sym_decl(state.1, enum_sym);
            let from = declared_param_keys(state.1, state.2, enum_item);
            let to = list_to_vec(state.2, args);
            let payload_pats = node_c(state.1, pat);
            let pc = list_len(state.2, payload_pats);
            if pcount != pc {
                push_error(state.3, &format!("variant pattern '{}' expects {} payload patterns, found {}", name_text(state.0, node_a(state.1, decl)), pcount, pc), file, start, end);
            }
            let mut idx = 0i64;
            while idx < pcount {
                let declared = ty_key_of(state.1, list_get(state.2, payload_decl, idx));
                let concrete = subst_key(state.1, state.2, declared, &from, &to);
                check_pattern(state, list_get(state.2, payload_pats, idx), concrete);
                idx += 1;
            }
        }
        pat_set_ty(state.1, pat, s_key);
        return s_key;
    }
    let inner = deref_key(state.1, s_key);
    let inner_kind = key_kind(state.1, inner);
    if inner_kind != TYD_SLICE && inner_kind != TYD_ARRAY {
        push_error(state.3, "array pattern requires a slice or array scrutinee", file, start, end);
        let key = unknown_key(state.1, state.2);
        pat_set_ty(state.1, pat, key);
        return key;
    }
    let elem = key_elem(state.1, inner);
    let elems = node_b(state.1, pat);
    let ecount = list_len(state.2, elems);
    let mut idx = 0i64;
    while idx < ecount {
        check_pattern(state, list_get(state.2, elems, idx), elem);
        idx += 1;
    }
    let rest = node_c(state.1, pat);
    if rest != NONE {
        let rest_key = rest_type_of(state.1, state.2, s_key, inner);
        bind(state.4, rest, rest_key, 0);
        pat_set_rest_key(state.1, pat, rest_key);
    }
    pat_set_ty(state.1, pat, s_key);
    s_key
}

/// The key of the rest binder: always a reference to the remaining
/// elements.  The emitter materializes the rest as a `{data, len}` slice
/// view pointing into the scrutinee's storage — for array and slice
/// scrutinees alike — so the binder is `&[T]` (or `&mut [T]` for a
/// `&mut` scrutinee), never a bare value or array.  A rest over a value
/// array borrows that array for the arm's duration, which the borrow
/// checker models as a shared loan on the scrutinee binding.
fn rest_type_of(nodes: &mut Vec<i64>, lists: &mut [Vec<i64>], s_key: i64, inner: i64) -> i64 {
    let is_mut = key_kind(nodes, s_key) == TYD_REF_MUT;
    let rest = canon_tyinfo(nodes, lists, TYD_SLICE, NONE, NONE, key_elem(nodes, inner), NONE);
    let kind_of = if is_mut { TYD_REF_MUT } else { TYD_REF };
    canon_tyinfo(nodes, lists, kind_of, NONE, NONE, rest, NONE)
}

/// True when `variant_sym` is a variant of the enum `s_key` denotes.
fn variant_matches(nodes: &[i64], s_key: i64, variant_sym: i64) -> bool {
    let s_kind = key_kind(nodes, s_key);
    if s_kind == TYD_BUILTIN {
        return sym_decl(nodes, variant_sym) == NONE;
    }
    if s_kind != TYD_ENUM {
        return false;
    }
    let enum_sym = key_sym(nodes, s_key);
    node_e(nodes, enum_sym) == sym_home(nodes, variant_sym)
}

/// Reports non-exhaustive matches.  A binding arm covers everything; an
/// enum match must name every variant; a fixed array must be covered at
/// its length; a slice must have a rest arm covering every length from
/// the smallest rest arm downwards.
fn check_exhaustive(state: &mut State, s_key: i64, arms: i64, file: i64, start: i64, end: i64) {
    let count = list_len(state.2, arms);
    let mut has_bind = false;
    let mut idx = 0i64;
    while idx < count {
        let pat = node_a(state.1, list_get(state.2, arms, idx));
        if node_tag(state.1, pat) == NODE_PAT && node_a(state.1, pat) == PAT_BIND {
            has_bind = true;
        }
        idx += 1;
    }
    if has_bind {
        return;
    }
    let inner = deref_key(state.1, s_key);
    let kind = key_kind(state.1, inner);
    if kind == TYD_ENUM {
        let enum_sym = key_sym(state.1, inner);
        let item = sym_decl(state.1, enum_sym);
        let variants = node_e(state.1, item);
        let vcount = list_len(state.2, variants);
        let mut v_idx = 0i64;
        while v_idx < vcount {
            let variant = list_get(state.2, variants, v_idx);
            if !variant_covered(state.1, state.2, arms, variant) {
                push_error(state.3, &format!("non-exhaustive match: missing variant '{}'", name_text(state.0, node_a(state.1, variant))), file, start, end);
            }
            v_idx += 1;
        }
    } else if kind == TYD_ARRAY {
        let n = key_len(state.1, inner);
        if !array_covers_len(state.1, state.2, arms, n) {
            push_error(state.3, "non-exhaustive match: no arm covers this array length", file, start, end);
        }
    } else if kind == TYD_SLICE {
        if !slice_exhaustive(state.1, state.2, arms) {
            push_error(state.3, "non-exhaustive match: a rest pattern must cover the remaining elements", file, start, end);
        }
    } else if kind != TYD_UNKNOWN {
        push_error(state.3, &format!("non-exhaustive match on '{}': add a binding arm", render_key(state.0, state.1, state.2, inner)), file, start, end);
    }
}

/// True when some arm pattern names `variant` (the NODE_VARIANT row).
fn variant_covered(nodes: &[i64], lists: &[Vec<i64>], arms: i64, variant: i64) -> bool {
    let count = list_len(lists, arms);
    let mut idx = 0i64;
    while idx < count {
        let pat = node_a(nodes, list_get(lists, arms, idx));
        if node_tag(nodes, pat) == NODE_PAT {
            let pkind = node_a(nodes, pat);
            if pkind == PAT_PATH || pkind == PAT_VARIANT {
                let sym = pat_sym_of(nodes, pat);
                if sym != NONE && sym_decl(nodes, sym) == variant {
                    return true;
                }
            }
        }
        idx += 1;
    }
    false
}

/// True when an exact-length or rest arm covers a fixed array of length
/// `n` (the only length a `[T; n]` value can have).
fn array_covers_len(nodes: &[i64], lists: &[Vec<i64>], arms: i64, n: i64) -> bool {
    let count = list_len(lists, arms);
    let mut idx = 0i64;
    while idx < count {
        let pat = node_a(nodes, list_get(lists, arms, idx));
        if node_tag(nodes, pat) == NODE_PAT && node_a(nodes, pat) == PAT_ARRAY {
            let fixed = list_len(lists, node_b(nodes, pat));
            let rest = node_c(nodes, pat);
            if rest != NONE {
                if fixed <= n {
                    return true;
                }
            } else if fixed == n {
                return true;
            }
        }
        idx += 1;
    }
    false
}

/// True when a slice match covers every length: a rest arm bounds the
/// lengths above its fixed count, and every shorter length is exactly
/// covered.
fn slice_exhaustive(nodes: &[i64], lists: &[Vec<i64>], arms: i64) -> bool {
    let count = list_len(lists, arms);
    let mut min_rest: i64 = NONE;
    let mut exacts: Vec<i64> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let pat = node_a(nodes, list_get(lists, arms, idx));
        if node_tag(nodes, pat) == NODE_PAT && node_a(nodes, pat) == PAT_ARRAY {
            let fixed = list_len(lists, node_b(nodes, pat));
            let rest = node_c(nodes, pat);
            if rest != NONE {
                if min_rest == NONE || fixed < min_rest {
                    min_rest = fixed;
                }
            } else {
                exacts.push(fixed);
            }
        }
        idx += 1;
    }
    if min_rest == NONE {
        return false;
    }
    let mut len = 0i64;
    while len < min_rest {
        if !exacts.contains(&len) {
            return false;
        }
        len += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// Final variable resolution.
// ---------------------------------------------------------------------------

/// Substitutes every inference variable in every attached key, reports
/// variables that never unified, and re-canonicalizes instance rows so
/// codegen only ever sees concrete keys.
fn resolve_all_vars(names: &mut [String], nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, vars: &[(i64, i64)], origins: &[(i64, i64, i64)]) {
    report_unbound(names, nodes, errors, vars, origins);
    let mut idx = 0i64;
    while idx < nodes.len() as i64 / NODE_STRIDE {
        let tag = node_tag(nodes, idx);
        if tag == NODE_EXPR {
            let resolved = resolve_key(nodes, lists, vars, expr_ty_of(nodes, idx));
            expr_set_ty(nodes, idx, resolved);
        } else if tag == NODE_STMT {
            let resolved = resolve_key(nodes, lists, vars, stmt_ty_of(nodes, idx));
            stmt_set_ty(nodes, idx, resolved);
        } else if tag == NODE_PAT {
            let resolved = resolve_key(nodes, lists, vars, pat_ty_of(nodes, idx));
            pat_set_ty(nodes, idx, resolved);
        } else if tag == NODE_TY {
            let resolved = resolve_key(nodes, lists, vars, ty_key_of(nodes, idx));
            ty_set_key(nodes, idx, resolved);
        } else if tag == NODE_INST {
            resolve_instance_row(nodes, lists, vars, idx);
        }
        idx += 1;
    }
}

/// Fully resolves one key: variables are followed to their bindings and
/// descriptor children are resolved and re-canonicalized when changed.
fn resolve_key(nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, vars: &[(i64, i64)], key: i64) -> i64 {
    if key == NONE {
        return NONE;
    }
    if is_var(key) {
        let r = resolve_var(vars, key);
        if is_var(r) {
            return r;
        }
        return resolve_key(nodes, lists, vars, r);
    }
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        return key;
    }
    let kind = node_b(nodes, row);
    let sym = node_c(nodes, row);
    let args = node_d(nodes, row);
    let elem = node_e(nodes, row);
    let len = node_f(nodes, row);
    let new_args = resolve_list_keys(nodes, lists, vars, args);
    let new_elem = resolve_key(nodes, lists, vars, elem);
    if new_args == args && new_elem == elem {
        return key;
    }
    canon_tyinfo(nodes, lists, kind, sym, new_args, new_elem, len)
}

fn resolve_list_keys(nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, vars: &[(i64, i64)], list: i64) -> i64 {
    let count = list_len(lists, list);
    if count == 0 {
        return list;
    }
    let mut changed = false;
    let fresh = alloc_list(lists);
    let mut idx = 0i64;
    while idx < count {
        let old = list_get(lists, list, idx);
        let new = resolve_key(nodes, lists, vars, old);
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

/// Re-canonicalizes one instance row after variable resolution.
fn resolve_instance_row(nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, vars: &[(i64, i64)], inst: i64) {
    let fn_node = inst_fn_of(nodes, inst);
    let args = inst_args_of(nodes, inst);
    let new_args = resolve_list_keys(nodes, lists, vars, args);
    let new_ret = resolve_key(nodes, lists, vars, inst_ret_of(nodes, inst));
    let new_params = resolve_list_keys(nodes, lists, vars, inst_params_of(nodes, inst));
    let mono = canon_tyinfo(nodes, lists, TYD_MONO, fn_node, new_args, NONE, NONE);
    inst_set_args(nodes, inst, new_args);
    inst_set_ret(nodes, inst, new_ret);
    inst_set_params(nodes, inst, new_params);
    inst_set_mono(nodes, inst, mono);
}

/// Reports every inference variable that never unified, at the origin
/// expression that created it.
fn report_unbound(names: &[String], nodes: &mut [i64], errors: &mut Vec<Diag>, vars: &[(i64, i64)], origins: &[(i64, i64, i64)]) {
    let mut idx = 0usize;
    while idx < vars.len() {
        match vars.get(idx) {
            Some(pair) => {
                let r = resolve_var(vars, pair.0);
                if is_var(r) {
                    let origin = origin_of(origins, pair.0);
                    let what = if origin.2 == NONE {
                        String::from("a type parameter")
                    } else {
                        format!("type parameter '{}'", name_text(names, origin.2))
                    };
                    if origin.1 == NONE {
                        push_internal(errors, &format!("cannot infer {}", what));
                    } else {
                        push_error(errors, &format!("cannot infer {}", what), node_file(nodes, origin.1), node_start(nodes, origin.1), node_end(nodes, origin.1));
                    }
                }
            }
            None => break,
        }
        idx += 1;
    }
}

fn origin_of(origins: &[(i64, i64, i64)], var: i64) -> (i64, i64, i64) {
    let mut idx = 0usize;
    loop {
        match origins.get(idx) {
            Some(origin) => {
                if origin.0 == var {
                    return *origin;
                }
            }
            None => return (NONE, NONE, NONE),
        }
        idx += 1;
    }
}
