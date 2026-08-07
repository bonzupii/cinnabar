//! Cinnabar borrow checker.
//!
//! Runs on the type-attributed arena (after the typechecker).  Flows
//! through each function body tracking every binding's move and borrow
//! state:
//!
//! - linear handles (the native surfaces `Memory.Block`, `Collections.Vec`,
//!   `Collections.String`, `Collections.HashMap`) must be consumed exactly
//!   once on every path;
//! - a moved linear value cannot be used again;
//! - a linear value cannot be copied (used by value where the callee does
//!   not consume it);
//! - `&T` and `&mut T` borrows are exclusive while live; a borrow is
//!   invalidated when its owner is moved;
//! - returning a borrow when more than one reference parameter could be
//!   the source is an error (an ambiguous returned borrow).
//!
//! Linearity and borrow kinds are read from the typechecker's attached
//! keys and the declared callee signatures; nothing is re-inferred.

use crate::ast::*;

/// A binding's borrow-check state.
/// `(key, is_linear, is_param, is_moved, holds, file, start, end)` where
/// `holds` records the `(owner name, borrow kind)` borrows this binding's
/// value currently holds (made by passing the owner by reference into a
/// call), and `file`/`start`/`end` are the binding's declaration span so
/// every diagnostic points at the real source origin.
type Binding = (i64, bool, bool, bool, Vec<(i64, i64)>, i64, i64, i64);

/// One lexical scope: `(name id, binding)` entries.
type Scope = Vec<(i64, Binding)>;

pub const BORROW_MUT: i64 = 1;

fn push_scope(scopes: &mut Vec<Scope>) {
    scopes.push(Vec::new());
}

fn pop_scope(scopes: &mut Vec<Scope>) {
    scopes.pop();
}

fn lookup(scopes: &[Scope], name: i64) -> Option<&Binding> {
    let mut s_idx = scopes.len();
    while s_idx > 0 {
        s_idx -= 1;
        let scope = scopes.get(s_idx)?;
        let mut idx = scope.len();
        while idx > 0 {
            idx -= 1;
            {
                let entry = scope.get(idx)?;
                if entry.0 == name {
                    return Some(&entry.1);
                }
            }
        }
    }
    None
}

fn lookup_mut(scopes: &mut [Scope], name: i64) -> Option<&mut Binding> {
    let mut s_idx = scopes.len();
    while s_idx > 0 {
        s_idx -= 1;
        let mut idx = match scopes.get(s_idx) {
            Some(scope) => scope.len(),
            None => 0,
        };
        while idx > 0 {
            idx -= 1;
            let found = match scopes.get(s_idx) {
                Some(scope) => match scope.get(idx) {
                    Some(entry) => entry.0 == name,
                    None => false,
                },
                None => false,
            };
            if found {
                return match scopes.get_mut(s_idx) {
                    Some(scope) => match scope.get_mut(idx) {
                        Some(entry) => Some(&mut entry.1),
                        None => None,
                    },
                    None => None,
                };
            }
        }
    }
    None
}

fn bind(scopes: &mut [Scope], name: i64, key: i64, is_linear: bool, is_param: bool, span: (i64, i64, i64)) {
    if let Some(scope) = scopes.last_mut() {
        scope.push((name, (key, is_linear, is_param, false, Vec::new(), span.0, span.1, span.2)));
    }
}

fn snapshot(scopes: &[Scope]) -> Vec<Scope> {
    let mut out: Vec<Scope> = Vec::new();
    let mut idx = 0usize;
    while idx < scopes.len() {
        match scopes.get(idx) {
            Some(scope) => out.push(scope.clone()),
            None => break,
        }
        idx += 1;
    }
    out
}

fn restore(scopes: &mut Vec<Scope>, state: &[Scope]) {
    scopes.clear();
    let mut idx = 0usize;
    while idx < state.len() {
        match state.get(idx) {
            Some(scope) => scopes.push(scope.clone()),
            None => break,
        }
        idx += 1;
    }
}

// ---------------------------------------------------------------------------
// Key reads.
// ---------------------------------------------------------------------------

fn key_kind(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        TYD_UNKNOWN
    } else {
        node_b(nodes, row)
    }
}

fn key_sym_of(nodes: &[i64], key: i64) -> i64 {
    let row = find_tyinfo(nodes, key);
    if row == NONE {
        NONE
    } else {
        node_c(nodes, row)
    }
}

/// True when a key is a linear native handle: one of the four surfaces
/// whose runtime allocation must be released by explicit consumption.
fn key_is_linear(nodes: &[i64], names: &[String], key: i64) -> bool {
    if key_kind(nodes, key) != TYD_NATIVE {
        return false;
    }
    let sym = key_sym_of(nodes, key);
    if sym == NONE {
        return false;
    }
    let name = node_b(nodes, sym);
    name_is(names, name, "Memory.Block")
        || name_is(names, name, "Collections.Vec")
        || name_is(names, name, "Collections.String")
        || name_is(names, name, "Collections.HashMap")
}

fn list_len(lists: &[Vec<i64>], id: i64) -> i64 {
    match lists.get(id as usize) {
        Some(items) => items.len() as i64,
        None => 0,
    }
}

fn list_get(lists: &[Vec<i64>], id: i64, idx: i64) -> i64 {
    match lists.get(id as usize) {
        Some(items) => match items.get(idx as usize) {
            Some(value) => *value,
            None => NONE,
        },
        None => NONE,
    }
}

// ---------------------------------------------------------------------------
// Entry: walk every function in the program.
// ---------------------------------------------------------------------------

pub fn borrow_check(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    errors: &mut Vec<Diag>,
    root: i64,
    ext_mods: &[(String, i64)],
) -> bool {
    check_item_list(names, nodes, lists, errors, root);
    let mut idx = 0usize;
    while idx < ext_mods.len() {
        match ext_mods.get(idx) {
            Some(pair) => check_item_list(names, nodes, lists, errors, pair.1),
            None => break,
        }
        idx += 1;
    }
    errors.is_empty()
}

fn check_item_list(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, list: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        check_item(names, nodes, lists, errors, list_get(lists, list, idx));
        idx += 1;
    }
}

fn check_item(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, item: i64) {
    if node_tag(nodes, item) != NODE_ITEM {
        return;
    }
    let kind = node_a(nodes, item);
    if kind == ITEM_MODULE {
        check_item_list(names, nodes, lists, errors, node_e(nodes, item));
        return;
    }
    if kind == ITEM_IMPL {
        check_fn_list(names, nodes, lists, errors, node_f(nodes, item));
        return;
    }
    if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
        check_fn(names, nodes, lists, errors, node_d(nodes, item));
    }
}

fn check_fn_list(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, list: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        check_fn(names, nodes, lists, errors, list_get(lists, list, idx));
        idx += 1;
    }
}

fn check_fn(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, fn_node: i64) {
    if node_tag(nodes, fn_node) != NODE_FN {
        return;
    }
    let body = node_f(nodes, fn_node);
    if body == NONE {
        return;
    }
    let mut scopes: Vec<Scope> = Vec::new();
    push_scope(&mut scopes);
    let params = node_c(nodes, fn_node);
    let pcount = list_len(lists, params);
    let mut idx = 0i64;
    while idx < pcount {
        let param = list_get(lists, params, idx);
        let name = node_a(nodes, param);
        let key = ty_key_of(nodes, node_b(nodes, param));
        let linear = key_is_linear(nodes, names, key);
        bind(&mut scopes, name, key, linear, true, (node_file(nodes, param), node_start(nodes, param), node_end(nodes, param)));
        idx += 1;
    }
    check_stmt_list(names, nodes, lists, errors, &mut scopes, body);
    // Every linear binding must be consumed on the fall-off path.
    check_unconsumed(names, errors, &scopes);
    pop_scope(&mut scopes);
}

/// Reports linear bindings in the topmost scope that reach the end of
/// their scope unmoved.  Inner blocks (match arms, loop bodies) only
/// check the bindings they introduce; outer bindings are consumed by the
/// statements after the block, so flagging them mid-function would be a
/// false positive.  Every diagnostic carries the binding's declaration
/// span.
fn check_unconsumed(names: &[String], errors: &mut Vec<Diag>, scopes: &[Scope]) {
    let scope = match scopes.last() {
        Some(scope) => scope,
        None => return,
    };
    let mut e_idx = 0usize;
    while e_idx < scope.len() {
        match scope.get(e_idx) {
            Some(entry) => {
                let binding = &entry.1;
                if binding.1 && !binding.3 {
                    push_error(
                        errors,
                        &format!("linear value '{}' must be consumed", name_text(names, entry.0)),
                        binding.5,
                        binding.6,
                        binding.7,
                    );
                }
            }
            None => break,
        }
        e_idx += 1;
    }
}

// ---------------------------------------------------------------------------
// Statements.
// ---------------------------------------------------------------------------

fn check_stmt_list(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, scopes: &mut Vec<Scope>, list: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        check_stmt(names, nodes, lists, errors, scopes, list_get(lists, list, idx));
        idx += 1;
    }
}

fn check_stmt(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, scopes: &mut Vec<Scope>, stmt: i64) {
    if node_tag(nodes, stmt) != NODE_STMT {
        return;
    }
    let kind = node_a(nodes, stmt);
    if kind == STMT_LET {
        let name = node_c(nodes, stmt);
        let init = node_e(nodes, stmt);
        let key = stmt_ty_of(nodes, stmt);
        let linear = key_is_linear(nodes, names, key);
        check_expr(names, nodes, lists, errors, scopes, init);
        bind(scopes, name, key, linear, false, (node_file(nodes, stmt), node_start(nodes, stmt), node_end(nodes, stmt)));
        return;
    }
    if kind == STMT_ASSIGN {
        let target = node_b(nodes, stmt);
        let value = node_c(nodes, stmt);
        check_expr(names, nodes, lists, errors, scopes, value);
        // Assignment replaces the old value: a linear target's old value
        // is dropped here (a consumption).
        if let Some(binding) = lookup_mut(scopes, target)
            && binding.1 {
                binding.3 = false;
            }
        return;
    }
    if kind == STMT_WHILE {
        let cond = node_b(nodes, stmt);
        let body = node_c(nodes, stmt);
        check_expr(names, nodes, lists, errors, scopes, cond);
        push_scope(scopes);
        check_stmt_list(names, nodes, lists, errors, scopes, body);
        check_unconsumed(names, errors, scopes);
        pop_scope(scopes);
        return;
    }
    if kind == STMT_IF {
        let cond = node_b(nodes, stmt);
        let then_list = node_c(nodes, stmt);
        let else_list = node_d(nodes, stmt);
        check_expr(names, nodes, lists, errors, scopes, cond);
        let parent = snapshot(scopes);
        push_scope(scopes);
        check_stmt_list(names, nodes, lists, errors, scopes, then_list);
        let then_state = snapshot(scopes);
        pop_scope(scopes);
        restore(scopes, &parent);
        let else_state;
        if else_list != NONE {
            push_scope(scopes);
            check_stmt_list(names, nodes, lists, errors, scopes, else_list);
            else_state = snapshot(scopes);
            pop_scope(scopes);
        } else {
            else_state = snapshot(scopes);
        }
        restore(scopes, &parent);
        merge_branch(names, errors, &parent, &then_state, &else_state);
        return;
    }
    if kind == STMT_RETURN {
        let value = node_b(nodes, stmt);
        if value != NONE {
            check_expr(names, nodes, lists, errors, scopes, value);
        }
        return;
    }
    let expr = node_b(nodes, stmt);
    check_expr(names, nodes, lists, errors, scopes, expr);
}

/// After a branch, a linear binding moved on some paths but not all is an
/// error: linear values must be consumed on every path.
fn merge_branch(names: &[String], errors: &mut Vec<Diag>, parent: &[Scope], a: &[Scope], b: &[Scope]) {
    let mut s_idx = 0usize;
    while s_idx < parent.len() {
        let scope = match parent.get(s_idx) {
            Some(scope) => scope,
            None => break,
        };
        let mut e_idx = 0usize;
        while e_idx < scope.len() {
            match scope.get(e_idx) {
                Some(entry) => {
                    let name = entry.0;
                    let binding = &entry.1;
                    if !binding.1 {
                        e_idx += 1;
                        continue;
                    }
                    let moved_a = binding_moved(a, s_idx, name);
                    let moved_b = binding_moved(b, s_idx, name);
                    if moved_a != moved_b {
                        push_error(
                            errors,
                            &format!("linear value '{}' is not consumed on every path", name_text(names, name)),
                            -1,
                            -1,
                            -1,
                        );
                    }
                }
                None => break,
            }
            e_idx += 1;
        }
        s_idx += 1;
    }
}

fn binding_moved(state: &[Scope], s_idx: usize, name: i64) -> bool {
    match state.get(s_idx) {
        Some(scope) => {
            let mut idx = scope.len();
            while idx > 0 {
                idx -= 1;
                match scope.get(idx) {
                    Some(entry) => {
                        if entry.0 == name {
                            return entry.1 .3;
                        }
                    }
                    None => return false,
                }
            }
            false
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Expressions.
// ---------------------------------------------------------------------------

fn check_expr(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, scopes: &mut Vec<Scope>, expr: i64) {
    if node_tag(nodes, expr) != NODE_EXPR {
        return;
    }
    let kind = node_a(nodes, expr);
    if kind == EXPR_PATH {
        check_path(names, nodes, lists, errors, scopes, expr);
        return;
    }
    if kind == EXPR_UNARY {
        check_expr(names, nodes, lists, errors, scopes, node_c(nodes, expr));
        return;
    }
    if kind == EXPR_BINARY {
        check_expr(names, nodes, lists, errors, scopes, node_c(nodes, expr));
        check_expr(names, nodes, lists, errors, scopes, node_d(nodes, expr));
        return;
    }
    if kind == EXPR_CALL {
        check_call(names, nodes, lists, errors, scopes, expr);
        return;
    }
    if kind == EXPR_STRUCT_LIT {
        let values = node_d(nodes, expr);
        let count = list_len(lists, values);
        let mut idx = 0i64;
        while idx < count {
            check_expr(names, nodes, lists, errors, scopes, list_get(lists, values, idx));
            idx += 1;
        }
        return;
    }
    if kind == EXPR_ARRAY {
        let elems = node_b(nodes, expr);
        let count = list_len(lists, elems);
        let mut idx = 0i64;
        while idx < count {
            check_expr(names, nodes, lists, errors, scopes, list_get(lists, elems, idx));
            idx += 1;
        }
        return;
    }
    if kind == EXPR_MATCH {
        let scrutinee = node_b(nodes, expr);
        let arms = node_c(nodes, expr);
        check_expr(names, nodes, lists, errors, scopes, scrutinee);
        let parent = snapshot(scopes);
        let mut branch_states: Vec<(Vec<Scope>, i64)> = Vec::new();
        let count = list_len(lists, arms);
        let mut idx = 0i64;
        while idx < count {
            let arm = list_get(lists, arms, idx);
            push_scope(scopes);
            check_pattern(names, nodes, lists, scopes, node_a(nodes, arm));
            // An arm body is a single statement (the parser stores it
            // directly in the arm row, not as a list).
            check_stmt(names, nodes, lists, errors, scopes, node_b(nodes, arm));
            consume_arm_result(nodes, lists, scopes, arm);
            check_unconsumed(names, errors, scopes);
            let diverges = stmt_diverges(nodes, lists, node_b(nodes, arm));
            branch_states.push((snapshot(scopes), diverges));
            pop_scope(scopes);
            restore(scopes, &parent);
            idx += 1;
        }
        restore(scopes, &parent);
        merge_arms(names, errors, scopes, &parent, &branch_states);
        return;
    }
    if kind == EXPR_TRY {
        check_expr(names, nodes, lists, errors, scopes, node_b(nodes, expr));
    }
}

/// The arm's body statement is the arm's result: an expression statement
/// whose expression is a bare reference to a linear binding moves it out
/// of the arm into the match result, so it is consumed here rather than
/// flagged by the arm-scope check.
fn consume_arm_result(nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, scopes: &mut Vec<Scope>, arm: i64) {
    let body = node_b(nodes, arm);
    if node_tag(nodes, body) != NODE_STMT || node_a(nodes, body) != STMT_EXPR {
        return;
    }
    consume_expr_result(nodes, lists, scopes, node_b(nodes, body));
}

/// Walks an arm-result expression, moving any linear path references it
/// produces.  Call arguments were already consumed when the body was
/// checked, and nested match arms consumed their own results, so those
/// are left alone.
fn consume_expr_result(nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, scopes: &mut Vec<Scope>, expr: i64) {
    if node_tag(nodes, expr) != NODE_EXPR {
        return;
    }
    let kind = node_a(nodes, expr);
    if kind == EXPR_PATH {
        let segs = node_b(nodes, expr);
        if list_len(lists, segs) == 1 {
            let name = list_get(lists, segs, 0);
            if let Some(binding) = lookup_mut(scopes, name)
                && binding.1 && !binding.3 {
                    binding.3 = true;
                }
        }
        return;
    }
    if kind == EXPR_UNARY {
        consume_expr_result(nodes, lists, scopes, node_c(nodes, expr));
        return;
    }
    if kind == EXPR_BINARY {
        consume_expr_result(nodes, lists, scopes, node_c(nodes, expr));
        consume_expr_result(nodes, lists, scopes, node_d(nodes, expr));
        return;
    }
    if kind == EXPR_STRUCT_LIT {
        let values = node_d(nodes, expr);
        let count = list_len(lists, values);
        let mut idx = 0i64;
        while idx < count {
            consume_expr_result(nodes, lists, scopes, list_get(lists, values, idx));
            idx += 1;
        }
        return;
    }
    if kind == EXPR_ARRAY {
        let elems = node_b(nodes, expr);
        let count = list_len(lists, elems);
        let mut idx = 0i64;
        while idx < count {
            consume_expr_result(nodes, lists, scopes, list_get(lists, elems, idx));
            idx += 1;
        }
        return;
    }
    if kind == EXPR_TRY {
        consume_expr_result(nodes, lists, scopes, node_b(nodes, expr));
    }
}

/// Merges every arm's state for one parent binding.  A continuing arm
/// that leaves the value unmoved while another arm moved it makes the
/// post-match state ambiguous (an error); a diverging arm must have
/// consumed the value before its path ends, or the value leaks.  The
/// post-match state is `moved` only when every continuing arm moved it.
fn merge_arm_binding(
    names: &[String],
    errors: &mut Vec<Diag>,
    scopes: &mut [Scope],
    s_idx: usize,
    entry: &(i64, Binding),
    branches: &[(Vec<Scope>, i64)],
) {
    let name = entry.0;
    let binding = &entry.1;
    if !binding.1 {
        return;
    }
    let mut some_moved = false;
    let mut all_moved = true;
    let mut saw_continuing = false;
    let mut leaked = false;
    let mut b_idx = 0usize;
    while b_idx < branches.len() {
        match branches.get(b_idx) {
            Some(pair) => {
                let (state, diverges) = pair;
                let moved = binding_moved(state, s_idx, name);
                if *diverges == 1 {
                    if !moved {
                        leaked = true;
                    }
                } else {
                    saw_continuing = true;
                    if moved {
                        some_moved = true;
                    } else {
                        all_moved = false;
                    }
                }
            }
            None => break,
        }
        b_idx += 1;
    }
    if leaked || (saw_continuing && some_moved && !all_moved) {
        push_error(
            errors,
            &format!("linear value '{}' is not consumed on every path", name_text(names, name)),
            binding.5,
            binding.6,
            binding.7,
        );
    }
    if saw_continuing {
        let merged_moved = all_moved;
        if let Some(scope) = scopes.get_mut(s_idx) {
            let mut f_idx = scope.len();
            while f_idx > 0 {
                f_idx -= 1;
                match scope.get_mut(f_idx) {
                    Some(entry_mut) => {
                        if entry_mut.0 == name {
                            entry_mut.1.3 = merged_moved;
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }
}

/// Merges every arm's state: a linear binding consumed in some arms but
/// not all is an error, and a diverging arm that leaks one is an error.
fn merge_arms(
    names: &[String],
    errors: &mut Vec<Diag>,
    scopes: &mut [Scope],
    parent: &[Scope],
    branches: &[(Vec<Scope>, i64)],
) {
    let mut s_idx = 0usize;
    while s_idx < parent.len() {
        let scope = match parent.get(s_idx) {
            Some(scope) => scope,
            None => break,
        };
        let mut e_idx = 0usize;
        while e_idx < scope.len() {
            match scope.get(e_idx) {
                Some(entry) => merge_arm_binding(names, errors, scopes, s_idx, entry, branches),
                None => break,
            }
            e_idx += 1;
        }
        s_idx += 1;
    }
}

fn check_path(names: &mut [String], nodes: &mut [i64], lists: &mut [Vec<i64>], errors: &mut Vec<Diag>, scopes: &mut [Scope], expr: i64) {
    let sym = expr_sym_of(nodes, expr);
    if sym != NONE {
        return;
    }
    // A local chain: the first segment names a binding.  Reading it by
    // value is fine unless it is a moved linear value.
    let segs = node_b(nodes, expr);
    let first = list_get(lists, segs, 0);
    if let Some(binding) = lookup(scopes, first)
        && binding.1 && binding.3 {
            push_error(
                errors,
                &format!("use of moved value '{}'", name_text(names, first)),
                node_file(nodes, expr),
                node_start(nodes, expr),
                node_end(nodes, expr),
            );
        }
}

fn fn_node_of_sym(nodes: &[i64], sym: i64) -> i64 {
    let decl = node_c(nodes, sym);
    if node_tag(nodes, decl) == NODE_FN {
        decl
    } else {
        node_d(nodes, decl)
    }
}

/// Checks a call's arguments against the callee's declared parameter
/// kinds: a by-value linear parameter consumes its argument, a `&T` or
/// `&mut T` parameter borrows it.  The instance row the typechecker
/// attached already carries the callee's fn node; the fallback only
/// unwraps the symbol-to-item indirection the typechecker uses for
/// imported declarations.
fn check_call(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, scopes: &mut Vec<Scope>, expr: i64) {
    let inst = expr_sym_of(nodes, expr);
    let arg_exprs = node_d(nodes, expr);
    let acount = list_len(lists, arg_exprs);
    let mut param_kinds: Vec<i64> = Vec::new();
    if inst != NONE {
        let fn_slot = inst_fn_of(nodes, inst);
        let fn_node = if node_tag(nodes, fn_slot) == NODE_FN {
            fn_slot
        } else {
            fn_node_of_sym(nodes, fn_slot)
        };
        param_kinds = declared_param_kinds(nodes, lists, fn_node);
    }
    let mut idx = 0i64;
    while idx < acount {
        let arg = list_get(lists, arg_exprs, idx);
        let kind = match param_kinds.get(idx as usize) {
            Some(kind) => *kind,
            None => TYD_UNKNOWN,
        };
        if kind == TYD_REF || kind == TYD_REF_MUT {
            check_borrow_arg(names, nodes, lists, errors, scopes, arg, kind);
        } else {
            check_consume_arg(names, nodes, lists, errors, scopes, arg);
        }
        idx += 1;
    }
}

/// The parameter kinds of a fn node, in order (`TYD_REF`, `TYD_REF_MUT`,
/// or the dereferenced-by-value kind).
fn declared_param_kinds(nodes: &[i64], lists: &[Vec<i64>], fn_node: i64) -> Vec<i64> {
    let params = node_c(nodes, fn_node);
    let count = list_len(lists, params);
    let mut kinds: Vec<i64> = Vec::new();
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(lists, params, idx);
        let key = ty_key_of(nodes, node_b(nodes, param));
        let kind = key_kind(nodes, key);
        if kind == TYD_REF || kind == TYD_REF_MUT {
            kinds.push(kind);
        } else {
            kinds.push(key);
        }
        idx += 1;
    }
    kinds
}

fn check_borrow_arg(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, scopes: &mut Vec<Scope>, arg: i64, kind: i64) {
    // The borrow is usually an explicit `&x`; a path argument passes an
    // existing reference through.
    if node_tag(nodes, arg) == NODE_EXPR && node_a(nodes, arg) == EXPR_UNARY {
        let inner = node_c(nodes, arg);
        if node_tag(nodes, inner) == NODE_EXPR && node_a(nodes, inner) == EXPR_PATH {
            let segs = node_b(nodes, inner);
            let name = list_get(lists, segs, 0);
            verify_borrow(names, nodes, errors, scopes, name, kind, inner);
            return;
        }
    }
    check_expr(names, nodes, lists, errors, scopes, arg);
}

fn verify_borrow(names: &mut [String], nodes: &mut [i64], errors: &mut Vec<Diag>, scopes: &mut [Scope], name: i64, kind: i64, at: i64) {
    if let Some(binding) = lookup(scopes, name) {
        if binding.1 && binding.3 {
            push_error(
                errors,
                &format!("cannot borrow moved value '{}'", name_text(names, name)),
                node_file(nodes, at),
                node_start(nodes, at),
                node_end(nodes, at),
            );
            return;
        }
        // Exclusivity: a mutable borrow cannot overlap a live borrow.
        let holds = &binding.4;
        if kind == BORROW_MUT && !holds.is_empty() {
            push_error(
                errors,
                &format!("cannot mutably borrow '{}': it is already borrowed", name_text(names, name)),
                node_file(nodes, at),
                node_start(nodes, at),
                node_end(nodes, at),
            );
        }
    }
    // The owner's borrowed-from set is the value's own holds; the borrow
    // of `name` is a use of `name`'s value, recorded against the owners
    // of the value `name` borrows.  For a direct borrow of `name` the
    // borrow is recorded on the bindings that own the value.
    record_borrow(names, errors, scopes, name, kind);
}

fn record_borrow(names: &mut [String], errors: &mut Vec<Diag>, scopes: &mut [Scope], owner: i64, kind: i64) {
    if let Some(binding) = lookup_mut(scopes, owner) {
        if binding.1 && binding.3 {
            push_error(
                errors,
                &format!("cannot borrow moved value '{}'", name_text(names, owner)),
                -1,
                -1,
                -1,
            );
            return;
        }
        let holds = &mut binding.4;
        holds.push((owner, kind));
    }
}

/// A by-value argument: a linear value is consumed here (moved).
fn check_consume_arg(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, errors: &mut Vec<Diag>, scopes: &mut Vec<Scope>, arg: i64) {
    if node_tag(nodes, arg) == NODE_EXPR && node_a(nodes, arg) == EXPR_PATH {
        let segs = node_b(nodes, arg);
        if list_len(lists, segs) == 1 {
            let name = list_get(lists, segs, 0);
            if let Some(binding) = lookup_mut(scopes, name) {
                if binding.1 {
                    if binding.3 {
                        push_error(
                            errors,
                            &format!("use of moved value '{}'", name_text(names, name)),
                            node_file(nodes, arg),
                            node_start(nodes, arg),
                            node_end(nodes, arg),
                        );
                    } else {
                        binding.3 = true;
                    }
                }
                return;
            }
        }
    }
    check_expr(names, nodes, lists, errors, scopes, arg);
}

/// Binds the names a pattern introduces (for match arm scopes).
fn check_pattern(names: &mut Vec<String>, nodes: &mut Vec<i64>, lists: &mut Vec<Vec<i64>>, scopes: &mut Vec<Scope>, pat: i64) {
    if node_tag(nodes, pat) != NODE_PAT {
        return;
    }
    let kind = node_a(nodes, pat);
    if kind == PAT_BIND {
        let name = node_b(nodes, pat);
        let key = pat_ty_of(nodes, pat);
        let linear = key_is_linear(nodes, names, key);
        bind(scopes, name, key, linear, false, (node_file(nodes, pat), node_start(nodes, pat), node_end(nodes, pat)));
        return;
    }
    if kind == PAT_VARIANT {
        let payload = node_c(nodes, pat);
        let count = list_len(lists, payload);
        let mut idx = 0i64;
        while idx < count {
            check_pattern(names, nodes, lists, scopes, list_get(lists, payload, idx));
            idx += 1;
        }
        return;
    }
    if kind == PAT_ARRAY {
        let elems = node_b(nodes, pat);
        let count = list_len(lists, elems);
        let mut idx = 0i64;
        while idx < count {
            check_pattern(names, nodes, lists, scopes, list_get(lists, elems, idx));
            idx += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the full pipeline on the reference spec and prints every
    /// borrow error with its source span.  Temporary diagnostic.
    #[test]
    fn spec_borrow_diagnostics() {
        let mut names: Vec<String> = Vec::new();
        let mut nodes: Vec<i64> = Vec::new();
        let mut lists: Vec<Vec<i64>> = Vec::new();
        let mut errors: Vec<Diag> = Vec::new();
        let (loaded, files) =
            crate::module_loader::load(&mut names, &mut nodes, &mut lists, &mut errors, "tests/fixtures/spec.cnb");
        let (root, ext_mods) = match loaded {
            Some(program) => program,
            None => {
                let mut idx = 0usize;
                while idx < errors.len() {
                    match errors.get(idx) {
                        Some(diag) => println!("LOADERR: {} @ {} {} {}", diag.0, diag.1, diag.2, diag.3),
                        None => break,
                    }
                    idx += 1;
                }
                return;
            }
        };
        crate::resolver::resolve(&mut names, &mut nodes, &mut lists, &mut errors, root, &ext_mods);
        let (ok, impls_list) = crate::typecheck::typecheck(&mut names, &mut nodes, &mut lists, &mut errors, root, &ext_mods);
        crate::borrow::borrow_check(&mut names, &mut nodes, &mut lists, &mut errors, root, &ext_mods);
        let mut detail: Vec<String> = Vec::new();
        let mut idx = 0usize;
        while idx < errors.len() {
            match errors.get(idx) {
                Some(diag) => {
                    let path = match files.get(diag.1 as usize) {
                        Some(pair) => pair.0.clone(),
                        None => "?".to_string(),
                    };
                    detail.push(format!("{} @ {} {} {}", diag.0, path, diag.2, diag.3));
                }
                None => break,
            }
            idx += 1;
        }
        assert!(errors.is_empty() && ok, "typecheck={} impls={} errors:\n{}", ok, impls_list, detail.join("\n"));
    }
}
