use crate::ast::*;

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
);

pub fn resolve(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    errors: &mut Vec<Diag>,
    root: i64,
    ext_mods: &[(i64, i64)],
) -> bool {
    let mut scopes: Vec<Vec<i64>> = Vec::new();
    let mut parents: Vec<i64> = Vec::new();
    let mut pubs: Vec<i64> = Vec::new();
    let mut prefixes: Vec<i64> = Vec::new();
    let mut item_scopes: Vec<(i64, i64)> = Vec::new();
    let mut used: Vec<i64> = Vec::new();
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

    check_unused_imports(state.0, state.1, state.2, state.3, state.9, root);
    idx = 0;
    while idx < ext_mods.len() {
        match ext_mods.get(idx) {
            Some(pair) => check_unused_imports(state.0, state.1, state.2, state.3, state.9, pair.1),
            None => break,
        }
        idx += 1;
    }

    state.3.is_empty()
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

fn scope_lookup(scopes: &[Vec<i64>], scope: i64, name: i64, ns: i64) -> (i64, i64) {
    let entries = match scopes.get(scope as usize) {
        Some(entries) => entries,
        None => return (NONE, NONE),
    };
    let mut idx = 0i64;
    while idx < entries.len() as i64 / 4 {
        if entry_get(entries, idx, 0) == name && entry_get(entries, idx, 2) == ns {
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
    node_e(nodes, sym)
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
        let rule = if casing == 1 {
            "snake_case"
        } else if casing == 2 {
            "PascalCase"
        } else {
            "SCREAMING_SNAKE_CASE"
        };
        push_error(errors, &format!("'{}' violates casing rule: expected {}", name_text(names, name), rule), file, start, end);
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
        collect_fields_casing(state.0, state.1, state.2, state.3, item);
    } else if kind == ITEM_ENUM {
        let name = node_d(state.1, item);
        let full = qualified_name(state.0, state.2, prefix, name);
        let sym = alloc_sym(state.1, SYM_ENUM, full, item, scope, NONE);
        sym_set_prim_kind(state.1, sym, prim_kind_of(state.0, full));
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
        collect_trait_methods(state, sub, full, item);
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
            sym_set_native_op(state.1, sym, native_opcode_of(state.0, full));
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
        node_set_e(state.1, sym, sub);
        state.8.push((item, sub));
        enter_type_params(state.1, state.2, state.4, sub, node_e(state.1, item));
    } else if kind == ITEM_USE {
        let segs = node_d(state.1, item);
        let last = list_last(state.2, segs);
        let alias = node_e(state.1, item);
        let entry_name = if alias != NONE { alias } else { last };
        push_entry(state.4, scope, entry_name, NONE, 0, item);
    }
}

fn collect_fields_casing(names: &[String], nodes: &[i64], lists: &[Vec<i64>], errors: &mut Vec<Diag>, item: i64) {
    let fields = node_e(nodes, item);
    let count = list_len(lists, fields);
    let mut idx = 0i64;
    while idx < count {
        let field = list_get(lists, fields, idx);
        report_casing(names, node_a(nodes, field), 1, errors, node_file(nodes, field), node_start(nodes, field), node_end(nodes, field));
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
    let mut idx = 0i64;
    while idx < count {
        let variant = list_get(state.2, variants, idx);
        let var_name = node_a(state.1, variant);
        report_casing(state.0, var_name, 2, state.3, node_file(state.1, variant), node_start(state.1, variant), node_end(state.1, variant));
        let single = single_name_list(state.2, enum_full);
        let full = qualified_name(state.0, state.2, single, var_name);
        let sym = alloc_sym(state.1, SYM_VARIANT, full, variant, sub, NONE);
        variant_set_sym(state.1, variant, sym);
        push_entry(state.4, sub, var_name, sym, NS_VALUE, NONE);
        insert_hoisted(state, hoist_scope, var_name, sym, variant);
        idx += 1;
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
}

fn collect_trait_methods(state: &mut State, sub: i64, trait_full: i64, item: i64) {
    let methods = node_e(state.1, item);
    let count = list_len(state.2, methods);
    let mut idx = 0i64;
    while idx < count {
        let fn_node = list_get(state.2, methods, idx);
        let name = node_a(state.1, fn_node);
        report_casing(state.0, name, 1, state.3, node_file(state.1, fn_node), node_start(state.1, fn_node), node_end(state.1, fn_node));
        let single = single_name_list(state.2, trait_full);
        let full = qualified_name(state.0, state.2, single, name);
        let sym = alloc_sym(state.1, SYM_TRAIT_METHOD, full, fn_node, sub, NONE);
        push_entry(state.4, sub, name, sym, NS_VALUE, NONE);
        idx += 1;
    }
}

fn seed_builtins(state: &mut State, root_scope: i64, root: i64) {
    let ints = builtin_int_names(state.0);
    let mut idx = 0usize;
    while idx < ints.len() {
        let name_id = match ints.get(idx) {
            Some(id) => *id,
            None => break,
        };
        let sub = seed_builtin_type(state, root_scope, name_id);
        seed_from_u8(state, sub, name_id);
        idx += 1;
    }
    let bool_name = intern(state.0, "Bool");
    seed_builtin_type(state, root_scope, bool_name);
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

fn seed_from_u8(state: &mut State, sub: i64, name: i64) {
    let prefix = alloc_list(state.2);
    list_push(state.2, prefix, name);
    let from_u8_name = intern(state.0, "from_u8");
    let method = qualified_name(state.0, state.2, prefix, from_u8_name);
    let from_u8 = alloc_sym(state.1, SYM_NATIVE_FUN, method, NONE, sub, NONE);
    sym_set_native_op(state.1, from_u8, native_opcode_of(state.0, method));
    push_entry(state.4, sub, from_u8_name, from_u8, NS_VALUE, NONE);
}

fn native_opcode_of(names: &[String], full: i64) -> i64 {
    if name_is(names, full, "U8.from_u8")
        || name_is(names, full, "U32.from_u8")
        || name_is(names, full, "Int.from_u8")
        || name_is(names, full, "Usize.from_u8")
    {
        return NAT_FROM_U8;
    }
    if name_is(names, full, "Slice.len") {
        return NAT_SLICE_LEN;
    }
    if name_is(names, full, "Memory.allocate") {
        return NAT_MEM_ALLOCATE;
    }
    if name_is(names, full, "Memory.deallocate") {
        return NAT_MEM_DEALLOCATE;
    }
    if name_is(names, full, "Memory.write_u8") {
        return NAT_MEM_WRITE_U8;
    }
    if name_is(names, full, "Memory.read_u8") {
        return NAT_MEM_READ_U8;
    }
    if name_is(names, full, "Collections.vec_new") {
        return NAT_VEC_NEW;
    }
    if name_is(names, full, "Collections.vec_push") {
        return NAT_VEC_PUSH;
    }
    if name_is(names, full, "Collections.vec_view") {
        return NAT_VEC_VIEW;
    }
    if name_is(names, full, "Collections.vec_free") {
        return NAT_VEC_FREE;
    }
    if name_is(names, full, "Collections.string_from_slice") {
        return NAT_STRING_FROM_SLICE;
    }
    if name_is(names, full, "Collections.string_len") {
        return NAT_STRING_LEN;
    }
    if name_is(names, full, "Collections.string_free") {
        return NAT_STRING_FREE;
    }
    if name_is(names, full, "Collections.hash_map_new") {
        return NAT_HASH_MAP_NEW;
    }
    if name_is(names, full, "Collections.hash_map_insert") {
        return NAT_HASH_MAP_INSERT;
    }
    if name_is(names, full, "Collections.hash_map_get") {
        return NAT_HASH_MAP_GET;
    }
    if name_is(names, full, "Collections.hash_map_free") {
        return NAT_HASH_MAP_FREE;
    }
    if name_is(names, full, "Runtime.self_check") {
        return NAT_SELF_CHECK;
    }
    if name_is(names, full, "Terminal.print") {
        return NAT_TERM_PRINT;
    }
    if name_is(names, full, "Terminal.print_line") {
        return NAT_TERM_PRINT_LINE;
    }
    if name_is(names, full, "Terminal.eprint") {
        return NAT_TERM_EPRINT;
    }
    if name_is(names, full, "Net.socket") {
        return NAT_NET_SOCKET;
    }
    if name_is(names, full, "Net.bind") {
        return NAT_NET_BIND;
    }
    if name_is(names, full, "Net.listen") {
        return NAT_NET_LISTEN;
    }
    if name_is(names, full, "Net.accept") {
        return NAT_NET_ACCEPT;
    }
    if name_is(names, full, "Net.send") {
        return NAT_NET_SEND;
    }
    if name_is(names, full, "Net.close") {
        return NAT_NET_CLOSE;
    }
    NAT_NONE
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
        intern(names, "Int"),
        intern(names, "U8"),
        intern(names, "U32"),
        intern(names, "Usize"),
    ]
}

fn seed_builtin_type(state: &mut State, root_scope: i64, name: i64) -> i64 {
    let prefix = alloc_list(state.2);
    list_push(state.2, prefix, name);
    let sub = alloc_scope(state.4, state.5, state.6, state.7, root_scope, prefix, 1);
    let sym = alloc_sym(state.1, SYM_TYPE, name, NONE, sub, sub);
    push_entry(state.4, root_scope, name, sym, NS_TYPE, NONE);
    sub
}

fn resolve_imports(state: &mut State, scope: i64, list: i64) {
    let count = list_len(state.2, list);
    let mut idx = 0i64;
    while idx < count {
        resolve_import(state, scope, list_get(state.2, list, idx));
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
        push_error(state.3, &format!("cannot import private item '{}'", name_text(state.0, sym_name_of(state.1, sym))), span.0, span.1, span.2);
        return;
    }
    let segs = node_d(state.1, item);
    let last = list_last(state.2, segs);
    let alias = node_e(state.1, item);
    let entry_name = if alias != NONE { alias } else { last };
    let casing = if target_ns == NS_TYPE { 2 } else { 1 };
    report_casing(state.0, entry_name, casing, state.3, span.0, span.1, span.2);
    let conflict = scope_lookup(state.4, scope, entry_name, target_ns);
    if conflict.0 != NONE && conflict.1 != item && conflict.0 != sym {
        push_error(state.3, &format!("import '{}' conflicts with another symbol", name_text(state.0, entry_name)), span.0, span.1, span.2);
        return;
    }
    rewrite_import(state.4, scope, item, sym, target_ns);
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

fn mark_used(used: &mut Vec<i64>, use_item: i64) {
    if !contains_i64(used, use_item) {
        used.push(use_item);
    }
}

fn list_last(lists: &[Vec<i64>], list: i64) -> i64 {
    let count = list_len(lists, list);
    if count == 0 {
        NONE
    } else {
        list_get(lists, list, count - 1)
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
        push_error(state.3, &format!("cannot access trait '{}' here", name_text(state.0, sym_name_of(state.1, sym))), node_file(state.1, param), node_start(state.1, param), node_end(state.1, param));
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
            push_error(state.3, &format!("cannot access trait '{}' here", name_text(state.0, sym_name_of(state.1, trait_sym))), node_file(state.1, item), node_start(state.1, item), node_end(state.1, item));
        }
        item_set_sym(state.1, item, trait_sym);
    }
    walk_type(state, scope, node_e(state.1, item));
    walk_fn_list(state, scope, node_f(state.1, item));
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
        push_error(state.3, &format!("cannot access '{}' here", name_text(state.0, sym_name_of(state.1, sym))), node_file(state.1, expr), node_start(state.1, expr), node_end(state.1, expr));
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
                push_error(state.3, &format!("cannot call '{}' here", name_text(state.0, sym_name_of(state.1, sym))), node_file(state.1, expr), node_start(state.1, expr), node_end(state.1, expr));
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
        return;
    }
    if !is_visible(state.5, state.6, state.1, scope, sym) {
        push_error(state.3, &format!("cannot access type '{}' here", name_text(state.0, sym_name_of(state.1, sym))), file, start, end);
        return;
    }
    expr_set_sym(state.1, expr, sym);
    walk_expr_list(state, scope, node_d(state.1, expr));
}

fn walk_pattern(state: &mut State, scope: i64, pat: i64) {
    if node_tag(state.1, pat) != NODE_PAT {
        return;
    }
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
        push_error(state.3, &format!("cannot access '{}' here", name_text(state.0, sym_name_of(state.1, sym))), node_file(state.1, pat), node_start(state.1, pat), node_end(state.1, pat));
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
        return;
    }
    if src != NONE {
        mark_used(state.9, src);
    }
    if !is_visible(state.5, state.6, state.1, scope, sym) {
        push_error(state.3, &format!("cannot access type '{}' here", name_text(state.0, name)), node_file(state.1, ty), node_start(state.1, ty), node_end(state.1, ty));
        return;
    }
    ty_set_sym(state.1, ty, sym);
}

fn resolve_type_path(state: &mut State, scope: i64, ty: i64) {
    let segs = node_b(state.1, ty);
    let sym = resolve_path(state, scope, segs, NS_TYPE);
    if sym == NONE {
        push_error(state.3, &format!("cannot resolve type '{}'", join_segs(state.0, state.2, segs)), node_file(state.1, ty), node_start(state.1, ty), node_end(state.1, ty));
        return;
    }
    if !is_visible(state.5, state.6, state.1, scope, sym) {
        push_error(state.3, &format!("cannot access type '{}' here", name_text(state.0, sym_name_of(state.1, sym))), node_file(state.1, ty), node_start(state.1, ty), node_end(state.1, ty));
        return;
    }
    ty_set_sym(state.1, ty, sym);
}

fn check_unused_imports(names: &[String], nodes: &[i64], lists: &[Vec<i64>], errors: &mut Vec<Diag>, used: &[i64], list: i64) {
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        let item = list_get(lists, list, idx);
        check_unused(names, nodes, lists, errors, used, item);
        idx += 1;
    }
}
fn check_unused(names: &[String], nodes: &[i64], lists: &[Vec<i64>], errors: &mut Vec<Diag>, used: &[i64], item: i64) {
    if node_tag(nodes, item) != NODE_ITEM {
        return;
    }
    let kind = node_a(nodes, item);
    if kind == ITEM_MODULE {
        let children = node_e(nodes, item);
        check_unused_imports(names, nodes, lists, errors, used, children);
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
    push_error(
        errors,
        &format!("unused import '{}'", name_text(names, name)),
        node_file(nodes, item),
        node_start(nodes, item),
        node_end(nodes, item),
    );
}
