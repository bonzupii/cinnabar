//! Name resolution, scope construction, and casing enforcement.
//!
//! `resolve` builds a scope tree over two separate namespaces, `NS_TYPE`
//! and `NS_VALUE`, then rewrites every unresolved `EXPR_PATH`, `TY_PATH`,
//! and `PAT_PATH` segment into a `NODE_SYM` reference. It seeds the
//! built-in primitives and native surfaces, hoists top-level declarations
//! so declaration order does not matter, wires in each sibling module's
//! scope from the loader's `ext_mods`, and resolves `use` imports including
//! `as` aliases.
//!
//! The compiler-checked casing rules are enforced here, on the hoisted
//! declaration rather than at every use: a mis-cased identifier is one
//! resolver error and never reaches the typechecker.
//!
//! Three tags are assigned at this point precisely so that no later stage
//! has to recognize something by its name — the program's entry point
//! (`SYM_FUN_MAIN`), each native operation's `NAT_*` verb, and each native
//! function's ownership mode (`sym_native_mode`), derived once by the
//! signature classifier from the seeded registry.  The registry tables are
//! the single declaration of the native surface: a native outside them is
//! a resolution error naming the function.
//!
//! **Invariants:**
//! - The symbol a name resolves to is decided here and nowhere else. A
//!   later stage that needs it reads the attached `NODE_SYM`; it does not
//!   re-walk the path and re-match segments with parallel logic.
//! - Downstream semantics key off `SYM_FUN_MAIN`, the `NAT_*` verbs, and
//!   the attached ownership modes, never off a string comparison against
//!   `"main"` or a native function's name. Attaching the tags here is
//!   what makes that rule keepable.
//! - A name that does not resolve produces a diagnostic at the use site's
//!   real span, optionally carrying a hedged suggestion. It never quietly
//!   resolves to a plausible neighbour.
use crate::ast::*;
use crate::suggest;
use crate::target::{NativeSubsystem, Target};

pub const NS_TYPE: i64 = 0;
pub const NS_VALUE: i64 = 1;

type State<'a> = (
    &'a mut Vec<String>,
    &'a mut Vec<i64>,
    &'a mut Vec<Vec<i64>>,
    &'a mut Vec<Diag>,
    &'a mut Vec<Vec<i64>>,
    &'a mut Vec<i64>,
    &'a mut Vec<i64>,
    &'a mut Vec<i64>,
    &'a mut Vec<(i64, i64)>,
    &'a mut Vec<i64>,
    // Secondary notes tied to the errors this stage reports (definition-site
    // labels and hedged name suggestions), indexed like the borrow checker's.
    &'a mut Vec<Note>,
    // The item whose body is currently being resolved, innermost last.
    &'a mut Vec<i64>,
    // Dependency edges for reachability from main.
    &'a mut Vec<(i64, i64)>,
    // Seeded builtin names and symbols, filled during resolution.
    &'a mut Seeds,
    // The platform a build is for, so resolution can reject an operation a
    // target does not support before code generation begins.
    &'a Target,
);

/// The owner of references belonging to no nameable item: the permanent
/// root the reachability fixpoint seeds with.
const ROOT_OWNER: i64 = -2;

/// Resolves names and reports which items and imports nothing reaches;
/// unused-item and unused-import diagnostics arrive in `deferred`.
pub struct Diagnostics<'a> {
    pub errors: &'a mut Vec<Diag>,
    pub notes: &'a mut Vec<Note>,
    /// Reported by the caller once the later stages have run, not here.
    pub deferred: &'a mut Vec<Diag>,
    pub target: &'a Target,
}

pub fn resolve(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    diagnostics: Diagnostics,
    root: i64,
    ext_mods: &[(i64, i64)],
    seeds: &mut Seeds,
) -> bool {
    let Diagnostics { errors, notes, deferred, target } = diagnostics;
    let mut scopes: Vec<Vec<i64>> = Vec::new();
    let mut parents: Vec<i64> = Vec::new();
    let mut pubs: Vec<i64> = Vec::new();
    let mut prefixes: Vec<i64> = Vec::new();
    let mut item_scopes: Vec<(i64, i64)> = Vec::new();
    let mut used: Vec<i64> = Vec::new();
    let mut owners: Vec<i64> = Vec::new();
    let mut edges: Vec<(i64, i64)> = Vec::new();
    let empty_prefix = alloc_list(lists);
    let root_scope = alloc_scope(&mut scopes, &mut parents, &mut pubs, &mut prefixes, NONE, empty_prefix, 1);
    let mut state: State = (
        names,
        nodes,
        lists,
        errors,
        &mut scopes,
        &mut parents,
        &mut pubs,
        &mut prefixes,
        &mut item_scopes,
        &mut used,
        notes,
        &mut owners,
        &mut edges,
        seeds,
        target,
    );
    seed_builtins(&mut state, root_scope, root);

    collect_list(&mut state, root_scope, root);

    let mut ext_scopes: Vec<(i64, i64)> = Vec::new();
    let mut idx = 0usize;
    while idx < ext_mods.len() {
        match ext_mods.get(idx) {
            Some(pair) => {
                let mod_name = pair.0;
                let prefix = alloc_list(state.2);
                list_push(state.2, prefix, mod_name);
                let ext_scope = alloc_scope(state.4, state.5, state.6, state.7, root_scope, prefix, 1);
                let ext_sym = alloc_sym(state.1, SYM_MODULE, mod_name, NONE, root_scope, ext_scope);
                push_entry(state.4, root_scope, mod_name, ext_sym, NS_TYPE, NONE);
                ext_scopes.push((pair.0, ext_scope));
                collect_list(&mut state, ext_scope, pair.1);
            }
            None => break,
        }
        idx += 1;
    }

    resolve_imports(&mut state, root_scope, root);
    idx = 0;
    while idx < ext_mods.len() {
        match ext_mods.get(idx) {
            Some(pair) => {
                let ext_scope = ext_scope_of(&ext_scopes, pair.0);
                resolve_imports(&mut state, ext_scope, pair.1);
            }
            None => break,
        }
        idx += 1;
    }

    walk_item_list(&mut state, root_scope, root);
    idx = 0;
    while idx < ext_mods.len() {
        match ext_mods.get(idx) {
            Some(pair) => {
                let ext_scope = ext_scope_of(&ext_scopes, pair.0);
                walk_item_list(&mut state, ext_scope, pair.1);
            }
            None => break,
        }
        idx += 1;
    }

    classify_native_modes(&mut state);
    link_extraction_surfaces(&mut state);

    // Unused imports are whole-program facts; the diagnostic is deferred
    // with the reachability set and reported after the later stages run.
    check_unused_imports(state.0, state.1, state.2, deferred, state.9, root);
    idx = 0;
    while idx < ext_mods.len() {
        match ext_mods.get(idx) {
            Some(pair) => check_unused_imports(state.0, state.1, state.2, deferred, state.9, pair.1),
            None => break,
        }
        idx += 1;
    }

    materialize_scope_facts(&mut state);

    // Reachability last, and only if nothing else failed.
    if state.3.is_empty()
        && let Some(reached) = reachable_from_main(&state)
    {
        report_unreachable(&mut state, root, &reached, deferred);
        idx = 0;
        while idx < ext_mods.len() {
            match ext_mods.get(idx) {
                Some(pair) => report_unreachable(&mut state, pair.1, &reached, deferred),
                None => break,
            }
            idx += 1;
        }
    }

    state.3.is_empty()
}

fn materialize_scope_facts(state: &mut State) {
    let mut visible: Vec<(i64, i64, i64, i64)> = Vec::new();
    let mut members: Vec<(i64, i64, i64, i64, i64)> = Vec::new();
    let scope_count = state.4.len() as i64;
    let mut query = 0i64;
    while query < scope_count {
        let mut seen: Vec<(i64, i64)> = Vec::new();
        let mut current = query;
        while current != NONE {
            let entries = match state.4.get(current as usize) {
                Some(value) => value,
                None => break,
            };
            let count = entries.len() as i64 / 4;
            let mut idx = 0i64;
            while idx < count {
                let name = entry_get(entries, idx, 0);
                let sym = entry_get(entries, idx, 1);
                let namespace = entry_get(entries, idx, 2);
                if sym != NONE
                    && !seen.contains(&(name, namespace))
                    && is_visible(state.5, state.6, state.1, query, sym)
                {
                    visible.push((query, name, sym, namespace));
                    seen.push((name, namespace));
                }
                idx += 1;
            }
            current = parent_of(state.5, current);
        }
        let mut member_scope = 0i64;
        while member_scope < scope_count {
            let entries = match state.4.get(member_scope as usize) {
                Some(value) => value,
                None => break,
            };
            let count = entries.len() as i64 / 4;
            let mut idx = 0i64;
            while idx < count {
                let name = entry_get(entries, idx, 0);
                let sym = entry_get(entries, idx, 1);
                let namespace = entry_get(entries, idx, 2);
                if sym != NONE && is_visible(state.5, state.6, state.1, query, sym) {
                    members.push((query, member_scope, name, sym, namespace));
                }
                idx += 1;
            }
            member_scope += 1;
        }
        query += 1;
    }
    let mut visible_idx = 0usize;
    while visible_idx < visible.len() {
        match visible.get(visible_idx) {
            Some(row) => {
                alloc_scope_visible(state.1, row.0, row.1, row.2, row.3);
            }
            None => break,
        }
        visible_idx += 1;
    }
    let mut member_idx = 0usize;
    while member_idx < members.len() {
        match members.get(member_idx) {
            Some(row) => {
                alloc_scope_member(state.1, row.0, row.1, row.2, row.3, row.4);
            }
            None => break,
        }
        member_idx += 1;
    }
}

fn attach_scope(state: &mut State, source: i64, scope: i64) {
    let count = state.1.len() as i64 / NODE_STRIDE;
    let mut idx = 0i64;
    while idx < count {
        if node_tag(state.1, idx) == NODE_SCOPEFACT
            && node_a(state.1, idx) == SCOPE_AT
            && node_b(state.1, idx) == source
        {
            return;
        }
        idx += 1;
    }
    alloc_scope_at(state.1, source, scope);
}

fn alloc_scope(scopes: &mut Vec<Vec<i64>>, parents: &mut Vec<i64>, pubs: &mut Vec<i64>, prefixes: &mut Vec<i64>, parent: i64, prefix: i64, is_pub: i64) -> i64 {
    scopes.push(Vec::new());
    parents.push(parent);
    pubs.push(is_pub);
    prefixes.push(prefix);
    scopes.len() as i64 - 1
}

fn scope_prefix_of(prefixes: &[i64], scope: i64) -> i64 {
    match prefixes.get(scope as usize) {
        Some(prefix) => *prefix,
        None => NONE,
    }
}

fn parent_of(parents: &[i64], scope: i64) -> i64 {
    match parents.get(scope as usize) {
        Some(parent) => *parent,
        None => NONE,
    }
}

fn is_pub_scope(pubs: &[i64], scope: i64) -> i64 {
    match pubs.get(scope as usize) {
        Some(is_pub) => *is_pub,
        None => 0,
    }
}

fn entry_get(entries: &[i64], idx: i64, slot: i64) -> i64 {
    match entries.get((idx * 4 + slot) as usize) {
        Some(value) => *value,
        None => NONE,
    }
}

fn entry_set(entries: &mut [i64], idx: i64, slot: i64, value: i64) -> bool {
    match entries.get_mut((idx * 4 + slot) as usize) {
        Some(cell) => {
            *cell = value;
            true
        }
        None => false,
    }
}

fn push_entry(scopes: &mut [Vec<i64>], scope: i64, name: i64, sym: i64, ns: i64, src: i64) {
    if let Some(entries) = scopes.get_mut(scope as usize) {
        entries.push(name);
        entries.push(sym);
        entries.push(ns);
        entries.push(src);
    }
}

/// The symbol a name binds to in one scope, and the `use` item that bound
/// it there.
///
/// An entry still carrying `NONE` in its symbol slot binds nothing: it is
/// the placeholder `collect_item` reserves for a `use` before the path that
/// `use` names has been resolved, and the scan continues past it rather
/// than answering "this name is taken". That is what makes this lookup give
/// the same answer wherever in the file the `use` was written.
fn scope_lookup(scopes: &[Vec<i64>], scope: i64, name: i64, ns: i64) -> (i64, i64) {
    let entries = match scopes.get(scope as usize) {
        Some(entries) => entries,
        None => return (NONE, NONE),
    };
    let mut idx = 0i64;
    while idx < entries.len() as i64 / 4 {
        if entry_get(entries, idx, 0) == name
            && entry_get(entries, idx, 2) == ns
            && entry_get(entries, idx, 1) != NONE
        {
            return (entry_get(entries, idx, 1), entry_get(entries, idx, 3));
        }
        idx += 1;
    }
    (NONE, NONE)
}

fn lookup_walk(scopes: &[Vec<i64>], parents: &[i64], scope: i64, name: i64, ns: i64) -> (i64, i64) {
    let mut current = scope;
    loop {
        let found = scope_lookup(scopes, current, name, ns);
        if found.0 != NONE {
            return found;
        }
        let parent = parent_of(parents, current);
        if parent == NONE {
            return (NONE, NONE);
        }
        current = parent;
    }
}

fn contains_i64(list: &[i64], needle: i64) -> bool {
    let mut idx = 0usize;
    loop {
        match list.get(idx) {
            Some(value) => {
                if *value == needle {
                    return true;
                }
            }
            None => return false,
        }
        idx += 1;
    }
}

fn alloc_sym(nodes: &mut Vec<i64>, kind: i64, name: i64, decl: i64, home: i64, sub: i64) -> i64 {
    alloc_node(nodes, &[NODE_SYM, NO_FILE, NO_FILE, NO_FILE, kind, name, decl, home, sub, NONE])
}

fn sym_kind_of(nodes: &[i64], sym: i64) -> i64 {
    node_a(nodes, sym)
}

fn sym_name_of(nodes: &[i64], sym: i64) -> i64 {
    node_b(nodes, sym)
}

fn sym_decl_of(nodes: &[i64], sym: i64) -> i64 {
    node_c(nodes, sym)
}

fn sym_home_of(nodes: &[i64], sym: i64) -> i64 {
    node_d(nodes, sym)
}

fn sym_sub_of(nodes: &[i64], sym: i64) -> i64 {
    // Slot e is a child scope only for namespace kinds and seeded builtin
    // types; a declared native type stores its container role there.
    let kind = sym_kind_of(nodes, sym);
    let is_namespace = kind == SYM_MODULE || kind == SYM_STRUCT || kind == SYM_ENUM || kind == SYM_TRAIT;
    let is_seeded_builtin = kind == SYM_TYPE && sym_decl_of(nodes, sym) == NONE;
    if is_namespace || is_seeded_builtin {
        node_e(nodes, sym)
    } else {
        NONE
    }
}

fn sym_is_pub(nodes: &[i64], sym: i64) -> i64 {
    let decl = sym_decl_of(nodes, sym);
    if decl == NONE {
        return 1;
    }
    if node_tag(nodes, decl) == NODE_ITEM {
        return item_is_pub(nodes, decl);
    }
    if node_tag(nodes, decl) == NODE_VARIANT {
        let enum_item = node_e(nodes, decl);
        if enum_item != NONE && node_tag(nodes, enum_item) == NODE_ITEM {
            return item_is_pub(nodes, enum_item);
        }
        return node_c(nodes, decl);
    }
    1
}

fn qualified_name(names: &mut Vec<String>, lists: &[Vec<i64>], prefix: i64, name: i64) -> i64 {
    let mut text = String::new();
    let count = list_len(lists, prefix);
    let mut idx = 0i64;
    while idx < count {
        if !text.is_empty() {
            text.push('.');
        }
        text.push_str(&name_text(names, list_get(lists, prefix, idx)));
        idx += 1;
    }
    if !text.is_empty() {
        text.push('.');
    }
    text.push_str(&name_text(names, name));
    intern(names, &text)
}

fn is_visible(parents: &[i64], pubs: &[i64], nodes: &[i64], scope: i64, sym: i64) -> bool {
    let home = sym_home_of(nodes, sym);
    let mut ancestors: Vec<i64> = Vec::new();
    let mut current = scope;
    loop {
        ancestors.push(current);
        let parent = parent_of(parents, current);
        if parent == NONE {
            break;
        }
        current = parent;
    }
    if contains_i64(&ancestors, home) {
        return true;
    }
    let mut crossed: Vec<i64> = Vec::new();
    let mut up = home;
    loop {
        if contains_i64(&ancestors, up) {
            break;
        }
        crossed.push(up);
        let parent = parent_of(parents, up);
        if parent == NONE {
            break;
        }
        up = parent;
    }
    let mut idx = 0usize;
    while let Some(scope_id) = crossed.get(idx) {
        if is_pub_scope(pubs, *scope_id) != 1 {
            return false;
        }
        idx += 1;
    }
    sym_is_pub(nodes, sym) == 1
}

fn first_char_lower(text: &str) -> bool {
    match text.chars().next() {
        Some(ch) => ch.is_ascii_lowercase(),
        None => false,
    }
}

fn first_char_upper(text: &str) -> bool {
    match text.chars().next() {
        Some(ch) => ch.is_ascii_uppercase(),
        None => false,
    }
}

fn all_lower_rest(text: &str) -> bool {
    let mut first = true;
    for ch in text.chars() {
        if first {
            first = false;
            continue;
        }
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '_' {
            return false;
        }
    }
    true
}

fn all_pascal_rest(text: &str) -> bool {
    let mut first = true;
    for ch in text.chars() {
        if first {
            first = false;
            continue;
        }
        if !ch.is_ascii_lowercase() && !ch.is_ascii_uppercase() && !ch.is_ascii_digit() {
            return false;
        }
    }
    true
}

fn all_screaming_rest(text: &str) -> bool {
    let mut first = true;
    for ch in text.chars() {
        if first {
            first = false;
            continue;
        }
        if !ch.is_ascii_uppercase() && !ch.is_ascii_digit() && ch != '_' {
            return false;
        }
    }
    true
}

fn casing_ok(names: &[String], name: i64, casing: i64) -> i64 {
    let text = name_text(names, name);
    if casing == 1 {
        if first_char_lower(&text) && all_lower_rest(&text) {
            return 1;
        }
    } else if casing == 2 {
        if first_char_upper(&text) && all_pascal_rest(&text) {
            return 1;
        }
    } else if casing == 3
        && (first_char_upper(&text) || text.starts_with('_')) && all_screaming_rest(&text) {
            return 1;
        }
    0
}

fn report_casing(names: &[String], name: i64, casing: i64, errors: &mut Vec<Diag>, file: i64, start: i64, end: i64) {
    if casing_ok(names, name, casing) == 0 {
        push_error_kind(
            errors,
            &format!("'{}' violates casing rule: expected {}", name_text(names, name), casing_rule_name(casing)),
            file,
            start,
            end,
            DiagKind::CasingViolation { name, expected: casing },
        );
    }
}

fn insert_decl(state: &mut State, scope: i64, name: i64, sym: i64, ns: i64, casing: i64, span: (i64, i64, i64)) -> i64 {
    report_casing(state.0, name, casing, state.3, span.0, span.1, span.2);
    let existing = scope_lookup(state.4, scope, name, ns);
    if existing.0 == NONE {
        push_entry(state.4, scope, name, sym, ns, NONE);
        return 1;
    }
    let existing_decl = sym_decl_of(state.1, existing.0);
    let incoming_decl = sym_decl_of(state.1, sym);
    let builtin_collision = existing_decl == NONE
        || (existing_decl != NONE && node_file(state.1, existing_decl) == NO_FILE)
        || (incoming_decl != NONE && node_file(state.1, incoming_decl) == NO_FILE);
    let message = if builtin_collision {
        format!("cannot redeclare builtin '{}'", name_text(state.0, name))
    } else {
        format!("duplicate symbol '{}'", name_text(state.0, name))
    };
    let report_span = if incoming_decl != NONE
        && node_file(state.1, incoming_decl) == NO_FILE
        && existing_decl != NONE
        && node_file(state.1, existing_decl) != NO_FILE
    {
        (
            node_file(state.1, existing_decl),
            node_start(state.1, existing_decl),
            node_end(state.1, existing_decl),
        )
    } else {
        span
    };
    push_error(state.3, &message, report_span.0, report_span.1, report_span.2);
    // A duplicate points at both clash sites; a builtin redeclaration has
    // no source origin, so it gets no note.
    if !builtin_collision && existing_decl != NONE {
        push_note_for_last(
            state.3,
            state.10,
            "first declared here",
            node_file(state.1, existing_decl),
            node_start(state.1, existing_decl),
            node_end(state.1, existing_decl),
            NOTE_CONTEXT,
        );
    }
    0
}

fn collect_list(state: &mut State, scope: i64, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        collect_item(state, scope, list_get(state.2, list, idx));
        idx += 1;
    }
}

fn collect_item(state: &mut State, scope: i64, item: i64) {
    if node_tag(state.1, item) != NODE_ITEM {
        return;
    }
    let kind = node_a(state.1, item);
    let is_pub = item_is_pub(state.1, item);
    let file = node_file(state.1, item);
    let start = node_start(state.1, item);
    let end = node_end(state.1, item);
    let prefix = scope_prefix_of(state.7, scope);
    if kind == ITEM_MODULE {
        let name = node_d(state.1, item);
        let full = qualified_name(state.0, state.2, prefix, name);
        let sym = alloc_sym(state.1, SYM_MODULE, full, item, scope, NONE);
        insert_decl(state, scope, name, sym, NS_TYPE, 2, (file, start, end));
        let sub_prefix = alloc_list(state.2);
        copy_list(state.2, prefix, sub_prefix);
        list_push(state.2, sub_prefix, name);
        let sub = alloc_scope(state.4, state.5, state.6, state.7, scope, sub_prefix, is_pub);
        node_set_e(state.1, sym, sub);
        state.8.push((item, sub));
        let children = node_e(state.1, item);
        collect_list(state, sub, children);
    } else if kind == ITEM_STRUCT {
        let name = node_d(state.1, item);
        let full = qualified_name(state.0, state.2, prefix, name);
        let sym = alloc_sym(state.1, SYM_STRUCT, full, item, scope, NONE);
        insert_decl(state, scope, name, sym, NS_TYPE, 2, (file, start, end));
        item_set_sym(state.1, item, sym);
        let sub = alloc_scope(state.4, state.5, state.6, state.7, scope, prefix, 1);
        node_set_e(state.1, sym, sub);
        state.8.push((item, sub));
        enter_type_params(state.1, state.2, state.4, sub, node_f(state.1, item));
        collect_fields_casing(state.0, state.1, state.2, state.3, item, scope);
    } else if kind == ITEM_ENUM {
        let name = node_d(state.1, item);
        let full = qualified_name(state.0, state.2, prefix, name);
        let sym = alloc_sym(state.1, SYM_ENUM, full, item, scope, NONE);
        let prim = prim_kind_of(state.0, full);
        sym_set_prim_kind(state.1, sym, prim);
        if prim == PRIM_UNIT {
            state.13.set_sym(SEED_SYM_UNIT, sym);
        } else if prim == PRIM_RESULT {
            state.13.set_sym(SEED_SYM_RESULT, sym);
        } else if prim == PRIM_OPTION {
            state.13.set_sym(SEED_SYM_OPTION, sym);
        } else if prim == PRIM_DIV_ERROR {
            state.13.set_sym(SEED_SYM_DIV_ERROR, sym);
        } else if prim == PRIM_INDEX_ERROR {
            state.13.set_sym(SEED_SYM_INDEX_ERROR, sym);
        }
        let declared = insert_decl(state, scope, name, sym, NS_TYPE, 2, (file, start, end));
        item_set_sym(state.1, item, sym);
        let sub = alloc_scope(state.4, state.5, state.6, state.7, scope, prefix, 1);
        node_set_e(state.1, sym, sub);
        state.8.push((item, sub));
        enter_type_params(state.1, state.2, state.4, sub, node_f(state.1, item));
        if declared == 1 {
            collect_variants(state, scope, sub, full, item);
        }
    } else if kind == ITEM_TRAIT {
        let name = node_d(state.1, item);
        let full = qualified_name(state.0, state.2, prefix, name);
        let sym = alloc_sym(state.1, SYM_TRAIT, full, item, scope, NONE);
        insert_decl(state, scope, name, sym, NS_TYPE, 2, (file, start, end));
        item_set_sym(state.1, item, sym);
        let sub = alloc_scope(state.4, state.5, state.6, state.7, scope, prefix, is_pub);
        node_set_e(state.1, sym, sub);
        state.8.push((item, sub));
        collect_trait_methods(state, scope, sub, full, item);
    } else if kind == ITEM_IMPL {
        item_set_sym(state.1, item, NONE);
    } else if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
        let fn_node = node_d(state.1, item);
        let name = node_a(state.1, fn_node);
        let full = qualified_name(state.0, state.2, prefix, name);
        let sym_kind = if kind == ITEM_FUN { SYM_FUN } else { SYM_NATIVE_FUN };
        let sym = alloc_sym(state.1, sym_kind, full, item, scope, NONE);
        insert_decl(state, scope, name, sym, NS_VALUE, 1, (file, start, end));
        item_set_sym(state.1, item, sym);
        if kind == ITEM_NATIVE_FUN {
            let row = native_fun_row_of(state.0, full);
            if row == usize::MAX {
                sym_set_native_op(state.1, sym, NAT_NONE);
                // A native outside the registry is a resolution error.
                push_error(
                    state.3,
                    &format!("unknown native function '{}'", name_text(state.0, full)),
                    file,
                    start,
                    end,
                );
            } else {
                sym_set_native_op(state.1, sym, native_fun_verb(row));
            }
        } else if full == intern(state.0, "main") {
            node_set_f(state.1, sym, SYM_FUN_MAIN);
        }
    } else if kind == ITEM_CONST {
        let name = node_d(state.1, item);
        let full = qualified_name(state.0, state.2, prefix, name);
        let sym = alloc_sym(state.1, SYM_CONST, full, item, scope, NONE);
        insert_decl(state, scope, name, sym, NS_VALUE, 3, (file, start, end));
        item_set_sym(state.1, item, sym);
    } else if kind == ITEM_NATIVE_TYPE {
        let name = node_d(state.1, item);
        let full = qualified_name(state.0, state.2, prefix, name);
        let sym = alloc_sym(state.1, SYM_TYPE, full, item, scope, NONE);
        insert_decl(state, scope, name, sym, NS_TYPE, 2, (file, start, end));
        item_set_sym(state.1, item, sym);
        let sub = alloc_scope(state.4, state.5, state.6, state.7, scope, prefix, 1);
        state.8.push((item, sub));
        enter_type_params(state.1, state.2, state.4, sub, node_e(state.1, item));
        // An unknown native type is a resolution error; a known one stores
        // its container role and layout kind on its own symbol row.
        let trow = native_type_row_of(state.0, full);
        if trow == usize::MAX {
            push_error(
                state.3,
                &format!("unknown native type '{}'", name_text(state.0, full)),
                file,
                start,
                end,
            );
        } else {
            sym_set_native_role(state.1, sym, native_type_role(trow));
            sym_set_native_layout(state.1, sym, native_type_layout(trow));
        }
    } else if kind == ITEM_USE {
        let segs = node_d(state.1, item);
        let last = list_last(state.2, segs);
        let alias = node_e(state.1, item);
        let entry_name = if alias != NONE { alias } else { last };
        push_entry(state.4, scope, entry_name, NONE, 0, item);
    }
}

fn collect_fields_casing(names: &[String], nodes: &mut [i64], lists: &[Vec<i64>], errors: &mut Vec<Diag>, item: i64, scope: i64) {
    let fields = node_e(nodes, item);
    let count = list_len(lists, fields);
    let mut idx = 0i64;
    while idx < count {
        let field = list_get(lists, fields, idx);
        report_casing(names, node_a(nodes, field), 1, errors, node_file(nodes, field), node_start(nodes, field), node_end(nodes, field));
        // Slot d records the declaring module scope; the typechecker
        // compares it against the accessing scope for field visibility.
        node_set_d(nodes, field, scope);
        idx += 1;
    }
}

fn enter_type_params(nodes: &mut Vec<i64>, lists: &mut [Vec<i64>], scopes: &mut [Vec<i64>], scope: i64, params: i64) {
    let count = list_len(lists, params);
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(lists, params, idx);
        if node_tag(nodes, param) == NODE_TY && node_a(nodes, param) == TY_PARAM {
            let name = node_b(nodes, param);
            let sym = alloc_sym(nodes, SYM_TYPE, name, NONE, scope, NONE);
            push_entry(scopes, scope, name, sym, NS_TYPE, NONE);
        }
        idx += 1;
    }
}

fn collect_variants(state: &mut State, hoist_scope: i64, sub: i64, enum_full: i64, item: i64) {
    let variants = node_e(state.1, item);
    let count = list_len(state.2, variants);
    let prim = prim_kind_of(state.0, enum_full);
    let mut idx = 0i64;
    while idx < count {
        let variant = list_get(state.2, variants, idx);
        let var_name = node_a(state.1, variant);
        report_casing(state.0, var_name, 2, state.3, node_file(state.1, variant), node_start(state.1, variant), node_end(state.1, variant));
        let single = single_name_list(state.2, enum_full);
        let full = qualified_name(state.0, state.2, single, var_name);
        let sym = alloc_sym(state.1, SYM_VARIANT, full, variant, hoist_scope, NONE);
        variant_set_sym(state.1, variant, sym);
        // Slot e records the parent enum item so variant visibility can
        // inherit the enum's `pub` flag without re-walking the module.
        node_set_e(state.1, variant, item);
        // A reached variant pulls its enclosing enum into the fixpoint:
        // the edge (variant, enum) makes construction reach the type.
        let enum_sym = item_sym_of(state.1, item);
        if enum_sym != NONE
            && enum_sym != sym
            && !state.12.iter().any(|edge| edge.0 == sym && edge.1 == enum_sym)
        {
            state.12.push((sym, enum_sym));
        }
        push_entry(state.4, sub, var_name, sym, NS_VALUE, NONE);
        insert_hoisted(state, hoist_scope, var_name, sym, variant);
        if let Some(slot) = seed_variant_slot(prim, idx) {
            state.13.set_sym(slot, sym);
        }
        seed_protocol_variant(state, enum_full, var_name, sym);
        idx += 1;
    }
}

// Maps a variant of one of the seven sealed native module enums to its
// symbol slot in the Seeds table.
fn seed_protocol_variant(state: &mut State, enum_full: i64, var_name: i64, sym: i64) {
    let is_protocol_enum = enum_full == intern(state.0, "Memory.Error")
        || enum_full == intern(state.0, "Collections.Error")
        || enum_full == intern(state.0, "Terminal.Error")
        || enum_full == intern(state.0, "File.Mode")
        || enum_full == intern(state.0, "File.Error")
        || enum_full == intern(state.0, "Net.Error")
        || enum_full == intern(state.0, "Process.Error");
    if !is_protocol_enum {
        return;
    }
    if let Some(slot) = protocol_variant_slot(state.13, var_name) {
        state.13.set_sym(slot, sym);
    }
}

// The seeded symbol slot a protocol variant name maps to, or none for a
// name no protocol enum declares.
fn protocol_variant_slot(seeds: &Seeds, var_name: i64) -> Option<usize> {
    if var_name == seeds.name(SEED_NAME_ALLOC_FAILED) {
        Some(SEED_SYM_ALLOC_FAILED)
    } else if var_name == seeds.name(SEED_NAME_ACCESS_OOB) {
        Some(SEED_SYM_ACCESS_OOB)
    } else if var_name == seeds.name(SEED_NAME_INDEX_OOB) {
        // `Collections.Error` declares its own `IndexOutOfBounds` in a slot
        // of its own, separate from the primitive `IndexError` variant.
        Some(SEED_SYM_COLLECTIONS_INDEX_OOB)
    } else if var_name == seeds.name(SEED_NAME_KEY_NOT_FOUND) {
        Some(SEED_SYM_KEY_NOT_FOUND)
    } else if var_name == seeds.name(SEED_NAME_INVALID_UTF8) {
        Some(SEED_SYM_INVALID_UTF8)
    } else if var_name == seeds.name(SEED_NAME_EXIT_DIAG) {
        Some(SEED_SYM_EXIT_DIAG)
    } else if var_name == seeds.name(SEED_NAME_SYSTEM_FAULT) {
        Some(SEED_SYM_SYSTEM_FAULT)
    } else if var_name == seeds.name(SEED_NAME_READ_ONLY) {
        Some(SEED_SYM_READ_ONLY)
    } else if var_name == seeds.name(SEED_NAME_WRITE_TRUNCATE) {
        Some(SEED_SYM_WRITE_TRUNCATE)
    } else if var_name == seeds.name(SEED_NAME_END_OF_INPUT) {
        Some(SEED_SYM_END_OF_INPUT)
    } else if var_name == seeds.name(SEED_NAME_READ_FAILED) {
        Some(SEED_SYM_READ_FAILED)
    } else {
        None
    }
}

// The seeded symbol slot for a builtin primitive enum's variant at
// declaration index `idx`, or none when the enum is not a builtin primitive.
fn seed_variant_slot(prim: i64, idx: i64) -> Option<usize> {
    if prim == PRIM_RESULT {
        if idx == 0 {
            Some(SEED_SYM_OK)
        } else if idx == 1 {
            Some(SEED_SYM_ERR)
        } else {
            None
        }
    } else if prim == PRIM_OPTION {
        if idx == 0 {
            Some(SEED_SYM_SOME)
        } else if idx == 1 {
            Some(SEED_SYM_NONE)
        } else {
            None
        }
    } else if prim == PRIM_DIV_ERROR {
        if idx == 0 {
            Some(SEED_SYM_DIV_BY_ZERO)
        } else {
            None
        }
    } else if prim == PRIM_INDEX_ERROR {
        if idx == 0 {
            Some(SEED_SYM_INDEX_OOB)
        } else {
            None
        }
    } else {
        None
    }
}

fn insert_hoisted(state: &mut State, scope: i64, name: i64, sym: i64, decl: i64) {
    let existing = scope_lookup(state.4, scope, name, NS_VALUE);
    if existing.0 == NONE {
        push_entry(state.4, scope, name, sym, NS_VALUE, NONE);
        return;
    }
    let existing_decl = sym_decl_of(state.1, existing.0);
    let incoming_decl = sym_decl_of(state.1, sym);
    let existing_builtin = existing_decl == NONE || node_file(state.1, existing_decl) == NO_FILE;
    let incoming_builtin = incoming_decl == NONE || node_file(state.1, incoming_decl) == NO_FILE;
    if existing.1 != NONE {
        push_error(state.3, &format!("duplicate symbol '{}'", name_text(state.0, name)), node_file(state.1, decl), node_start(state.1, decl), node_end(state.1, decl));
        return;
    }
    if existing_builtin && incoming_builtin {
        return;
    }
    if existing_builtin || incoming_builtin {
        let user_decl = if !existing_builtin { existing_decl } else { incoming_decl };
        push_error(state.3, &format!("cannot redeclare builtin '{}'", name_text(state.0, name)), node_file(state.1, user_decl), node_start(state.1, user_decl), node_end(state.1, user_decl));
        return;
    }
    push_error(state.3, &format!("duplicate symbol '{}'", name_text(state.0, name)), node_file(state.1, decl), node_start(state.1, decl), node_end(state.1, decl));
    if existing_decl != NONE && node_file(state.1, existing_decl) != NO_FILE {
        push_note_for_last(
            state.3,
            state.10,
            "first declared here",
            node_file(state.1, existing_decl),
            node_start(state.1, existing_decl),
            node_end(state.1, existing_decl),
            NOTE_CONTEXT,
        );
    }
}

fn collect_trait_methods(state: &mut State, scope: i64, sub: i64, trait_full: i64, item: i64) {
    let methods = node_e(state.1, item);
    let count = list_len(state.2, methods);
    let mut idx = 0i64;
    while idx < count {
        let fn_node = list_get(state.2, methods, idx);
        let name = node_a(state.1, fn_node);
        report_casing(state.0, name, 1, state.3, node_file(state.1, fn_node), node_start(state.1, fn_node), node_end(state.1, fn_node));
        let single = single_name_list(state.2, trait_full);
        let full = qualified_name(state.0, state.2, single, name);
        let sym = alloc_sym(state.1, SYM_TRAIT_METHOD, full, fn_node, scope, NONE);
        // Slot e records the parent trait item so trait-method lookups can
        // read the owning trait directly instead of matching scopes.
        node_set_e(state.1, sym, item);
        push_entry(state.4, sub, name, sym, NS_VALUE, NONE);
        idx += 1;
    }
}

fn seed_builtins(state: &mut State, root_scope: i64, root: i64) {
    let seed_names: &[(&str, usize)] = &[
        ("Ok", SEED_NAME_OK),
        ("Err", SEED_NAME_ERR),
        ("Some", SEED_NAME_SOME),
        ("None", SEED_NAME_NONE),
        ("DivByZero", SEED_NAME_DIV_BY_ZERO),
        ("AllocationFailed", SEED_NAME_ALLOC_FAILED),
        ("AccessOutOfBounds", SEED_NAME_ACCESS_OOB),
        ("IndexOutOfBounds", SEED_NAME_INDEX_OOB),
        ("KeyNotFound", SEED_NAME_KEY_NOT_FOUND),
        ("InvalidUtf8", SEED_NAME_INVALID_UTF8),
        ("ExitDiagnostic", SEED_NAME_EXIT_DIAG),
        ("SystemFault", SEED_NAME_SYSTEM_FAULT),
        ("ReadOnly", SEED_NAME_READ_ONLY),
        ("WriteTruncate", SEED_NAME_WRITE_TRUNCATE),
        ("EndOfInput", SEED_NAME_END_OF_INPUT),
        ("ReadFailed", SEED_NAME_READ_FAILED),
        ("Self", SEED_NAME_SELF),
    ];
    let mut nidx = 0usize;
    while nidx < seed_names.len() {
        let (text, slot) = match seed_names.get(nidx) {
            Some(pair) => *pair,
            None => break,
        };
        let name_id = intern(state.0, text);
        state.13.set_name(slot, name_id);
        nidx += 1;
    }
    let ints = builtin_int_names(state.0);
    let mut idx = 0usize;
    while idx < ints.len() {
        let name_id = match ints.get(idx) {
            Some(id) => *id,
            None => break,
        };
        let sym = seed_builtin_type(state, root_scope, name_id);
        state.13.set_sym(SEED_SYM_I8 + idx, sym);
        seed_int_from(state, node_e(state.1, sym), name_id);
        idx += 1;
    }
    let bool_name = intern(state.0, "Bool");
    let bool_sym = seed_builtin_type(state, root_scope, bool_name);
    state.13.set_sym(SEED_SYM_BOOL, bool_sym);
    seed_primitive(state, root, "Unit", &[], &[("Unit", &[])]);
    seed_primitive(state, root, "Result", &["T", "E"], &[("Ok", &["T"]), ("Err", &["E"])]);
    seed_primitive(state, root, "Option", &["T"], &[("Some", &["T"]), ("None", &[])]);
    seed_primitive(state, root, "DivError", &[], &[("DivByZero", &[])]);
    seed_primitive(state, root, "IndexError", &[], &[("IndexOutOfBounds", &["Usize", "Usize"])]);
}

fn seed_primitive(state: &mut State, root: i64, name: &str, params: &[&str], variants: &[(&str, &[&str])]) {
    let params_list = alloc_list(state.2);
    let mut p_idx = 0usize;
    while p_idx < params.len() {
        let param_name = match params.get(p_idx) {
            Some(text) => *text,
            None => break,
        };
        let param_id = intern(state.0, param_name);
        list_push(state.2, params_list, seed_ty_node(state.1, TY_PARAM, param_id));
        p_idx += 1;
    }
    let variants_list = alloc_list(state.2);
    let mut v_idx = 0usize;
    while v_idx < variants.len() {
        let (variant_name, payloads) = match variants.get(v_idx) {
            Some(pair) => *pair,
            None => break,
        };
        let payload_list = alloc_list(state.2);
        let mut pl_idx = 0usize;
        while pl_idx < payloads.len() {
            let payload_name = match payloads.get(pl_idx) {
                Some(text) => *text,
                None => break,
            };
            let payload_id = intern(state.0, payload_name);
            list_push(state.2, payload_list, seed_ty_node(state.1, TY_NAMED, payload_id));
            pl_idx += 1;
        }
        let var_id = intern(state.0, variant_name);
        let variant = alloc_node(state.1, &[NODE_VARIANT, NO_FILE, 0, 0, var_id, payload_list, 1]);
        list_push(state.2, variants_list, variant);
        v_idx += 1;
    }
    let name_id = intern(state.0, name);
    let item = alloc_node(
        state.1,
        &[NODE_ITEM, NO_FILE, 0, 0, ITEM_ENUM, 1, NONE, name_id, variants_list, params_list],
    );
    list_push(state.2, root, item);
}

fn seed_ty_node(nodes: &mut Vec<i64>, kind: i64, name: i64) -> i64 {
    alloc_node(nodes, &[NODE_TY, NO_FILE, 0, 0, kind, name, NONE])
}

fn seed_int_from(state: &mut State, scope: i64, name: i64) {
    let prefix = alloc_list(state.2);
    list_push(state.2, prefix, name);
    let from_name = intern(state.0, "from");
    let method = qualified_name(state.0, state.2, prefix, from_name);
    let row = native_fun_row_of(state.0, method);
    if row == usize::MAX {
        // A name with no registry row has no int `.from` surface; the
        // ordinary unknown-function diagnostic applies.
        return;
    }
    let from = alloc_sym(state.1, SYM_NATIVE_FUN, method, NONE, scope, NONE);
    sym_set_native_op(state.1, from, native_fun_verb(row));
    sym_set_native_mode(state.1, from, native_fun_mode(row));
    push_entry(state.4, scope, from_name, from, NS_VALUE, NONE);
}

// The native registry: one seeded row per native function
// { qualified name, declared mode, verb } and per native type { name, layout kind, container role }.

const NATIVE_FUN_ROWS: &[(&str, i64, i64)] = &[
    ("I8.from", NAT_MODE_EFFECT, NAT_INT_FROM),
    ("I16.from", NAT_MODE_EFFECT, NAT_INT_FROM),
    ("I32.from", NAT_MODE_EFFECT, NAT_INT_FROM),
    ("I64.from", NAT_MODE_EFFECT, NAT_INT_FROM),
    ("Isize.from", NAT_MODE_EFFECT, NAT_INT_FROM),
    ("U8.from", NAT_MODE_EFFECT, NAT_INT_FROM),
    ("U16.from", NAT_MODE_EFFECT, NAT_INT_FROM),
    ("U32.from", NAT_MODE_EFFECT, NAT_INT_FROM),
    ("U64.from", NAT_MODE_EFFECT, NAT_INT_FROM),
    ("Usize.from", NAT_MODE_EFFECT, NAT_INT_FROM),
    ("Slice.len", NAT_MODE_EFFECT, NAT_SLICE_LEN),
    ("Memory.allocate", NAT_MODE_CREATE, NAT_MEM_ALLOCATE),
    ("Memory.deallocate", NAT_MODE_CONSUME, NAT_MEM_DEALLOCATE),
    ("Memory.write_u8", NAT_MODE_BORROW, NAT_MEM_WRITE_U8),
    ("Memory.read_u8", NAT_MODE_BORROW, NAT_MEM_READ_U8),
    ("Memory.block_view", NAT_MODE_VIEW, NAT_SLICE_VIEW),
    ("Collections.vec_new", NAT_MODE_CREATE, NAT_VEC_NEW),
    ("Collections.vec_push", NAT_MODE_MUTATE, NAT_VEC_PUSH),
    ("Collections.vec_pop", NAT_MODE_EXTRACT, NAT_VEC_POP),
    ("Collections.vec_free", NAT_MODE_CONSUME, NAT_VEC_FREE),
    ("Collections.vec_view", NAT_MODE_VIEW, NAT_SLICE_VIEW),
    ("Collections.string_from_slice", NAT_MODE_CREATE, NAT_STRING_FROM_SLICE),
    ("Collections.string_len", NAT_MODE_BORROW, NAT_STRING_LEN),
    ("Collections.string_free", NAT_MODE_CONSUME, NAT_STRING_FREE),
    ("Collections.string_view", NAT_MODE_VIEW, NAT_SLICE_VIEW),
    ("Collections.hash_map_new", NAT_MODE_CREATE, NAT_HASH_MAP_NEW),
    ("Collections.hash_map_insert", NAT_MODE_MUTATE, NAT_HASH_MAP_INSERT),
    ("Collections.hash_map_get", NAT_MODE_BORROW, NAT_HASH_MAP_GET),
    ("Collections.hash_map_free", NAT_MODE_CONSUME, NAT_HASH_MAP_FREE),
    ("Collections.hash_map_remove", NAT_MODE_EXTRACT, NAT_HASH_MAP_REMOVE),
    ("Runtime.self_check", NAT_MODE_EFFECT, NAT_SELF_CHECK),
    ("Runtime.args", NAT_MODE_EFFECT, NAT_RUNTIME_ARGS),
    ("Terminal.print", NAT_MODE_BORROW, NAT_TERM_PRINT),
    ("Terminal.print_line", NAT_MODE_BORROW, NAT_TERM_PRINT_LINE),
    ("Terminal.eprint", NAT_MODE_BORROW, NAT_TERM_EPRINT),
    ("Terminal.read_line", NAT_MODE_CREATE, NAT_TERM_READ_LINE),
    ("File.open", NAT_MODE_CREATE, NAT_FILE_OPEN),
    ("File.read", NAT_MODE_BORROW, NAT_FILE_READ),
    ("File.write", NAT_MODE_BORROW, NAT_FILE_WRITE),
    ("File.close", NAT_MODE_CONSUME, NAT_FILE_CLOSE),
    ("Net.socket", NAT_MODE_CREATE, NAT_NET_SOCKET),
    ("Net.bind", NAT_MODE_BORROW, NAT_NET_BIND),
    ("Net.listen", NAT_MODE_BORROW, NAT_NET_LISTEN),
    ("Net.accept", NAT_MODE_CREATE, NAT_NET_ACCEPT),
    ("Net.send", NAT_MODE_BORROW, NAT_NET_SEND),
    ("Net.recv", NAT_MODE_BORROW, NAT_NET_RECV),
    ("Net.close", NAT_MODE_CONSUME, NAT_NET_CLOSE),
    ("Process.spawn", NAT_MODE_CREATE, NAT_PROCESS_SPAWN),
    ("Process.wait", NAT_MODE_CONSUME, NAT_PROCESS_WAIT),
];

// The modes each verb's codegen supports; a derived mode outside its
// verb's set is an internal error.
const NATIVE_VERB_MODES: &[(i64, &[i64])] = &[
    (NAT_INT_FROM, &[NAT_MODE_EFFECT]),
    (NAT_SLICE_LEN, &[NAT_MODE_EFFECT]),
    (NAT_MEM_ALLOCATE, &[NAT_MODE_CREATE]),
    (NAT_MEM_DEALLOCATE, &[NAT_MODE_CONSUME]),
    (NAT_MEM_WRITE_U8, &[NAT_MODE_BORROW]),
    (NAT_MEM_READ_U8, &[NAT_MODE_BORROW]),
    (NAT_VEC_NEW, &[NAT_MODE_CREATE]),
    (NAT_VEC_PUSH, &[NAT_MODE_MUTATE]),
    (NAT_VEC_POP, &[NAT_MODE_EXTRACT]),
    (NAT_VEC_FREE, &[NAT_MODE_CONSUME]),
    (NAT_SLICE_VIEW, &[NAT_MODE_VIEW]),
    (NAT_STRING_FROM_SLICE, &[NAT_MODE_CREATE]),
    (NAT_STRING_LEN, &[NAT_MODE_BORROW]),
    (NAT_STRING_FREE, &[NAT_MODE_CONSUME]),
    (NAT_HASH_MAP_NEW, &[NAT_MODE_CREATE]),
    (NAT_HASH_MAP_INSERT, &[NAT_MODE_MUTATE]),
    (NAT_HASH_MAP_GET, &[NAT_MODE_BORROW]),
    (NAT_HASH_MAP_FREE, &[NAT_MODE_CONSUME]),
    (NAT_HASH_MAP_REMOVE, &[NAT_MODE_EXTRACT]),
    (NAT_SELF_CHECK, &[NAT_MODE_EFFECT]),
    (NAT_TERM_PRINT, &[NAT_MODE_BORROW, NAT_MODE_EFFECT]),
    (NAT_TERM_PRINT_LINE, &[NAT_MODE_BORROW, NAT_MODE_EFFECT]),
    (NAT_TERM_EPRINT, &[NAT_MODE_BORROW, NAT_MODE_EFFECT]),
    (NAT_TERM_READ_LINE, &[NAT_MODE_CREATE]),
    (NAT_FILE_OPEN, &[NAT_MODE_CREATE]),
    (NAT_FILE_READ, &[NAT_MODE_BORROW, NAT_MODE_TRANSFER]),
    (NAT_FILE_WRITE, &[NAT_MODE_BORROW, NAT_MODE_TRANSFER]),
    (NAT_FILE_CLOSE, &[NAT_MODE_CONSUME]),
    (NAT_RUNTIME_ARGS, &[NAT_MODE_EFFECT]),
    (NAT_NET_SOCKET, &[NAT_MODE_CREATE]),
    (NAT_NET_BIND, &[NAT_MODE_BORROW]),
    (NAT_NET_LISTEN, &[NAT_MODE_BORROW]),
    (NAT_NET_ACCEPT, &[NAT_MODE_CREATE]),
    (NAT_NET_SEND, &[NAT_MODE_BORROW, NAT_MODE_TRANSFER]),
    (NAT_NET_RECV, &[NAT_MODE_BORROW, NAT_MODE_TRANSFER]),
    (NAT_NET_CLOSE, &[NAT_MODE_CONSUME]),
    (NAT_PROCESS_SPAWN, &[NAT_MODE_CREATE]),
    (NAT_PROCESS_WAIT, &[NAT_MODE_CONSUME]),
];

// (qualified name, layout kind, container role); the role is the declared
// fact borrow and typecheck read.
const NATIVE_TYPE_ROWS: &[(&str, i64, i64)] = &[
    ("Memory.Block", NATIVE_LAYOUT_PAIR, 0),
    ("Collections.Vec", NATIVE_LAYOUT_TRIPLE, 1),
    ("Collections.String", NATIVE_LAYOUT_PAIR, 0),
    ("Collections.HashMap", NATIVE_LAYOUT_TRIPLE, 1),
    ("File.Handle", NATIVE_LAYOUT_SCALAR, 0),
    ("Net.Socket", NATIVE_LAYOUT_SCALAR, 0),
    ("Process.Child", NATIVE_LAYOUT_SCALAR, 0),
];

fn native_fun_row_of(names: &[String], full: i64) -> usize {
    let mut idx = 0usize;
    while idx < NATIVE_FUN_ROWS.len() {
        match NATIVE_FUN_ROWS.get(idx) {
            Some(row) => {
                if name_is(names, full, row.0) {
                    return idx;
                }
            }
            None => break,
        }
        idx += 1;
    }
    usize::MAX
}

fn native_fun_verb(row: usize) -> i64 {
    NATIVE_FUN_ROWS[row].2
}

fn native_fun_mode(row: usize) -> i64 {
    NATIVE_FUN_ROWS[row].1
}

fn native_type_row_of(names: &[String], full: i64) -> usize {
    let mut idx = 0usize;
    while idx < NATIVE_TYPE_ROWS.len() {
        match NATIVE_TYPE_ROWS.get(idx) {
            Some(row) => {
                if name_is(names, full, row.0) {
                    return idx;
                }
            }
            None => break,
        }
        idx += 1;
    }
    usize::MAX
}

fn native_type_role(row: usize) -> i64 {
    NATIVE_TYPE_ROWS[row].2
}

fn native_type_layout(row: usize) -> i64 {
    NATIVE_TYPE_ROWS[row].1
}

fn verb_supports_mode(verb: i64, mode: i64) -> bool {
    let mut idx = 0usize;
    while idx < NATIVE_VERB_MODES.len() {
        match NATIVE_VERB_MODES.get(idx) {
            Some(row) => {
                if row.0 == verb {
                    let mut m = 0usize;
                    while m < row.1.len() {
                        if row.1[m] == mode {
                            return true;
                        }
                        m += 1;
                    }
                    return false;
                }
            }
            None => break,
        }
        idx += 1;
    }
    false
}

// The native subsystem a verb belongs to, so the resolver can check the
// declared surface against the target's capabilities before typechecking.
fn native_subsystem_of(verb: i64) -> NativeSubsystem {
    if verb == NAT_MEM_ALLOCATE
        || verb == NAT_MEM_DEALLOCATE
        || verb == NAT_MEM_WRITE_U8
        || verb == NAT_MEM_READ_U8
    {
        NativeSubsystem::Memory
    } else if verb == NAT_FILE_OPEN
        || verb == NAT_FILE_READ
        || verb == NAT_FILE_WRITE
        || verb == NAT_FILE_CLOSE
    {
        NativeSubsystem::File
    } else if verb == NAT_NET_SOCKET
        || verb == NAT_NET_BIND
        || verb == NAT_NET_LISTEN
        || verb == NAT_NET_ACCEPT
        || verb == NAT_NET_SEND
        || verb == NAT_NET_RECV
        || verb == NAT_NET_CLOSE
    {
        NativeSubsystem::Network
    } else if verb == NAT_PROCESS_SPAWN || verb == NAT_PROCESS_WAIT {
        NativeSubsystem::Process
    } else {
        NativeSubsystem::Core
    }
}

// Derives an ownership mode from a native function's signature and
// attaches it to the symbol's natfact row.
fn classify_native_modes(state: &mut State) {
    let count = state.1.len() as i64 / NODE_STRIDE;
    let mut idx = 0i64;
    while idx < count {
        if node_tag(state.1, idx) == NODE_SYM && sym_kind_of(state.1, idx) == SYM_NATIVE_FUN {
            if sym_native_op(state.1, idx) == NAT_NONE {
                idx += 1;
                continue;
            }
            let decl = sym_decl_of(state.1, idx);
            let derived;
            let span;
            if decl == NONE || node_tag(state.1, decl) != NODE_ITEM || node_a(state.1, decl) != ITEM_NATIVE_FUN {
                // Seeded surface (the int `.from` methods): the table's
                // declared mode is the mode.
                derived = sym_native_mode(state.1, idx);
                span = (NONE, NONE, NONE);
            } else {
                span = (node_file(state.1, decl), node_start(state.1, decl), node_end(state.1, decl));
                let fn_node = node_d(state.1, decl);
                derived = if fn_node == NONE {
                    NAT_MODE_NONE
                } else {
                    native_mode_of_signature(state.1, state.2, fn_node)
                };
                sym_set_native_mode(state.1, idx, derived);
            }
            let verb = sym_native_op(state.1, idx);
            if verb != NAT_NONE && span.0 != NONE {
                let subsystem = native_subsystem_of(verb);
                if !state.14.supports_subsystem(subsystem) {
                    push_error_kind(
                        state.3,
                        &format!(
                            "native operation '{}' requires {}, which target '{}' does not support",
                            name_text(state.0, sym_name_of(state.1, idx)),
                            subsystem.name(),
                            state.14.os.name()
                        ),
                        span.0,
                        span.1,
                        span.2,
                        DiagKind::Resolve,
                    );
                }
            }
            if derived != NAT_MODE_NONE && verb != NAT_NONE && !verb_supports_mode(verb, derived) && span.0 != NONE {
                push_error(
                    state.3,
                    &format!(
                        "internal error: native function '{}' has mode {}, which its verb does not support",
                        name_text(state.0, sym_name_of(state.1, idx)),
                        mode_name_of(derived)
                    ),
                    span.0,
                    span.1,
                    span.2,
                );
            } else if derived == NAT_MODE_NONE && span.0 != NONE {
                push_error(
                    state.3,
                    &format!(
                        "internal error: native function '{}' has a signature matching no ownership mode",
                        name_text(state.0, sym_name_of(state.1, idx))
                    ),
                    span.0,
                    span.1,
                    span.2,
                );
            }
        }
        idx += 1;
    }
}

fn mode_name_of(mode: i64) -> String {
    if mode == NAT_MODE_VIEW {
        String::from("view")
    } else if mode == NAT_MODE_EXTRACT {
        String::from("extract")
    } else if mode == NAT_MODE_TRANSFER {
        String::from("transfer")
    } else if mode == NAT_MODE_CREATE {
        String::from("create")
    } else if mode == NAT_MODE_CONSUME {
        String::from("consume")
    } else if mode == NAT_MODE_MUTATE {
        String::from("mutate")
    } else if mode == NAT_MODE_BORROW {
        String::from("borrow")
    } else {
        String::from("effect")
    }
}

// The signature classifier: reads the declared signature (parameter and
// return type nodes with their resolved symbols).
fn native_mode_of_signature(nodes: &[i64], lists: &[Vec<i64>], fn_node: i64) -> i64 {
    let params = node_c(nodes, fn_node);
    let ret = node_d(nodes, fn_node);
    let pcount = list_len(lists, params);
    let first_ty = if pcount > 0 {
        let first = list_first(lists, params);
        if first == NONE {
            NONE
        } else {
            node_b(nodes, first)
        }
    } else {
        NONE
    };

    // 1. view — first parameter `&H` (opaque handle), returns `&[T]`.
    if first_ty != NONE
        && node_a(nodes, first_ty) == TY_REF
        && type_is_handle_value(nodes, node_b(nodes, first_ty))
        && type_is_slice_ref(nodes, ret)
    {
        return NAT_MODE_VIEW;
    }

    // 2. extract — first parameter `&mut C` (declared container type),
    //    returns the container's element type (Result-wrapped or direct).
    if first_ty != NONE && node_a(nodes, first_ty) == TY_REF_MUT {
        let inner = node_b(nodes, first_ty);
        if type_is_container(nodes, inner) {
            let elem = container_element_of(nodes, lists, inner);
            if elem != NONE && ty_nodes_equal(nodes, lists, result_payload_of(nodes, lists, ret), elem) {
                return NAT_MODE_EXTRACT;
            }
        }
    }

    // 3. transfer — a handle (by value or `&mut`) alongside a slice.  A
    //    `&mut [U8]` buffer is caller memory, not a handle resource.
    if pcount > 0 {
        let mut io_resource = false;
        let mut has_slice = false;
        let mut idx = 0i64;
        while idx < pcount {
            let pty = node_b(nodes, list_get(lists, params, idx));
            let resource = type_is_handle_value(nodes, pty)
                || (node_a(nodes, pty) == TY_REF_MUT && type_is_handle_value(nodes, node_b(nodes, pty)));
            if resource {
                io_resource = true;
            }
            if type_is_slice(nodes, pty) {
                has_slice = true;
            }
            idx += 1;
        }
        if io_resource && has_slice {
            return NAT_MODE_TRANSFER;
        }
    }

    // 4. create — returns a handle by value, consumes no handle by value.
    if type_is_handle_value(nodes, result_payload_of(nodes, lists, ret))
        && !signature_has_handle_value_param(nodes, lists, params, pcount)
    {
        return NAT_MODE_CREATE;
    }

    // 5. consume — takes a handle by value; returns Unit or Result.
    if first_ty != NONE
        && type_is_handle_value(nodes, first_ty)
        && (type_is_unit(nodes, ret) || type_is_result(nodes, ret))
    {
        return NAT_MODE_CONSUME;
    }

    // 6. mutate — takes `&mut H` (opaque handle); returns a non-slice.
    if first_ty != NONE
        && node_a(nodes, first_ty) == TY_REF_MUT
        && type_is_handle_value(nodes, node_b(nodes, first_ty))
        && !type_is_slice_ref(nodes, ret)
    {
        return NAT_MODE_MUTATE;
    }

    // 7. borrow — takes `&H` and returns a value that is not provably a
    //    fresh handle (a concrete native type by value).
    if first_ty != NONE
        && node_a(nodes, first_ty) == TY_REF
        && type_is_handle_value(nodes, node_b(nodes, first_ty))
        && !type_is_slice_ref(nodes, ret)
        && !type_is_handle_value(nodes, result_payload_of(nodes, lists, ret))
    {
        return NAT_MODE_BORROW;
    }

    // 8. effect — no native handle anywhere in the signature.
    if !signature_mentions_handle(nodes, lists, params, pcount, ret) {
        return NAT_MODE_EFFECT;
    }

    NAT_MODE_NONE
}

// The resolved symbol of a type node naming a declared native type; a
// type parameter's declaration is not a `nat type` item, so NONE.
fn ty_native_sym(nodes: &[i64], ty: i64) -> i64 {
    let sym = ty_sym_of(nodes, ty);
    if sym == NONE || node_tag(nodes, sym) != NODE_SYM || node_a(nodes, sym) != SYM_TYPE {
        return NONE;
    }
    let decl = sym_decl_of(nodes, sym);
    if decl == NONE || node_tag(nodes, decl) != NODE_ITEM || node_a(nodes, decl) != ITEM_NATIVE_TYPE {
        return NONE;
    }
    sym
}

// A by-value opaque handle: the type itself is a declared native type,
// with no reference layer.  `Vec(T)` counts; `&Vec(T)` does not.
fn type_is_handle_value(nodes: &[i64], ty: i64) -> bool {
    let kind = node_a(nodes, ty);
    (kind == TY_NAMED || kind == TY_GENERIC || kind == TY_PATH) && ty_native_sym(nodes, ty) != NONE
}

// A slice type, behind any reference layer.
fn type_is_slice(nodes: &[i64], ty: i64) -> bool {
    let kind = node_a(nodes, ty);
    if kind == TY_REF || kind == TY_REF_MUT {
        return node_a(nodes, node_b(nodes, ty)) == TY_SLICE;
    }
    kind == TY_SLICE
}

// A reference to a slice, as a `&[T]` return is written.
fn type_is_slice_ref(nodes: &[i64], ty: i64) -> bool {
    let kind = node_a(nodes, ty);
    (kind == TY_REF || kind == TY_REF_MUT) && node_a(nodes, node_b(nodes, ty)) == TY_SLICE
}

// A declared container: a native type whose registry row says so.
fn type_is_container(nodes: &[i64], ty: i64) -> bool {
    let sym = ty_native_sym(nodes, ty);
    sym != NONE && nattype_is_container(nodes, sym) == 1
}

// The element type of a container type application: its last type
// argument; NONE for a container used without arguments.
fn container_element_of(nodes: &[i64], lists: &[Vec<i64>], ty: i64) -> i64 {
    let args = node_c(nodes, ty);
    let count = list_len(lists, args);
    if count == 0 {
        NONE
    } else {
        list_get(lists, args, count - 1)
    }
}

// The payload of a Result-wrapped return, or the return type itself.
// The wrapper is recognized by the attached primitive kind, never by name.
fn result_payload_of(nodes: &[i64], lists: &[Vec<i64>], ty: i64) -> i64 {
    if type_is_result(nodes, ty) {
        let args = node_c(nodes, ty);
        let first = list_first(lists, args);
        if first != NONE {
            return first;
        }
    }
    ty
}

fn type_is_result(nodes: &[i64], ty: i64) -> bool {
    // `Result` is a seeded enum symbol, and only seeded primitives carry
    // a PRIM_* kind, so the kind check alone identifies the wrapper.
    let sym = ty_sym_of(nodes, ty);
    sym != NONE && node_tag(nodes, sym) == NODE_SYM && node_f(nodes, sym) == PRIM_RESULT
}

fn type_is_unit(nodes: &[i64], ty: i64) -> bool {
    let sym = ty_sym_of(nodes, ty);
    sym != NONE && node_tag(nodes, sym) == NODE_SYM && node_f(nodes, sym) == PRIM_UNIT
}

// Whether the signature mentions a native handle anywhere: any parameter
// or the return type, through reference layers.
fn signature_mentions_handle(nodes: &[i64], lists: &[Vec<i64>], params: i64, pcount: i64, ret: i64) -> bool {
    let mut idx = 0i64;
    while idx < pcount {
        if type_mentions_handle(nodes, node_b(nodes, list_get(lists, params, idx))) {
            return true;
        }
        idx += 1;
    }
    type_mentions_handle(nodes, ret)
}

fn signature_has_handle_value_param(nodes: &[i64], lists: &[Vec<i64>], params: i64, pcount: i64) -> bool {
    let mut idx = 0i64;
    while idx < pcount {
        if type_is_handle_value(nodes, node_b(nodes, list_get(lists, params, idx))) {
            return true;
        }
        idx += 1;
    }
    false
}

fn type_mentions_handle(nodes: &[i64], ty: i64) -> bool {
    let kind = node_a(nodes, ty);
    if kind == TY_REF || kind == TY_REF_MUT {
        return type_is_handle_value(nodes, node_b(nodes, ty));
    }
    type_is_handle_value(nodes, ty)
}

// Structural equality of two parse-level type nodes: same shape, same
// name ids at named leaves.
fn ty_nodes_equal(nodes: &[i64], lists: &[Vec<i64>], a: i64, b: i64) -> bool {
    if a == NONE || b == NONE || node_tag(nodes, a) != NODE_TY || node_tag(nodes, b) != NODE_TY {
        return false;
    }
    let ka = node_a(nodes, a);
    if ka != node_a(nodes, b) {
        return false;
    }
    if ka == TY_NAMED || ka == TY_PARAM {
        return node_b(nodes, a) == node_b(nodes, b);
    }
    if ka == TY_SELF {
        return true;
    }
    if ka == TY_REF || ka == TY_REF_MUT || ka == TY_SLICE {
        return ty_nodes_equal(nodes, lists, node_b(nodes, a), node_b(nodes, b));
    }
    if ka == TY_ARRAY {
        return node_c(nodes, a) == node_c(nodes, b)
            && ty_nodes_equal(nodes, lists, node_b(nodes, a), node_b(nodes, b));
    }
    if ka == TY_GENERIC || ka == TY_PATH {
        let sa = node_b(nodes, a);
        let sb = node_b(nodes, b);
        let scount = list_len(lists, sa);
        if scount != list_len(lists, sb) {
            return false;
        }
        let mut idx = 0i64;
        while idx < scount {
            if list_get(lists, sa, idx) != list_get(lists, sb, idx) {
                return false;
            }
            idx += 1;
        }
        let aa = node_c(nodes, a);
        let ab = node_c(nodes, b);
        let acount = list_len(lists, aa);
        if acount != list_len(lists, ab) {
            return false;
        }
        let mut ai = 0i64;
        while ai < acount {
            if !ty_nodes_equal(nodes, lists, list_get(lists, aa, ai), list_get(lists, ab, ai)) {
                return false;
            }
            ai += 1;
        }
        return true;
    }
    true
}

// Marks the container type each extract-mode native function's first
// parameter names as having an extraction surface.
fn link_extraction_surfaces(state: &mut State) {
    let count = state.1.len() as i64 / NODE_STRIDE;
    let mut idx = 0i64;
    while idx < count {
        if node_tag(state.1, idx) == NODE_SYM
            && node_a(state.1, idx) == SYM_NATIVE_FUN
            && sym_native_mode(state.1, idx) == NAT_MODE_EXTRACT
        {
            let decl = sym_decl_of(state.1, idx);
            if decl != NONE && node_tag(state.1, decl) == NODE_ITEM {
                let fn_node = node_d(state.1, decl);
                let first = list_first(state.2, node_c(state.1, fn_node));
                if first != NONE {
                    let cty_sym = container_type_sym(state.1, node_b(state.1, first));
                    if cty_sym != NONE {
                        let cty_decl = sym_decl_of(state.1, cty_sym);
                        node_set_f(state.1, cty_decl, idx);
                    }
                }
            }
        }
        idx += 1;
    }
}

// The resolved type symbol of a parameter type node, dereferencing any
// reference layers so `&mut Vec(T)` resolves to the Vec type symbol.
fn container_type_sym(nodes: &[i64], ty_node: i64) -> i64 {
    let mut node = ty_node;
    loop {
        let kind = node_a(nodes, node);
        if kind == TY_REF || kind == TY_REF_MUT {
            node = node_b(nodes, node);
            continue;
        }
        break;
    }
    node_e(nodes, node)
}

fn prim_kind_of(names: &mut Vec<String>, full: i64) -> i64 {
    if full == intern(names, "Unit") {
        PRIM_UNIT
    } else if full == intern(names, "Result") {
        PRIM_RESULT
    } else if full == intern(names, "Option") {
        PRIM_OPTION
    } else if full == intern(names, "DivError") {
        PRIM_DIV_ERROR
    } else if full == intern(names, "IndexError") {
        PRIM_INDEX_ERROR
    } else {
        PRIM_NONE
    }
}

fn builtin_int_names(names: &mut Vec<String>) -> Vec<i64> {
    vec![
        intern(names, "I8"),
        intern(names, "I16"),
        intern(names, "I32"),
        intern(names, "I64"),
        intern(names, "Isize"),
        intern(names, "U8"),
        intern(names, "U16"),
        intern(names, "U32"),
        intern(names, "U64"),
        intern(names, "Usize"),
    ]
}

fn seed_builtin_type(state: &mut State, root_scope: i64, name: i64) -> i64 {
    let prefix = alloc_list(state.2);
    list_push(state.2, prefix, name);
    let sub = alloc_scope(state.4, state.5, state.6, state.7, root_scope, prefix, 1);
    let sym = alloc_sym(state.1, SYM_TYPE, name, NONE, sub, sub);
    push_entry(state.4, root_scope, name, sym, NS_TYPE, NONE);
    sym
}

fn resolve_imports(state: &mut State, scope: i64, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        let item = list_get(state.2, list, idx);
        resolve_import(state, scope, item);
        // A module-local import resolves against the module's own scope,
        // not the scope its `mod` block sits in.
        if node_tag(state.1, item) == NODE_ITEM && node_a(state.1, item) == ITEM_MODULE {
            let children = node_e(state.1, item);
            resolve_imports(state, item_scope_of(state.8, item), children);
        }
        idx += 1;
    }
}

fn resolve_import(state: &mut State, scope: i64, item: i64) {
    if node_tag(state.1, item) != NODE_ITEM || node_a(state.1, item) != ITEM_USE {
        return;
    }
    let segs = node_d(state.1, item);
    let file = node_file(state.1, item);
    let start = node_start(state.1, item);
    let end = node_end(state.1, item);
    let sym = resolve_path(state, scope, segs, NS_VALUE);
    if sym != NONE {
        let target_ns = sym_ns(state.1, sym);
        finish_import(state, scope, item, sym, target_ns, (file, start, end));
        return;
    }
    let type_sym = resolve_path(state, scope, segs, NS_TYPE);
    if type_sym == NONE {
        let path = join_segs(state.0, state.2, segs);
        push_error(state.3, &format!("cannot resolve import '{}'", path), file, start, end);
        return;
    }
    finish_import(state, scope, item, type_sym, NS_TYPE, (file, start, end));
}

fn finish_import(state: &mut State, scope: i64, item: i64, sym: i64, target_ns: i64, span: (i64, i64, i64)) {
    if !is_visible(state.5, state.6, state.1, scope, sym) {
        push_error_kind(state.3, &format!("cannot import private item '{}'", name_text(state.0, sym_name_of(state.1, sym))), span.0, span.1, span.2, DiagKind::PrivateAccess { sym });
        return;
    }
    let segs = node_d(state.1, item);
    let last = list_last(state.2, segs);
    let alias = node_e(state.1, item);
    let entry_name = if alias != NONE { alias } else { last };
    let casing = import_casing_of(state.1, sym);
    report_casing(state.0, entry_name, casing, state.3, span.0, span.1, span.2);
    let conflict = scope_lookup(state.4, scope, entry_name, target_ns);
    if conflict.0 != NONE && conflict.1 != item && conflict.0 != sym {
        push_error(state.3, &format!("import '{}' conflicts with another symbol", name_text(state.0, entry_name)), span.0, span.1, span.2);
        return;
    }
    // The `use` item's own symbol slot records what it resolved to, so
    // `check_unused` can tell a resolved import from one that never did.
    item_set_sym(state.1, item, sym);
    rewrite_import(state.4, scope, item, sym, target_ns);
}

// The casing an imported name must satisfy follows the symbol's own kind
// (function, constant, variant, type), not which namespace it resolves in.
fn import_casing_of(nodes: &[i64], sym: i64) -> i64 {
    let kind = sym_kind_of(nodes, sym);
    if kind == SYM_CONST {
        3
    } else if kind == SYM_FUN || kind == SYM_NATIVE_FUN || kind == SYM_TRAIT_METHOD || kind == SYM_IMPL_METHOD {
        1
    } else {
        2
    }
}

fn sym_ns(nodes: &[i64], sym: i64) -> i64 {
    let kind = sym_kind_of(nodes, sym);
    if kind == SYM_FUN || kind == SYM_NATIVE_FUN || kind == SYM_CONST || kind == SYM_VARIANT || kind == SYM_TRAIT_METHOD || kind == SYM_IMPL_METHOD {
        NS_VALUE
    } else {
        NS_TYPE
    }
}

fn rewrite_import(scopes: &mut [Vec<i64>], scope: i64, use_item: i64, sym: i64, ns: i64) {
    let entries = match scopes.get_mut(scope as usize) {
        Some(entries) => entries,
        None => return,
    };
    let mut idx = 0i64;
    while idx < entries.len() as i64 / 4 {
        if entry_get(entries, idx, 3) == use_item {
            entry_set(entries, idx, 1, sym);
            entry_set(entries, idx, 2, ns);
            return;
        }
        idx += 1;
    }
}

fn resolve_path(state: &mut State, scope: i64, segs: i64, final_ns: i64) -> i64 {
    let sym = resolve_path_sym(state, scope, segs, final_ns);
    record_dependency(state, sym);
    sym
}

// The pure descent of `resolve_path`: find the symbol a path names without
// recording a dependency edge; an impl's target is resolved this way.
fn resolve_path_sym(state: &mut State, scope: i64, segs: i64, final_ns: i64) -> i64 {
    let count = list_len(state.2, segs);
    if count == 0 {
        return NONE;
    }
    let mut current = scope;
    let mut idx = 0i64;
    while idx < count - 1 {
        let (sym, src) = lookup_walk(state.4, state.5, current, list_get(state.2, segs, idx), NS_TYPE);
        if sym == NONE {
            return NONE;
        }
        if src != NONE {
            mark_used(state.9, src);
        }
        let sub = sym_sub_of(state.1, sym);
        if sub == NONE {
            return NONE;
        }
        current = sub;
        idx += 1;
    }
    let (sym, src) = lookup_walk(state.4, state.5, current, list_get(state.2, segs, count - 1), final_ns);
    if sym != NONE && src != NONE {
        mark_used(state.9, src);
    }
    sym
}

// Records that the items being resolved depend on `sym`: every stack owner
// gets an incoming edge, or the permanent root when the stack is empty.
fn record_dependency(state: &mut State, sym: i64) {
    if sym == NONE {
        return;
    }
    if state.11.is_empty() {
        if !state.12.iter().any(|edge| edge.0 == ROOT_OWNER && edge.1 == sym) {
            state.12.push((ROOT_OWNER, sym));
        }
        return;
    }
    let mut oidx = 0usize;
    while oidx < state.11.len() {
        let owner = match state.11.get(oidx) {
            Some(value) => *value,
            None => break,
        };
        if owner != sym && !state.12.iter().any(|edge| edge.0 == owner && edge.1 == sym) {
            state.12.push((owner, sym));
        }
        oidx += 1;
    }
}

/// Every item symbol reachable from `main`, or `None` when there is no
/// `main` to reach from: a compilation unit without an entry point is not
/// a whole program, and reachability is a whole-program property.
fn reachable_from_main(state: &State) -> Option<Vec<i64>> {
    let mut reached: Vec<i64> = vec![ROOT_OWNER];
    let mut found_main = false;
    let mut idx = 0i64;
    while idx < state.1.len() as i64 / NODE_STRIDE {
        // The kind matters as well as the flag: slot `f` means something
        // else for non-function symbol kinds.
        if node_tag(state.1, idx) == NODE_SYM
            && node_a(state.1, idx) == SYM_FUN
            && node_f(state.1, idx) == SYM_FUN_MAIN
        {
            reached.push(idx);
            found_main = true;
        }
        idx += 1;
    }
    if !found_main {
        return None;
    }
    // Fixpoint rather than a single pass: an item pulled in by a later edge
    // brings its own dependencies with it.
    let mut grew = true;
    while grew {
        grew = false;
        let mut edge_idx = 0usize;
        while edge_idx < state.12.len() {
            match state.12.get(edge_idx) {
                Some(edge) => {
                    if contains_i64(&reached, edge.0) && !contains_i64(&reached, edge.1) {
                        reached.push(edge.1);
                        grew = true;
                    }
                }
                None => break,
            }
            edge_idx += 1;
        }
    }
    Some(reached)
}

/// The word for an item kind in an "unused ..." diagnostic, and whether the
/// kind is one reachability applies to at all.
fn unreachable_label(kind: i64) -> Option<&'static str> {
    if kind == ITEM_FUN {
        Some("function")
    } else if kind == ITEM_NATIVE_FUN {
        Some("native function")
    } else if kind == ITEM_CONST {
        Some("constant")
    } else if kind == ITEM_STRUCT {
        Some("struct")
    } else if kind == ITEM_ENUM {
        Some("enum")
    } else if kind == ITEM_TRAIT {
        Some("trait")
    } else if kind == ITEM_NATIVE_TYPE {
        Some("native type")
    } else {
        // Modules are namespaces, `use` items have their own unused check,
        // and impls extend the type they are for.
        None
    }
}

/// Whether `sym` has a live incoming edge from a caller in another module
/// scope, or is exposed by a reached public function's signature.
fn has_live_cross_module_edge(state: &State, sym: i64, reached: &[i64]) -> bool {
    let target_home = sym_home_of(state.1, sym);
    let mut idx = 0usize;
    while idx < state.12.len() {
        match state.12.get(idx) {
            Some(edge) => {
                if edge.1 == sym && edge.0 >= 0 && contains_i64(reached, edge.0)
                    && sym_home_of(state.1, edge.0) != target_home {
                        return true;
                    }
            }
            None => break,
        }
        idx += 1;
    }
    let kind = node_a(state.1, sym);
    if kind != SYM_STRUCT && kind != SYM_ENUM {
        return false;
    }
    let mut idx = 0usize;
    while idx < state.12.len() {
        match state.12.get(idx) {
            Some(edge) => {
                if edge.1 == sym && edge.0 >= 0 && contains_i64(reached, edge.0) {
                    let from = edge.0;
                    let from_kind = node_a(state.1, from);
                    if (from_kind == SYM_FUN || from_kind == SYM_NATIVE_FUN) && sym_home_of(state.1, from) == target_home {
                        let decl = node_c(state.1, from);
                        if node_tag(state.1, decl) == NODE_ITEM && item_is_pub(state.1, decl) == 1 && has_live_cross_module_edge(state, from, reached) {
                            return true;
                        }
                    }
                }
            }
            None => break,
        }
        idx += 1;
    }
    false
}

/// Emits an unconsumed-visibility diagnostic for a reached item whose `pub`
/// no live cross-module edge justifies.
fn report_unnecessary_pub(state: &mut State, item: i64, sym: i64, reached: &[i64], deferred: &mut Vec<Diag>) {
    if item_is_pub(state.1, item) != 1 {
        return;
    }
    if has_live_cross_module_edge(state, sym, reached) {
        return;
    }
    let kind = node_a(state.1, item);
    let name = if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
        node_a(state.1, node_d(state.1, item))
    } else {
        node_d(state.1, item)
    };
    push_error_kind(
        deferred,
        &format!("pub on '{}' has no cross-module caller", name_text(state.0, name)),
        node_file(state.1, item),
        node_start(state.1, item),
        node_end(state.1, item),
        DiagKind::UnnecessaryPub { sym },
    );
}

/// Reports every declared item nothing reachable from `main` needs, dead
/// impl methods, and unjustified `pub` on reached items.
fn report_unreachable(state: &mut State, list: i64, reached: &[i64], deferred: &mut Vec<Diag>) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        let item = list_get(state.2, list, idx);
        idx += 1;
        if node_tag(state.1, item) != NODE_ITEM {
            continue;
        }
        let kind = node_a(state.1, item);
        if kind == ITEM_MODULE {
            report_unreachable(state, node_e(state.1, item), reached, deferred);
            continue;
        }
        // A seeded builtin (`Result`, `Option`, `DivError`, `IndexError`)
        // is injected by the resolver and carries `NO_FILE`.
        if node_file(state.1, item) == NO_FILE {
            continue;
        }
        // An impl is reached exactly when the type it extends is; its
        // methods are the reportable units when that type is dead.
        if kind == ITEM_IMPL {
            let for_ty_sym = ty_sym_of(state.1, node_e(state.1, item));
            if for_ty_sym == NONE || contains_i64(reached, for_ty_sym) {
                continue;
            }
            let methods = node_f(state.1, item);
            let mcount = list_len(state.2, methods);
            let mut midx = 0i64;
            while midx < mcount {
                let method = list_get(state.2, methods, midx);
                push_error_kind(
                    deferred,
                    &format!("unused method '{}'", name_text(state.0, node_a(state.1, method))),
                    node_file(state.1, method),
                    node_start(state.1, method),
                    node_end(state.1, method),
                    DiagKind::UnusedDeclaration(method),
                );
                midx += 1;
            }
            continue;
        }
        let label = match unreachable_label(kind) {
            Some(text) => text,
            None => continue,
        };
        let sym = item_sym_of(state.1, item);
        if sym == NONE {
            continue;
        }
        if contains_i64(reached, sym) {
            // A reached item's `pub` is justified by a live cross-module
            // caller edge, read from the struct symbol's flag.
            report_unnecessary_pub(state, item, sym, reached, deferred);
            if kind == ITEM_STRUCT && sym != NONE {
                node_set_f(state.1, sym, 1);
            }
            continue;
        }
        if node_f(state.1, sym) == SYM_FUN_MAIN {
            continue;
        }
        let name = if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
            node_a(state.1, node_d(state.1, item))
        } else {
            node_d(state.1, item)
        };
        let text = format!("unused {} '{}'", label, name_text(state.0, name));
        push_error_kind(
            deferred,
            &text,
            node_file(state.1, item),
            node_start(state.1, item),
            node_end(state.1, item),
            DiagKind::UnusedDeclaration(sym),
        );
    }
}

fn mark_used(used: &mut Vec<i64>, use_item: i64) {
    if !contains_i64(used, use_item) {
        used.push(use_item);
    }
}


fn join_segs(names: &[String], lists: &[Vec<i64>], segs: i64) -> String {
    let mut text = String::new();
    let count = list_len(lists, segs);
    let mut idx = 0i64;
    while idx < count {
        if !text.is_empty() {
            text.push('.');
        }
        text.push_str(&name_text(names, list_get(lists, segs, idx)));
        idx += 1;
    }
    text
}

fn single_name_list(lists: &mut Vec<Vec<i64>>, name: i64) -> i64 {
    let list = alloc_list(lists);
    list_push(lists, list, name);
    list
}

fn copy_list(lists: &mut [Vec<i64>], from: i64, to: i64) {
    let count = list_len(lists, from);
    let mut idx = 0i64;
    while idx < count {
        let item = list_get(lists, from, idx);
        list_push(lists, to, item);
        idx += 1;
    }
}

fn ext_scope_of(ext_scopes: &[(i64, i64)], name: i64) -> i64 {
    let mut idx = 0usize;
    loop {
        match ext_scopes.get(idx) {
            Some(pair) => {
                if pair.0 == name {
                    return pair.1;
                }
            }
            None => return 0,
        }
        idx += 1;
    }
}

fn walk_item_list(state: &mut State, scope: i64, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        walk_item(state, scope, list_get(state.2, list, idx));
        idx += 1;
    }
}

fn walk_item(state: &mut State, scope: i64, item: i64) {
    if node_tag(state.1, item) != NODE_ITEM {
        return;
    }
    let kind = node_a(state.1, item);
    // A module is a namespace, owning nothing; `resolve_impl` owns the
    // impl's attribution instead.
    let owner = if kind == ITEM_MODULE || kind == ITEM_IMPL {
        NONE
    } else {
        let sym = item_sym_of(state.1, item);
        if sym == NONE { ROOT_OWNER } else { sym }
    };
    if owner != NONE {
        state.11.push(owner);
    }
    let item_scope = if kind == ITEM_MODULE {
        item_scope_of(state.8, item)
    } else {
        scope
    };
    attach_scope(state, item, item_scope);
    if kind == ITEM_MODULE {
        let children = node_e(state.1, item);
        walk_item_list(state, item_scope_of(state.8, item), children);
    } else if kind == ITEM_STRUCT {
        walk_fields(state, item_scope_of(state.8, item), item);
    } else if kind == ITEM_ENUM {
        walk_variants(state, item_scope_of(state.8, item), item);
    } else if kind == ITEM_TRAIT {
        walk_fn_list(state, scope, node_e(state.1, item));
    } else if kind == ITEM_IMPL {
        resolve_impl(state, scope, item);
    } else if kind == ITEM_FUN || kind == ITEM_NATIVE_FUN {
        walk_fn(state, scope, node_d(state.1, item));
    } else if kind == ITEM_CONST {
        walk_type(state, scope, node_e(state.1, item));
        walk_expr(state, scope, node_f(state.1, item));
    }
    if owner != NONE {
        state.11.pop();
    }
}

fn item_scope_of(item_scopes: &[(i64, i64)], item: i64) -> i64 {
    let mut idx = 0usize;
    loop {
        match item_scopes.get(idx) {
            Some(pair) => {
                if pair.0 == item {
                    return pair.1;
                }
            }
            None => return 0,
        }
        idx += 1;
    }
}

fn walk_fields(state: &mut State, scope: i64, item: i64) {
    let fields = node_e(state.1, item);
    let count = list_len(state.2, fields);
    let mut idx = 0i64;
    while idx < count {
        let field = list_get(state.2, fields, idx);
        walk_type(state, scope, node_b(state.1, field));
        idx += 1;
    }
    walk_type_params(state, scope, node_f(state.1, item));
}

fn walk_variants(state: &mut State, scope: i64, item: i64) {
    let variants = node_e(state.1, item);
    let count = list_len(state.2, variants);
    let mut idx = 0i64;
    while idx < count {
        let variant = list_get(state.2, variants, idx);
        walk_type_list(state, scope, node_b(state.1, variant));
        idx += 1;
    }
    walk_type_params(state, scope, node_f(state.1, item));
}

fn walk_type_params(state: &mut State, scope: i64, params: i64) {
    let count = list_len(state.2, params);
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(state.2, params, idx);
        if node_tag(state.1, param) == NODE_TY && node_a(state.1, param) == TY_PARAM {
            report_casing(state.0, node_b(state.1, param), 2, state.3, node_file(state.1, param), node_start(state.1, param), node_end(state.1, param));
            if node_c(state.1, param) != NONE {
                resolve_param_bound(state, scope, param);
            }
        }
        idx += 1;
    }
}

fn resolve_param_bound(state: &mut State, scope: i64, param: i64) {
    let segs = node_c(state.1, param);
    let sym = resolve_path(state, scope, segs, NS_TYPE);
    if sym == NONE {
        push_error(state.3, &format!("cannot resolve trait bound '{}'", join_segs(state.0, state.2, segs)), node_file(state.1, param), node_start(state.1, param), node_end(state.1, param));
        return;
    }
    if !is_visible(state.5, state.6, state.1, scope, sym) {
        push_error_kind(state.3, &format!("cannot access trait '{}' here", name_text(state.0, sym_name_of(state.1, sym))), node_file(state.1, param), node_start(state.1, param), node_end(state.1, param), DiagKind::PrivateAccess { sym });
        return;
    }
    node_set_c(state.1, param, sym);
}

fn walk_type_list(state: &mut State, scope: i64, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        walk_type(state, scope, list_get(state.2, list, idx));
        idx += 1;
    }
}

fn resolve_impl(state: &mut State, scope: i64, item: i64) {
    let trait_segs = node_d(state.1, item);
    let trait_sym = resolve_path(state, scope, trait_segs, NS_TYPE);
    if trait_sym == NONE {
        push_error(state.3, &format!("cannot resolve trait '{}'", join_segs(state.0, state.2, trait_segs)), node_file(state.1, item), node_start(state.1, item), node_end(state.1, item));
    } else if sym_kind_of(state.1, trait_sym) != SYM_TRAIT {
        push_error(state.3, "'impl' target is not a trait", node_file(state.1, item), node_start(state.1, item), node_end(state.1, item));
    } else {
        if !is_visible(state.5, state.6, state.1, scope, trait_sym) {
            push_error_kind(state.3, &format!("cannot access trait '{}' here", name_text(state.0, sym_name_of(state.1, trait_sym))), node_file(state.1, item), node_start(state.1, item), node_end(state.1, item), DiagKind::PrivateAccess { sym: trait_sym });
        }
        item_set_sym(state.1, item, trait_sym);
    }
    // The implementing type owns the impl's references, found without
    // recording a dependency.
    let for_ty = node_e(state.1, item);
    let for_ty_sym = impl_target_sym(state, scope, for_ty);
    if for_ty_sym != NONE {
        state.11.push(for_ty_sym);
    }
    walk_type(state, scope, for_ty);
    walk_fn_list(state, scope, node_f(state.1, item));
    if for_ty_sym != NONE {
        state.11.pop();
    }
}

// The symbol the impl extends, looked up without recording a dependency
// edge: attributing it to the root would keep a dead type alive.
fn impl_target_sym(state: &mut State, scope: i64, ty: i64) -> i64 {
    if node_tag(state.1, ty) != NODE_TY {
        return NONE;
    }
    let kind = node_a(state.1, ty);
    if kind == TY_NAMED {
        let (sym, src) = lookup_walk(state.4, state.5, scope, node_b(state.1, ty), NS_TYPE);
        if sym != NONE && src != NONE {
            mark_used(state.9, src);
        }
        sym
    } else if kind == TY_PATH || kind == TY_GENERIC {
        resolve_path_sym(state, scope, node_b(state.1, ty), NS_TYPE)
    } else {
        NONE
    }
}

fn walk_fn_list(state: &mut State, scope: i64, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        walk_fn(state, scope, list_get(state.2, list, idx));
        idx += 1;
    }
}

fn walk_fn(state: &mut State, scope: i64, fn_node: i64) {
    if node_tag(state.1, fn_node) != NODE_FN {
        return;
    }
    let prefix = scope_prefix_of(state.7, scope);
    let param_scope = alloc_scope(state.4, state.5, state.6, state.7, scope, prefix, 1);
    attach_scope(state, fn_node, param_scope);
    enter_type_params(state.1, state.2, state.4, param_scope, node_b(state.1, fn_node));
    walk_type_params(state, scope, node_b(state.1, fn_node));
    let params = node_c(state.1, fn_node);
    let count = list_len(state.2, params);
    let mut idx = 0i64;
    while idx < count {
        let param = list_get(state.2, params, idx);
        walk_type(state, param_scope, node_b(state.1, param));
        idx += 1;
    }
    walk_type(state, param_scope, node_d(state.1, fn_node));
    walk_stmt_list(state, param_scope, node_f(state.1, fn_node));
}

fn walk_stmt_list(state: &mut State, scope: i64, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        walk_stmt(state, scope, list_get(state.2, list, idx));
        idx += 1;
    }
}

fn walk_stmt(state: &mut State, scope: i64, stmt: i64) {
    if node_tag(state.1, stmt) != NODE_STMT {
        return;
    }
    attach_scope(state, stmt, scope);
    let kind = node_a(state.1, stmt);
    if kind == STMT_LET {
        if node_d(state.1, stmt) != NONE {
            walk_type(state, scope, node_d(state.1, stmt));
        }
        walk_expr(state, scope, node_e(state.1, stmt));
    } else if kind == STMT_ASSIGN {
        walk_expr(state, scope, node_b(state.1, stmt));
        walk_expr(state, scope, node_c(state.1, stmt));
    } else if kind == STMT_WHILE {
        walk_expr(state, scope, node_b(state.1, stmt));
        walk_stmt_list(state, scope, node_c(state.1, stmt));
    } else if kind == STMT_IF {
        walk_expr(state, scope, node_b(state.1, stmt));
        walk_stmt_list(state, scope, node_c(state.1, stmt));
        if node_d(state.1, stmt) != NONE {
            walk_stmt_list(state, scope, node_d(state.1, stmt));
        }
    } else if kind == STMT_RETURN {
        if node_b(state.1, stmt) != NONE {
            walk_expr(state, scope, node_b(state.1, stmt));
        }
    } else if kind == STMT_EXPR {
        walk_expr(state, scope, node_b(state.1, stmt));
    }
}

fn walk_expr(state: &mut State, scope: i64, expr: i64) {
    if node_tag(state.1, expr) != NODE_EXPR {
        return;
    }
    attach_scope(state, expr, scope);
    let kind = node_a(state.1, expr);
    if kind == EXPR_PATH {
        resolve_expr_path(state, scope, expr);
    } else if kind == EXPR_UNARY {
        walk_expr(state, scope, node_c(state.1, expr));
    } else if kind == EXPR_BINARY {
        walk_expr(state, scope, node_c(state.1, expr));
        walk_expr(state, scope, node_d(state.1, expr));
    } else if kind == EXPR_CALL {
        walk_call(state, scope, expr);
    } else if kind == EXPR_STRUCT_LIT {
        walk_struct_lit(state, scope, expr);
    } else if kind == EXPR_ARRAY {
        walk_expr_list(state, scope, node_b(state.1, expr));
    } else if kind == EXPR_MATCH {
        walk_expr(state, scope, node_b(state.1, expr));
        walk_arms(state, scope, node_c(state.1, expr));
    } else if kind == EXPR_TRY {
        walk_expr(state, scope, node_b(state.1, expr));
    } else if kind == EXPR_INDEX {
        walk_expr(state, scope, node_b(state.1, expr));
        walk_expr(state, scope, node_c(state.1, expr));
    } else if kind == EXPR_FIELD_ACCESS {
        walk_expr(state, scope, node_b(state.1, expr));
    }
}

fn walk_expr_list(state: &mut State, scope: i64, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        walk_expr(state, scope, list_get(state.2, list, idx));
        idx += 1;
    }
}

fn walk_arms(state: &mut State, scope: i64, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        let arm = list_get(state.2, list, idx);
        walk_pattern(state, scope, node_a(state.1, arm));
        walk_stmt(state, scope, node_b(state.1, arm));
        idx += 1;
    }
}

fn resolve_expr_path(state: &mut State, scope: i64, expr: i64) {
    let segs = node_b(state.1, expr);
    let sym = resolve_path(state, scope, segs, NS_VALUE);
    if sym == NONE {
        return;
    }
    if !is_visible(state.5, state.6, state.1, scope, sym) {
        push_error_kind(state.3, &format!("cannot access '{}' here", name_text(state.0, sym_name_of(state.1, sym))), node_file(state.1, expr), node_start(state.1, expr), node_end(state.1, expr), DiagKind::PrivateAccess { sym });
        return;
    }
    expr_set_sym(state.1, expr, sym);
}

fn walk_call(state: &mut State, scope: i64, expr: i64) {
    let callee = node_b(state.1, expr);
    if node_tag(state.1, callee) == NODE_EXPR && node_a(state.1, callee) == EXPR_PATH {
        let segs = node_b(state.1, callee);
        let sym = resolve_path(state, scope, segs, NS_VALUE);
        if sym != NONE {
            let kind = sym_kind_of(state.1, sym);
            if kind == SYM_VARIANT || kind == SYM_STRUCT || kind == SYM_ENUM {
                node_set_a(state.1, expr, EXPR_STRUCT_LIT);
                node_set_b(state.1, expr, segs);
                node_set_c(state.1, expr, NONE);
                expr_set_sym(state.1, expr, sym);
                walk_expr_list(state, scope, node_d(state.1, expr));
                return;
            }
            if !is_visible(state.5, state.6, state.1, scope, sym) {
                push_error_kind(state.3, &format!("cannot call '{}' here", name_text(state.0, sym_name_of(state.1, sym))), node_file(state.1, expr), node_start(state.1, expr), node_end(state.1, expr), DiagKind::PrivateAccess { sym });
            } else {
                expr_set_sym(state.1, callee, sym);
            }
        }
    }
    if node_c(state.1, expr) != NONE {
        walk_type_list(state, scope, node_c(state.1, expr));
    }
    walk_expr_list(state, scope, node_d(state.1, expr));
}

fn walk_struct_lit(state: &mut State, scope: i64, expr: i64) {
    let segs = node_b(state.1, expr);
    let file = node_file(state.1, expr);
    let start = node_start(state.1, expr);
    let end = node_end(state.1, expr);
    if list_len(state.2, segs) > 1 {
        push_error(state.3, "qualified struct initialization is not allowed", file, start, end);
        return;
    }
    let sym = resolve_path(state, scope, segs, NS_TYPE);
    if sym == NONE {
        push_error(state.3, &format!("unknown type '{}'", join_segs(state.0, state.2, segs)), file, start, end);
        let first = list_first(state.2, segs);
        if first != NONE {
            suggest_type_name(state, scope, first);
        }
        return;
    }
    if !is_visible(state.5, state.6, state.1, scope, sym) {
        push_error_kind(state.3, &format!("cannot access type '{}' here", name_text(state.0, sym_name_of(state.1, sym))), file, start, end, DiagKind::PrivateAccess { sym });
        return;
    }
    expr_set_sym(state.1, expr, sym);
    walk_expr_list(state, scope, node_d(state.1, expr));
}

fn walk_pattern(state: &mut State, scope: i64, pat: i64) {
    if node_tag(state.1, pat) != NODE_PAT {
        return;
    }
    attach_scope(state, pat, scope);
    let kind = node_a(state.1, pat);
    if kind == PAT_BIND {
        let name = node_b(state.1, pat);
        let (sym, src) = lookup_walk(state.4, state.5, scope, name, NS_VALUE);
        if sym == NONE {
            return;
        }
        if src != NONE {
            mark_used(state.9, src);
        }
        if sym_kind_of(state.1, sym) != SYM_VARIANT || !is_visible(state.5, state.6, state.1, scope, sym) {
            push_error(state.3, &format!("'{}' is not a variant pattern", name_text(state.0, name)), node_file(state.1, pat), node_start(state.1, pat), node_end(state.1, pat));
            return;
        }
        let segs = single_name_list(state.2, name);
        node_set_a(state.1, pat, PAT_PATH);
        node_set_b(state.1, pat, segs);
        pat_set_sym(state.1, pat, sym);
    } else if kind == PAT_PATH {
        resolve_pat_path(state, scope, pat);
    } else if kind == PAT_VARIANT {
        resolve_pat_path(state, scope, pat);
        walk_pattern_list(state, scope, node_c(state.1, pat));
    } else if kind == PAT_ARRAY {
        walk_pattern_list(state, scope, node_b(state.1, pat));
    }
}

fn resolve_pat_path(state: &mut State, scope: i64, pat: i64) {
    let segs = node_b(state.1, pat);
    let sym = resolve_path(state, scope, segs, NS_VALUE);
    if sym == NONE {
        push_error(state.3, &format!("cannot resolve pattern '{}'", join_segs(state.0, state.2, segs)), node_file(state.1, pat), node_start(state.1, pat), node_end(state.1, pat));
        return;
    }
    if !is_visible(state.5, state.6, state.1, scope, sym) {
        push_error_kind(state.3, &format!("cannot access '{}' here", name_text(state.0, sym_name_of(state.1, sym))), node_file(state.1, pat), node_start(state.1, pat), node_end(state.1, pat), DiagKind::PrivateAccess { sym });
        return;
    }
    pat_set_sym(state.1, pat, sym);
}

fn walk_pattern_list(state: &mut State, scope: i64, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        walk_pattern(state, scope, list_get(state.2, list, idx));
        idx += 1;
    }
}

fn walk_type(state: &mut State, scope: i64, ty: i64) {
    if node_tag(state.1, ty) != NODE_TY {
        return;
    }
    attach_scope(state, ty, scope);
    let kind = node_a(state.1, ty);
    if kind == TY_NAMED {
        resolve_type_name(state, scope, ty, node_b(state.1, ty));
    } else if kind == TY_PATH {
        resolve_type_path(state, scope, ty);
    } else if kind == TY_GENERIC {
        resolve_type_path(state, scope, ty);
        walk_type_list(state, scope, node_c(state.1, ty));
    } else if kind == TY_REF || kind == TY_REF_MUT || kind == TY_SLICE || kind == TY_ARRAY {
        walk_type(state, scope, node_b(state.1, ty));
    }
}

fn resolve_type_name(state: &mut State, scope: i64, ty: i64, name: i64) {
    let (sym, src) = lookup_walk(state.4, state.5, scope, name, NS_TYPE);
    if sym == NONE {
        push_error(state.3, &format!("unknown type '{}'", name_text(state.0, name)), node_file(state.1, ty), node_start(state.1, ty), node_end(state.1, ty));
        suggest_type_name(state, scope, name);
        return;
    }
    if src != NONE {
        mark_used(state.9, src);
    }
    // A single-identifier type never goes through `resolve_path`, so the
    // dependency is recorded here.
    record_dependency(state, sym);
    if !is_visible(state.5, state.6, state.1, scope, sym) {
        push_error_kind(state.3, &format!("cannot access type '{}' here", name_text(state.0, name)), node_file(state.1, ty), node_start(state.1, ty), node_end(state.1, ty), DiagKind::PrivateAccess { sym });
        return;
    }
    ty_set_sym(state.1, ty, sym);
}

fn resolve_type_path(state: &mut State, scope: i64, ty: i64) {
    let segs = node_b(state.1, ty);
    let sym = resolve_path(state, scope, segs, NS_TYPE);
    if sym == NONE {
        push_error(state.3, &format!("cannot resolve type '{}'", join_segs(state.0, state.2, segs)), node_file(state.1, ty), node_start(state.1, ty), node_end(state.1, ty));
        if list_len(state.2, segs) == 1 {
            let first = list_first(state.2, segs);
            if first != NONE {
                suggest_type_name(state, scope, first);
            }
        }
        return;
    }
    if !is_visible(state.5, state.6, state.1, scope, sym) {
        push_error_kind(state.3, &format!("cannot access type '{}' here", name_text(state.0, sym_name_of(state.1, sym))), node_file(state.1, ty), node_start(state.1, ty), node_end(state.1, ty), DiagKind::PrivateAccess { sym });
        return;
    }
    ty_set_sym(state.1, ty, sym);
}

fn check_unused_imports(names: &[String], nodes: &[i64], lists: &[Vec<i64>], deferred: &mut Vec<Diag>, used: &[i64], list: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        let item = list_get(lists, list, idx);
        check_unused(names, nodes, lists, deferred, used, item);
        idx += 1;
    }
}

/// Reports a `use` that resolved to something no name in the file went on
/// to reach; an import that never resolved is skipped.
fn check_unused(names: &[String], nodes: &[i64], lists: &[Vec<i64>], deferred: &mut Vec<Diag>, used: &[i64], item: i64) {
    if node_tag(nodes, item) != NODE_ITEM {
        return;
    }
    let kind = node_a(nodes, item);
    if kind == ITEM_MODULE {
        let children = node_e(nodes, item);
        check_unused_imports(names, nodes, lists, deferred, used, children);
        return;
    }
    if kind != ITEM_USE {
        return;
    }
    if item_is_pub(nodes, item) == 1 {
        return;
    }
    let sym = item_sym_of(nodes, item);
    if sym == NONE {
        return;
    }
    if contains_i64(used, item) {
        return;
    }
    let segs = node_d(nodes, item);
    let alias = node_e(nodes, item);
    let name = if alias != NONE { alias } else { list_last(lists, segs) };
    push_error_kind(
        deferred,
        &format!("unused import '{}'", name_text(names, name)),
        node_file(nodes, item),
        node_start(nodes, item),
        node_end(nodes, item),
        DiagKind::UnusedImport(item),
    );
}

// The visible type-namespace names from `scope` up through its parents, as
// (name, file, start, end) per declaration.
fn visible_type_names(state: &State, scope: i64) -> Vec<(String, i64, i64, i64)> {
    let mut out: Vec<(String, i64, i64, i64)> = Vec::new();
    let mut seen: Vec<i64> = Vec::new();
    let mut current = scope;
    while current != NONE {
        let entries = match state.4.get(current as usize) {
            Some(value) => value,
            None => break,
        };
        let count = entries.len() as i64 / 4;
        let mut idx = 0i64;
        while idx < count {
            let name = entry_get(entries, idx, 0);
            let sym = entry_get(entries, idx, 1);
            let ns = entry_get(entries, idx, 2);
            if ns == NS_TYPE
                && sym != NONE
                && !contains_i64(&seen, name)
                && is_visible(state.5, state.6, state.1, scope, sym)
            {
                seen.push(name);
                let decl = sym_decl_of(state.1, sym);
                if decl != NONE && node_tag(state.1, decl) == NODE_ITEM && node_file(state.1, decl) != NO_FILE {
                    out.push((
                        name_text(state.0, name),
                        node_file(state.1, decl),
                        node_start(state.1, decl),
                        node_end(state.1, decl),
                    ));
                }
            }
            idx += 1;
        }
        current = parent_of(state.5, current);
    }
    out
}

// Offers a hedged "did you mean" note for an unresolved type name; the
// error must already be pushed so the note attaches to it.
fn suggest_type_name(state: &mut State, scope: i64, misspelled: i64) {
    let text = name_text(state.0, misspelled);
    let entries = visible_type_names(state, scope);
    let mut candidates: Vec<suggest::Candidate> = Vec::new();
    let mut idx = 0usize;
    while idx < entries.len() {
        match entries.get(idx) {
            Some(entry) => {
                candidates.push(suggest::Candidate {
                    name: entry.0.clone(),
                    file: entry.1,
                    start: entry.2,
                    end: entry.3,
                });
            }
            None => break,
        }
        idx += 1;
    }
    if let Some(suggestion) = suggest::suggest(&text, &candidates) {
        push_note_for_last(state.3, state.10, &suggestion.message, suggestion.file, suggestion.start, suggestion.end, NOTE_GUIDANCE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Runs the real front end over an in-memory source, no LLVM needed.
    fn errors_for(source: &str) -> Vec<String> {
        let overlay = [("scratch.cnb".to_string(), source.to_string())];
        let result = crate::analysis::analyze("scratch.cnb", &overlay, &crate::target::Target::host());
        result.errors.iter().map(|d| d.message.clone()).collect()
    }

    #[test]
    fn seeded_identity_table_fills_every_slot() {
        let mut names: Vec<String> = Vec::new();
        let mut nodes: Vec<i64> = Vec::new();
        let mut lists: Vec<Vec<i64>> = Vec::new();
        let mut errors: Vec<Diag> = Vec::new();
        let mut notes: Vec<Note> = Vec::new();
        let mut deferred: Vec<Diag> = Vec::new();
        // Pass one: a bare program populates only the primitive seed
        // slots; protocol slots stay `NONE`.
        let source = "pub fun main() I32\n  return 0\nend\n";
        let overlay = [("scratch.cnb".to_string(), source.to_string())];
        let (loaded, files) = crate::module_loader::load_with_overlay(
            &mut names,
            &mut nodes,
            &mut lists,
            &mut errors,
            "scratch.cnb",
            &overlay,
        );
        let (root, ext_mods) = match loaded {
            Some(program) => program,
            None => {
                assert!(false, "module load failed: {:?}", errors);
                return;
            }
        };
        assert_eq!(files.len(), 1, "overlay source was not loaded");
        let mut seeds = Seeds::new();
        let resolved = resolve(
            &mut names,
            &mut nodes,
            &mut lists,
            Diagnostics { errors: &mut errors, notes: &mut notes, deferred: &mut deferred, target: &crate::target::Target::host() },
            root,
            &ext_mods,
            &mut seeds,
        );
        assert!(resolved, "resolve failed: {:?}", errors);
        let mut nidx = 0usize;
        while nidx < SEED_NAME_COUNT {
            assert_ne!(seeds.name(nidx), NONE, "seed name slot {} left empty", nidx);
            nidx += 1;
        }
        let mut sidx = 0usize;
        while sidx < SEED_SYM_PRIMITIVE_COUNT {
            assert_ne!(seeds.sym(sidx), NONE, "primitive seed symbol slot {} left empty", sidx);
            sidx += 1;
        }

        // Pass two: declaring every sealed native module populates each
        // protocol slot with the matching variant symbol.
        let mut names2: Vec<String> = Vec::new();
        let mut nodes2: Vec<i64> = Vec::new();
        let mut lists2: Vec<Vec<i64>> = Vec::new();
        let mut errors2: Vec<Diag> = Vec::new();
        let mut notes2: Vec<Note> = Vec::new();
        let mut deferred2: Vec<Diag> = Vec::new();
        let proto_source = "\
pub mod Memory\n  pub type Error\n    pub AllocationFailed(Usize)\n    pub AccessOutOfBounds(Usize, Usize)\n  end\nend\n\npub mod Collections\n  pub type Error\n    pub AllocationFailed(Usize)\n    pub IndexOutOfBounds(Usize)\n    pub KeyNotFound\n    pub InvalidUtf8\n    pub EndOfInput\n    pub ReadFailed(Usize)\n  end\nend\n\npub mod Terminal\n  pub type Error\n    pub ExitDiagnostic(I64)\n  end\nend\n\npub mod File\n  pub type Mode\n    pub ReadOnly\n    pub WriteTruncate\n    pub WriteAppend\n  end\n  pub type Error\n    pub SystemFault(I64)\n  end\nend\n\npub mod Net\n  pub type Error\n    pub SystemFault(I64)\n  end\nend\n\npub mod Process\n  pub type Error\n    pub SystemFault(I64)\n  end\nend\n\npub fun main() I32\n  return 0\nend\n";
        let overlay2 = [("scratch.cnb".to_string(), proto_source.to_string())];
        let (loaded2, files2) = crate::module_loader::load_with_overlay(
            &mut names2,
            &mut nodes2,
            &mut lists2,
            &mut errors2,
            "scratch.cnb",
            &overlay2,
        );
        let (root2, ext_mods2) = match loaded2 {
            Some(program) => program,
            None => {
                assert!(false, "module load failed: {:?}", errors2);
                return;
            }
        };
        assert_eq!(files2.len(), 1, "overlay source was not loaded");
        let mut seeds2 = Seeds::new();
        let resolved2 = resolve(
            &mut names2,
            &mut nodes2,
            &mut lists2,
            Diagnostics { errors: &mut errors2, notes: &mut notes2, deferred: &mut deferred2, target: &crate::target::Target::host() },
            root2,
            &ext_mods2,
            &mut seeds2,
        );
        assert!(resolved2, "resolve failed: {:?}", errors2);
        let mut pidx = SEED_SYM_PRIMITIVE_COUNT;
        while pidx < SEED_SYM_COUNT {
            assert_ne!(seeds2.sym(pidx), NONE, "protocol seed symbol slot {} left empty", pidx);
            pidx += 1;
        }
    }

    // An import nothing names is an error; the `use` item's symbol slot
    // tells a resolved import from one that never resolved.
    #[test]
    fn unused_import_is_rejected() {
        let source = r#"
pub mod Other
  pub fun helper() I64
    return 7
  end
  pub fun spare() I64
    return 9
  end
end

use Other.helper
use Other.spare

pub fun main() I64
  return helper()
end
"#;
        let errors = errors_for(source);
        assert!(errors.iter().any(|m| m.contains("unused import 'spare'")), "{:?}", errors);
    }

    // The alias is what the diagnostic must name, since the alias is the
    // name the program failed to use.
    #[test]
    fn unused_aliased_import_is_rejected_under_its_alias() {
        let source = r#"
pub mod Other
  pub fun helper() I64
    return 7
  end
  pub fun spare() I64
    return 9
  end
end

use Other.helper
use Other.spare as reserve

pub fun main() I64
  return helper()
end
"#;
        let errors = errors_for(source);
        assert!(errors.iter().any(|m| m.contains("unused import 'reserve'")), "{:?}", errors);
    }

    // Negative control: an import the program actually uses stays accepted,
    // and turning the check on must not make a clean program fail.
    #[test]
    fn used_import_is_accepted() {
        let source = r#"
pub mod Other
  pub fun helper() I64
    return 7
  end
end

use Other.helper

fun main() I64
  return helper()
end
"#;
        let errors = errors_for(source);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    // The unused-import check reports through `deferred`, so a program with
    // a real type error still reaches the typechecker.
    #[test]
    fn a_real_error_is_reported_ahead_of_an_unused_import() {
        let source = r#"
pub mod Other
  pub fun helper() I64
    return 7
  end
  pub fun spare() I64
    return 9
  end
end

use Other.helper
use Other.spare

pub fun main() I64
  val flag: Bool = helper()
  if flag
    return 1
  end
  return 0
end
"#;
        let errors = errors_for(source);
        assert!(!errors.is_empty(), "{:?}", errors);
        assert!(!errors.iter().any(|m| m.contains("unused import")), "{:?}", errors);
    }

    // A `use` naming a type mentioned only in a signature counts as used.
    #[test]
    fn import_used_only_in_a_type_position_is_accepted() {
        let source = r#"
pub mod Other
  pub type Payload
    pub value: I64
  end
end

use Other.Payload

fun read(payload: &Payload) I64
  return payload.value
end

fun main() I64
  val payload = Payload(value: 4)
  return read(&payload)
end
"#;
        let errors = errors_for(source);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    // An unresolvable import reports only its resolution failure.
    #[test]
    fn unresolvable_import_reports_only_the_resolution_failure() {
        let source = r#"
pub mod Other
  pub fun helper() I64
    return 7
  end
end

use Other.missing

pub fun main() I64
  return Other.helper()
end
"#;
        let errors = errors_for(source);
        assert!(errors.iter().any(|m| m.contains("cannot resolve import")), "{:?}", errors);
        assert!(!errors.iter().any(|m| m.contains("unused import")), "{:?}", errors);
    }

    // A `use` conflicting with a same-named local type is an error in
    // either declaration order.
    #[test]
    fn import_before_local_type_of_the_same_name_conflicts() {
        let source = r#"
pub mod Other
  pub type Shape
    pub width: I64
  end
end

use Other.Shape

pub type Shape
  pub height: I64
end

pub fun main() I64
  val shape = Shape(height: 3)
  return shape.height
end
"#;
        let errors = errors_for(source);
        assert!(errors.iter().any(|m| m.contains("import 'Shape' conflicts with another symbol")), "{:?}", errors);
    }

    // The same conflict in the opposite declaration order reports the
    // same diagnostic.
    #[test]
    fn local_type_before_import_of_the_same_name_conflicts() {
        let source = r#"
pub mod Other
  pub type Shape
    pub width: I64
  end
end

pub type Shape
  pub height: I64
end

use Other.Shape

pub fun main() I64
  val shape = Shape(height: 3)
  return shape.height
end
"#;
        let errors = errors_for(source);
        assert!(errors.iter().any(|m| m.contains("import 'Shape' conflicts with another symbol")), "{:?}", errors);
    }

    // With the import written first, `Shape(height: 3)` must still resolve
    // to the local type, not the imported one.
    #[test]
    fn import_does_not_silently_shadow_a_later_local_type() {
        let source = r#"
pub mod Other
  pub type Shape
    pub width: I64
  end
end

use Other.Shape

pub type Shape
  pub height: I64
end

pub fun main() I64
  val shape = Shape(height: 3)
  return shape.height
end
"#;
        let errors = errors_for(source);
        assert!(!errors.iter().any(|m| m.contains("Other.Shape")), "{:?}", errors);
    }

    // A local declaration and an import of an unrelated name coexist in
    // one scope.
    #[test]
    fn import_beside_an_unrelated_local_type_is_accepted() {
        let source = r#"
pub mod Other
  pub type Shape
    pub width: I64
  end
end

use Other.Shape

type Box
  height: I64
end

fun widen(shape: &Shape) I64
  return shape.width
end

fun main() I64
  val shape = Shape(width: 2)
  val box = Box(height: 3)
  return widen(&shape) + box.height
end
"#;
        let errors = errors_for(source);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    // The repro fixtures declare only imports their `main` reaches.
    #[test]
    fn fixture_corpus_stays_clean_of_dead_imports() {
        let paths = [
            "tests/fixtures/repro/mem_probe.cnb",
            "tests/fixtures/repro/slice_test.cnb",
            "tests/fixtures/repro/vec_pop_drain.cnb",
            "tests/fixtures/repro/hash_map_remove_drain.cnb",
            "tests/fixtures/repro/head.cnb",
        ];
        for path in paths {
            let result = crate::analysis::analyze(path, &[], &crate::target::Target::host());
            let errors: Vec<String> = result.errors.iter().map(|d| d.message.clone()).collect();
            assert!(errors.is_empty(), "{}: {:?}", path, errors);
        }
    }

    // A `use` written inside `mod ... end` resolves against the module's
    // own scope.
    #[test]
    fn a_use_inside_a_mod_block_resolves() {
        let source = r#"
pub mod Tools
  pub fun helper() I64
    return 1
  end
end

pub mod Wrapper
  use Tools.helper

  pub fun call_helper() I64
    return helper()
  end
end

fun main() I64
  return Wrapper.call_helper()
end
"#;
        let errors = errors_for(source);
        assert!(errors.is_empty(), "{:?}", errors);
    }

    // Negative control: an unresolvable module-local import reports the
    // resolution failure.
    #[test]
    fn an_unresolvable_use_inside_a_mod_block_is_rejected() {
        let source = r#"
pub mod Wrapper
  use Tools.nonexistent

  pub fun call_helper() I64
    return 1
  end
end

pub fun main() I64
  return Wrapper.call_helper()
end
"#;
        let errors = errors_for(source);
        assert!(errors.iter().any(|m| m.contains("cannot resolve import")), "{:?}", errors);
    }

    // A native function with no registry row is a resolution error naming
    // the function; builtins-only signature keeps this the one diagnostic.
    #[test]
    fn unknown_native_function_is_rejected() {
        let source = r#"
pub mod Widget
  pub nat fun touch() impure Unit
end

use Widget.touch

pub fun main() impure I64
  touch()
  return 0
end
"#;
        let errors = errors_for(source);
        assert!(
            errors.iter().any(|m| m.contains("unknown native function 'Widget.touch'")),
            "{:?}",
            errors
        );
    }

    // A native type with no registry row is rejected the same way, naming
    // the type; it has no layout kind to lower.
    #[test]
    fn unknown_native_type_is_rejected() {
        let source = r#"
pub mod Widget
  pub nat type Gadget
end

pub fun main() I64
  return 0
end
"#;
        let errors = errors_for(source);
        assert!(
            errors.iter().any(|m| m.contains("unknown native type 'Widget.Gadget'")),
            "{:?}",
            errors
        );
    }

    // The registry tables must be self-consistent: every verb supports its
    // declared mode, every type row names a known layout kind.
    #[test]
    fn registry_tables_are_self_consistent() {
        let mut idx = 0usize;
        while idx < NATIVE_FUN_ROWS.len() {
            let mode = NATIVE_FUN_ROWS[idx].1;
            let verb = NATIVE_FUN_ROWS[idx].2;
            assert!(
                verb_supports_mode(verb, mode),
                "row {}: verb {} does not support its declared mode {}",
                idx,
                verb,
                mode
            );
            idx += 1;
        }
        let mut tidx = 0usize;
        while tidx < NATIVE_TYPE_ROWS.len() {
            let layout = NATIVE_TYPE_ROWS[tidx].1;
            assert!(
                layout == NATIVE_LAYOUT_SCALAR
                    || layout == NATIVE_LAYOUT_PAIR
                    || layout == NATIVE_LAYOUT_TRIPLE,
                "type row {}: unknown layout kind {}",
                tidx,
                layout
            );
            tidx += 1;
        }
    }

    // The extraction binding of an extract-mode call lives on its
    // NODE_CALLFACT row; the call's type-argument slot keeps the list
    // the parser wrote.
    #[test]
    fn extraction_binding_is_a_fact_row_not_a_mutated_parse_slot() {
        let source = r#"
pub mod Collections
  pub nat type Vec(T)

  pub type Error
    pub AllocationFailed(Usize)
    pub IndexOutOfBounds(Usize, Usize)
  end

  pub nat fun vec_new<T>() impure Result(Vec(T), Error)
  pub nat fun vec_push<T>(vec: &mut Vec(T), value: T) impure Result(Unit, Error)
  pub nat fun vec_pop<T>(vec: &mut Vec(T)) impure Result(T, Error)
  pub nat fun vec_free<T>(vec: Vec(T)) impure Unit
end

use Collections.vec_new
use Collections.vec_push
use Collections.vec_pop
use Collections.vec_free

fun fail(vec: Collections.Vec(I64), code: I64) impure I64
  vec_free(vec)
  return code
end

fun main() impure I64
  val vec = match vec_new[I64]()
    Ok(v) => v
    Err(error) => return 1
  end

  match vec_push[I64](&mut vec, 7)
    Ok(Unit) => Unit
    Err(error) => return fail(vec, 2)
  end

  val popped = match vec_pop[I64](&mut vec)
    Ok(value) => value
    Err(error) => return fail(vec, 3)
  end
  if popped != 7
    return fail(vec, 4)
  end

  vec_free(vec)
  return 0
end
"#;
        let overlay = [("scratch.cnb".to_string(), source.to_string())];
        let result = crate::analysis::analyze("scratch.cnb", &overlay, &crate::target::Target::host());
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        // Exactly one extraction binding: the vec_pop call's call-fact row.
        let mut call = NONE;
        let mut row = 0i64;
        while row < result.nodes.len() as i64 / NODE_STRIDE {
            if node_tag(&result.nodes, row) == NODE_CALLFACT && node_d(&result.nodes, row) != NONE {
                call = node_a(&result.nodes, row);
            }
            row += 1;
        }
        assert!(call != NONE, "no extraction call-fact row was attached");
        assert_eq!(node_tag(&result.nodes, call), NODE_EXPR);
        assert_eq!(node_a(&result.nodes, call), EXPR_CALL);
        // The call's type-argument slot still holds the parser's list.
        assert!(
            node_c(&result.nodes, call) != NONE,
            "EXPR_CALL type-argument slot was clobbered"
        );
        // The row names the container the borrow checker drains.
        let name = node_d(&result.nodes, callfact_row_of(&result.nodes, call));
        assert_eq!(name_text(&result.names, name), "vec");
    }
}

