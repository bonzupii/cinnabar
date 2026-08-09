//! Cinnabar module loader.
//!
//! Loads the entry file and every external module referenced by `use`
//! statements whose first segment is not a declared module: a `use
//! Math.add` with no in-file `mod Math` loads `Math.cnb` from the
//! importing file's directory and exposes its items under the module name
//! `Math`.  Loading is recursive and each file is recorded with its real
//! path and source text so diagnostics render at the true origin.  Every
//! file's tokens are lexed into the shared arenas and parsed from its own
//! `token_start`; a file's tokens end with their own `TOK_EOF`, so parsing
//! one file never runs into the next file's tokens.

use crate::ast::*;
use std::path::Path;

/// The loaded program: the entry item-list id and the external modules
/// `(name id, root list)`.  The module name is carried as its interned
/// id so every later comparison is an integer equality, never a string
/// compare.  The files table travels separately so failures during
/// loading still render diagnostics at their real origin.
type Loaded = (i64, Vec<(i64, i64)>);

/// The loader's own tables: loaded `(path, source)` files, external
/// modules `(name id, root list)`, and the work queue `(path, root list)`.
type Loader = (Vec<(String, String)>, Vec<(i64, i64)>, Vec<(String, i64)>);

pub fn load(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    errors: &mut Vec<Diag>,
    entry_path: &str,
) -> (Option<Loaded>, Vec<(String, String)>) {
    let root = alloc_list(lists);
    let source = match read_source(entry_path, errors) {
        Some(text) => text,
        None => return (None, Vec::new()),
    };
    let mut loader: Loader = (Vec::new(), Vec::new(), Vec::new());
    loader.0.push((entry_path.to_string(), source.clone()));
    if !parse_file(names, nodes, lists, errors, &source, 0, root) {
        return (None, loader.0);
    }
    loader.2.push((entry_path.to_string(), root));
    while let Some((path, list)) = loader.2.pop() {
        process_imports(names, nodes, lists, errors, &path, list, &mut loader);
    }
    let files = loader.0;
    let ext_mods = loader.1;
    (Some((root, ext_mods)), files)
}

/// Lexes and parses one file's tokens (starting after every previously
/// loaded file's tokens) into `root`.
fn parse_file(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    errors: &mut Vec<Diag>,
    source: &str,
    file_id: i64,
    root: i64,
) -> bool {
    let token_start = nodes.len() as i64 / NODE_STRIDE;
    if !crate::lexer::lex(names, nodes, source, file_id, errors) {
        return false;
    }
    crate::parser::parse(names, nodes, lists, errors, root, token_start)
}

/// Loads the sibling modules the file at `path` imports.  Files whose
/// first segment is a declared module or an already-loaded module are
/// skipped; a missing sibling file is left for the resolver to report as
/// an unknown module.
fn process_imports(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    errors: &mut Vec<Diag>,
    path: &str,
    list: i64,
    loader: &mut Loader,
) {
    let declared = declared_module_names(nodes, lists, list);
    let uses = use_targets(nodes, lists, list);
    let dir = parent_dir(path);
    let mut idx = 0usize;
    while let Some(pair) = uses.get(idx) {
        if !name_in(&declared, pair.0)
            && !loaded_name(&loader.1, pair.0)
            && let Some(module) = sibling_module(&dir, names, pair.0)
        {
            load_sibling(names, nodes, lists, errors, module, pair.1, loader);
        }
        idx += 1;
    }
}

/// Resolves one external module to `(path, module name id)`, or None
/// when no sibling file exists.
fn sibling_module(dir: &str, names: &[String], seg: i64) -> Option<(String, i64)> {
    let mod_path = sibling_path(dir, names, seg)?;
    Some((mod_path, seg))
}

/// Loads one external module file: reads it, parses it into its own item
/// list, and queues it for its own imports to be processed.
fn load_sibling(
    names: &mut Vec<String>,
    nodes: &mut Vec<i64>,
    lists: &mut Vec<Vec<i64>>,
    errors: &mut Vec<Diag>,
    module: (String, i64),
    use_item: i64,
    loader: &mut Loader,
) {
    let (mod_path, mod_name) = module;
    let source = read_source_at(&mod_path, errors, node_file(nodes, use_item), node_start(nodes, use_item), node_end(nodes, use_item));
    let mod_source = match source {
        Some(text) => text,
        None => return,
    };
    let child_list = alloc_list(lists);
    let child_id = loader.0.len() as i64;
    loader.0.push((mod_path.clone(), mod_source.clone()));
    if !parse_file(names, nodes, lists, errors, &mod_source, child_id, child_list) {
        return;
    }
    loader.1.push((mod_name, child_list));
    loader.2.push((mod_path, child_list));
}

// ---------------------------------------------------------------------------
// File helpers.
// ---------------------------------------------------------------------------

/// Reads `path`, binding the I/O failure into the diagnostic.  A missing
/// input file has no Cinnabar source origin, so that diagnostic is
/// source-less.
fn read_source(path: &str, errors: &mut Vec<Diag>) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(cause) => {
            errors.push((format!("cannot read input file '{}': {}", path, cause), NO_FILE, 0, 0));
            None
        }
    }
}

/// Reads a module file, reporting any failure at the `use` statement that
/// referenced it.
fn read_source_at(path: &str, errors: &mut Vec<Diag>, file: i64, start: i64, end: i64) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(cause) => {
            push_error(errors, &format!("cannot read module file '{}': {}", path, cause), file, start, end);
            None
        }
    }
}

/// The directory containing `path`, or "." when there is none.
fn parent_dir(path: &str) -> String {
    match Path::new(path).parent() {
        Some(dir) => dir.to_string_lossy().to_string(),
        None => ".".to_string(),
    }
}

/// `<dir>/<name>.cnb` when that file exists.
fn sibling_path(dir: &str, names: &[String], seg: i64) -> Option<String> {
    let name = name_text(names, seg);
    let candidate = Path::new(dir).join(format!("{}.cnb", name));
    let path = candidate.to_string_lossy().to_string();
    if Path::new(&path).exists() {
        Some(path)
    } else {
        None
    }
}

/// True when `seg` names a module that is already loaded (or queued).
/// The module names are interned ids, so this is an integer equality.
fn loaded_name(ext_mods: &[(i64, i64)], seg: i64) -> bool {
    let mut idx = 0usize;
    loop {
        match ext_mods.get(idx) {
            Some(pair) => {
                if pair.0 == seg {
                    return true;
                }
            }
            None => return false,
        }
        idx += 1;
    }
}

/// True when `name` appears in `list`.
fn name_in(list: &[i64], name: i64) -> bool {
    let mut idx = 0usize;
    loop {
        match list.get(idx) {
            Some(candidate) => {
                if *candidate == name {
                    return true;
                }
            }
            None => return false,
        }
        idx += 1;
    }
}

// ---------------------------------------------------------------------------
// Item scanning.
// ---------------------------------------------------------------------------

/// The declared module name ids in `list` (recursively, since a `use`
/// inside a module resolves against modules declared anywhere in the file).
fn declared_module_names(nodes: &[i64], lists: &[Vec<i64>], list: i64) -> Vec<i64> {
    let mut found: Vec<i64> = Vec::new();
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        collect_module_names(nodes, lists, list_get(lists, list, idx), &mut found);
        idx += 1;
    }
    found
}

fn collect_module_names(nodes: &[i64], lists: &[Vec<i64>], item: i64, found: &mut Vec<i64>) {
    if node_tag(nodes, item) != NODE_ITEM || node_a(nodes, item) != ITEM_MODULE {
        return;
    }
    found.push(node_d(nodes, item));
    let children = node_e(nodes, item);
    let count = list_len(lists, children);
    let mut idx = 0i64;
    while idx < count {
        collect_module_names(nodes, lists, list_get(lists, children, idx), found);
        idx += 1;
    }
}

/// The `(first_segment_name, use_item)` pairs for every `use` in `list`.
fn use_targets(nodes: &[i64], lists: &[Vec<i64>], list: i64) -> Vec<(i64, i64)> {
    let mut found: Vec<(i64, i64)> = Vec::new();
    let count = list_len(lists, list);
    let mut idx = 0i64;
    while idx < count {
        collect_use_targets(nodes, lists, list_get(lists, list, idx), &mut found);
        idx += 1;
    }
    found
}

fn collect_use_targets(nodes: &[i64], lists: &[Vec<i64>], item: i64, found: &mut Vec<(i64, i64)>) {
    if node_tag(nodes, item) != NODE_ITEM {
        return;
    }
    if node_a(nodes, item) == ITEM_USE {
        let first = list_first(lists, node_d(nodes, item));
        if first != NONE {
            found.push((first, item));
        }
    } else if node_a(nodes, item) == ITEM_MODULE {
        let children = node_e(nodes, item);
        let count = list_len(lists, children);
        let mut idx = 0i64;
        while idx < count {
            collect_use_targets(nodes, lists, list_get(lists, children, idx), found);
            idx += 1;
        }
    }
}
