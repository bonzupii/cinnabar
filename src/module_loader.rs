use crate::ast::Item;
use crate::ast::ItemKind;
use crate::ast::Spanned;
use crate::lexer::Lexer;
use crate::lexer::Span;
use crate::parser::Parser;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ModuleLoaderError {
    pub message: String,
    pub span: Span,
}

pub struct ModuleLoader {
    base_dir: PathBuf,
    loaded_paths: HashSet<PathBuf>,
}

impl ModuleLoader {
    pub fn new(root_file_path: &Path) -> Self {
        let base_dir = match root_file_path.parent() {
            Some(parent_dir) => parent_dir.to_path_buf(),
            None => PathBuf::from("."),
        };

        let mut loaded_paths = HashSet::new();
        let canonical_root = match root_file_path.canonicalize() {
            Ok(path) => path,
            Err(_) => root_file_path.to_path_buf(),
        };
        loaded_paths.insert(canonical_root);

        Self {
            base_dir,
            loaded_paths,
        }
    }

    pub fn load_external_modules(&mut self, items: &mut Vec<Spanned<Item>>) -> Result<(), ModuleLoaderError> {
        let mut required_modules = Vec::new();
        self.collect_required_modules(items, &mut required_modules);

        let mut req_idx = 0;
        while req_idx < required_modules.len() {
            let mod_name = &required_modules[req_idx];
            if let Some(loaded_module_item) = self.try_load_module_file(mod_name)? {
                items.push(loaded_module_item);
            }
            req_idx += 1;
        }

        Ok(())
    }

    fn collect_required_modules(&self, items: &[Spanned<Item>], required_modules: &mut Vec<String>) {
        let mut idx = 0;
        while idx < items.len() {
            let item = &items[idx];
            match &item.node.kind {
                ItemKind::Use { path, .. } => {
                    if !path.is_empty() {
                        let root_mod = &path[0];
                        if !self.has_module_declaration(items, root_mod) && !required_modules.contains(root_mod) {
                            required_modules.push(root_mod.clone());
                        }
                    }
                }
                ItemKind::Module { items: child_items, .. } => {
                    self.collect_required_modules(child_items, required_modules);
                }
                ItemKind::Const { ty, .. } => {
                    self.inspect_type(ty, items, required_modules);
                }
                ItemKind::TypeDecl { kind, .. } => {
                    match kind {
                        crate::ast::TypeKind::Struct(fields) => {
                            let mut f_idx = 0;
                            while f_idx < fields.len() {
                                self.inspect_type(&fields[f_idx].ty, items, required_modules);
                                f_idx += 1;
                            }
                        }
                        crate::ast::TypeKind::Enum(variants) => {
                            let mut v_idx = 0;
                            while v_idx < variants.len() {
                                let variant = &variants[v_idx];
                                let mut t_idx = 0;
                                while t_idx < variant.types.len() {
                                    self.inspect_type(&variant.types[t_idx], items, required_modules);
                                    t_idx += 1;
                                }
                                v_idx += 1;
                            }
                        }
                        crate::ast::TypeKind::Native | crate::ast::TypeKind::Unit => {}
                    }
                }
                ItemKind::Function { params, return_ty, .. } => {
                    let mut p_idx = 0;
                    while p_idx < params.len() {
                        self.inspect_type(&params[p_idx].ty, items, required_modules);
                        p_idx += 1;
                    }
                    if let Some(ret_type) = return_ty {
                        self.inspect_type(ret_type, items, required_modules);
                    }
                }
                ItemKind::Trait { methods, .. } => {
                    self.collect_required_modules(methods, required_modules);
                }
                ItemKind::Impl { target_type, methods, .. } => {
                    self.inspect_type(target_type, items, required_modules);
                    self.collect_required_modules(methods, required_modules);
                }
            }
            idx += 1;
        }
    }

    fn inspect_type(&self, ty: &Spanned<crate::ast::Type>, items: &[Spanned<Item>], required_modules: &mut Vec<String>) {
        match &ty.node {
            crate::ast::Type::Path(path) | crate::ast::Type::GenericPath(path, _) => {
                if !path.is_empty() {
                    let root_mod = &path[0];
                    if !self.has_module_declaration(items, root_mod) && !required_modules.contains(root_mod) {
                        required_modules.push(root_mod.clone());
                    }
                }
            }
            crate::ast::Type::Generic(_, args) => {
                let mut a_idx = 0;
                while a_idx < args.len() {
                    self.inspect_type(&args[a_idx], items, required_modules);
                    a_idx += 1;
                }
            }
            crate::ast::Type::Array(inner, _) | crate::ast::Type::Slice(inner) | crate::ast::Type::Ref(inner) | crate::ast::Type::RefMut(inner) => {
                self.inspect_type(inner, items, required_modules);
            }
            crate::ast::Type::Named(_) => {}
        }
    }

    fn has_module_declaration(&self, items: &[Spanned<Item>], mod_name: &str) -> bool {
        let mut idx = 0;
        while idx < items.len() {
            if let ItemKind::Module { name, .. } = &items[idx].node.kind
                && name == mod_name
            {
                return true;
            }
            idx += 1;
        }
        false
    }

    fn try_load_module_file(&mut self, mod_name: &str) -> Result<Option<Spanned<Item>>, ModuleLoaderError> {
        let pascal_filename = format!("{}.cnb", mod_name);
        let snake_filename = format!("{}.cnb", self.to_snake_case(mod_name));

        let candidate_paths = [
            self.base_dir.join(&pascal_filename),
            self.base_dir.join(&snake_filename),
            self.base_dir.join(mod_name).join("mod.cnb"),
        ];

        let mut found_path: Option<PathBuf> = None;
        let mut path_idx = 0;
        while path_idx < candidate_paths.len() {
            let path_candidate = &candidate_paths[path_idx];
            if path_candidate.exists() && path_candidate.is_file() {
                found_path = Some(path_candidate.clone());
                break;
            }
            path_idx += 1;
        }

        let file_path = match found_path {
            Some(path_val) => path_val,
            None => return Ok(None),
        };

        let canonical_path = match file_path.canonicalize() {
            Ok(path) => path,
            Err(_) => file_path.clone(),
        };

        if self.loaded_paths.contains(&canonical_path) {
            return Ok(None);
        }
        self.loaded_paths.insert(canonical_path);

        let source = match fs::read_to_string(&file_path) {
            Ok(content) => content,
            Err(io_err) => return Err(ModuleLoaderError {
                message: format!("Failed to read module file '{}': {}", file_path.display(), io_err),
                span: Span::new(0, 0, 1, 1),
            }),
        };

        let mut lexer = Lexer::new(&source);
        let tokens = match lexer.tokenize() {
            Ok(toks) => toks,
            Err(lex_err) => return Err(ModuleLoaderError {
                message: format!("Lexical error in external module file '{}': {}", file_path.display(), lex_err.message),
                span: lex_err.span,
            }),
        };

        let mut parser = Parser::new(&tokens);
        let mut child_items = match parser.parse_program() {
            Ok(parsed_ast) => parsed_ast,
            Err(parse_err) => return Err(ModuleLoaderError {
                message: format!("Syntax error in external module file '{}': {}", file_path.display(), parse_err.message),
                span: parse_err.span,
            }),
        };

        self.load_external_modules(&mut child_items)?;

        let dummy_span = Span::new(0, 0, 1, 1);
        let module_item = Item {
            doc: None,
            kind: ItemKind::Module {
                is_pub: true,
                name: mod_name.to_string(),
                items: child_items,
            },
        };

        Ok(Some(Spanned::new(module_item, dummy_span)))
    }

    fn to_snake_case(&self, pascal_str: &str) -> String {
        let mut snake = String::new();
        let mut idx = 0;
        let chars: Vec<char> = pascal_str.chars().collect();
        while idx < chars.len() {
            let ch = chars[idx];
            if ch.is_uppercase() {
                if idx > 0 {
                    snake.push('_');
                }
                for lower_ch in ch.to_lowercase() {
                    snake.push(lower_ch);
                }
            } else {
                snake.push(ch);
            }
            idx += 1;
        }
        snake
    }
}