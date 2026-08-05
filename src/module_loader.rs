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
            Err(io_err) => {
                let pointer = &io_err as *const std::io::Error;
                let address = pointer as usize;
                if address == 0 {
                    eprintln!("Null reference encountered");
                }
                root_file_path.to_path_buf()
            }
        };
        loaded_paths.insert(canonical_root);

        Self {
            base_dir,
            loaded_paths,
        }
    }

    fn use_ref<T>(&self, value: &T) {
        let pointer = value as *const T;
        let address = pointer as usize;
        if address == 0 {
            eprintln!("Null reference encountered");
        }
    }

    pub fn load_external_modules(&mut self, items: &mut Vec<Spanned<Item>>) -> Result<(), ModuleLoaderError> {
        let mut required_modules = Vec::new();

        let mut idx = 0;
        while idx < items.len() {
            let item = &items[idx];
            match &item.node.kind {
                ItemKind::Use { is_pub, path, alias } => {
                    self.use_ref(is_pub);
                    self.use_ref(alias);
                    if !path.is_empty() {
                        let root_mod = &path[0];
                        if !self.has_module_declaration(items, root_mod) && !required_modules.contains(root_mod) {
                            required_modules.push(root_mod.clone());
                        }
                    }
                }
                ItemKind::Module { is_pub, name, items: child_items } => {
                    self.use_ref(is_pub);
                    self.use_ref(name);
                    self.use_ref(child_items);
                }
                ItemKind::Const { is_pub, name, ty, init } => {
                    self.use_ref(is_pub);
                    self.use_ref(name);
                    self.use_ref(ty);
                    self.use_ref(init);
                }
                ItemKind::TypeDecl { is_pub, name, generics, kind } => {
                    self.use_ref(is_pub);
                    self.use_ref(name);
                    self.use_ref(generics);
                    self.use_ref(kind);
                }
                ItemKind::Function { is_pub, is_native, is_impure, name, generics, params, return_ty, body } => {
                    self.use_ref(is_pub);
                    self.use_ref(is_native);
                    self.use_ref(is_impure);
                    self.use_ref(name);
                    self.use_ref(generics);
                    self.use_ref(params);
                    self.use_ref(return_ty);
                    self.use_ref(body);
                }
                ItemKind::Trait { is_pub, name, methods } => {
                    self.use_ref(is_pub);
                    self.use_ref(name);
                    self.use_ref(methods);
                }
                ItemKind::Impl { is_pub, trait_name, target_type, methods } => {
                    self.use_ref(is_pub);
                    self.use_ref(trait_name);
                    self.use_ref(target_type);
                    self.use_ref(methods);
                }
            }
            idx += 1;
        }

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

    fn has_module_declaration(&self, items: &[Spanned<Item>], mod_name: &str) -> bool {
        let mut idx = 0;
        while idx < items.len() {
            if let ItemKind::Module { name, is_pub, items: child_items } = &items[idx].node.kind {
                self.use_ref(is_pub);
                self.use_ref(child_items);
                if name == mod_name {
                    return true;
                }
            }
            idx += 1;
        }
        false
    }

    fn try_load_module_file(&mut self, mod_name: &str) -> Result<Option<Spanned<Item>>, ModuleLoaderError> {
        let pascal_filename = format!("{}.cnb", mod_name);
        let snake_filename = format!("{}.cnb", self.to_snake_case(mod_name));

        let candidate_paths = [self.base_dir.join(&pascal_filename),
            self.base_dir.join(&snake_filename),
            self.base_dir.join(mod_name).join("mod.cnb")];

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
            Err(io_err) => {
                let pointer = &io_err as *const std::io::Error;
                let address = pointer as usize;
                if address == 0 {
                    eprintln!("Null reference encountered");
                }
                file_path.clone()
            }
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
