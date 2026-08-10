use crate::ast::*;
use std::path::Path;

type Loaded = (i64, Vec<(i64, i64)>);

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

fn sibling_module(dir: &str, names: &[String], seg: i64) -> Option<(String, i64)> {
    let mod_path = sibling_path(dir, names, seg)?;
    Some((mod_path, seg))
}

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

fn read_source(path: &str, errors: &mut Vec<Diag>) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(cause) => {
            errors.push((format!("cannot read input file '{}': {}", path, cause), NO_FILE, 0, 0));
            None
        }
    }
}

fn read_source_at(path: &str, errors: &mut Vec<Diag>, file: i64, start: i64, end: i64) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(cause) => {
            push_error(errors, &format!("cannot read module file '{}': {}", path, cause), file, start, end);
            None
        }
    }
}

fn parent_dir(path: &str) -> String {
    match Path::new(path).parent() {
        Some(dir) => dir.to_string_lossy().to_string(),
        None => ".".to_string(),
    }
}

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
