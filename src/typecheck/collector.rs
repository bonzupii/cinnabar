use crate::ast::Item;
use crate::ast::ItemKind;
use crate::ast::Spanned;
use crate::ast::Type;
use crate::ast::TypeKind;
use crate::typecheck::TypeChecker;
use crate::typecheck::TypeError;
use std::collections::HashMap;

impl TypeChecker {
    pub fn register_builtins(&mut self) {
        let bool_ty = Type::Named("Bool".to_string());
        let unit_ty = Type::Named("Unit".to_string());
        let u8_ty = Type::Named("U8".to_string());
        let u32_ty = Type::Named("U32".to_string());

        self.functions.insert("bool_and".to_string(), (vec![], vec![bool_ty.clone(), bool_ty.clone()], bool_ty));
        self.functions.insert("Unit".to_string(), (vec![], vec![], unit_ty));
        self.functions.insert("U32.from_u8".to_string(), (vec![], vec![u8_ty], u32_ty));
    }

    pub fn collect_module_local_types(&self, items: &[Spanned<Item>]) -> Vec<String> {
        let mut local_types = Vec::new();
        let mut idx = 0;
        while idx < items.len() {
            if let ItemKind::TypeDecl { name, .. } = &items[idx].node.kind {
                local_types.push(name.clone());
            }
            idx += 1;
        }
        local_types
    }

    pub fn collect_items(&mut self, items: &[Spanned<Item>]) {
        let local_type_names = self.collect_module_local_types(items);

        let mut idx = 0;
        while idx < items.len() {
            let item = &items[idx];
            match &item.node.kind {
                ItemKind::Function {
                    name,
                    generics,
                    params,
                    return_ty,
                    ..
                } => {
                    let mut param_types = Vec::new();
                    let mut p_idx = 0;
                    while p_idx < params.len() {
                        let qual_p_ty = self.qualify_type(&params[p_idx].ty.node, &local_type_names);
                        param_types.push(qual_p_ty);
                        p_idx += 1;
                    }
                    let ret_ty = match return_ty {
                        Some(spanned_type) => self.qualify_type(&spanned_type.node, &local_type_names),
                        None => Type::Named("Unit".to_string()),
                    };

                    let full_name = if self.module_stack.is_empty() {
                        name.clone()
                    } else {
                        format!("{}.{}", self.module_stack.join("."), name)
                    };

                    let gen_names: Vec<String> = generics.iter().map(|g| g.name.clone()).collect();

                    self.functions.insert(full_name, (gen_names.clone(), param_types.clone(), ret_ty.clone()));
                    self.functions.insert(name.clone(), (gen_names, param_types, ret_ty));
                }
                ItemKind::TypeDecl { name, kind, .. } => {
                    let full_name = if self.module_stack.is_empty() {
                        name.clone()
                    } else {
                        format!("{}.{}", self.module_stack.join("."), name)
                    };

                    let ret_path_ty = if self.module_stack.is_empty() {
                        Type::Named(name.clone())
                    } else {
                        let mut path = self.module_stack.clone();
                        path.push(name.clone());
                        Type::Path(path)
                    };

                    match kind {
                        TypeKind::Struct(fields) => {
                            let mut field_map = HashMap::new();
                            let mut f_idx = 0;
                            while f_idx < fields.len() {
                                let qual_f_ty = self.qualify_type(&fields[f_idx].ty.node, &local_type_names);
                                field_map.insert(fields[f_idx].name.clone(), qual_f_ty);
                                f_idx += 1;
                            }
                            self.structs.insert(full_name.clone(), field_map.clone());
                            self.structs.insert(name.clone(), field_map);
                        }
                        TypeKind::Enum(variants) => {
                            let mut v_idx = 0;
                            while v_idx < variants.len() {
                                let variant = &variants[v_idx];
                                if variant.name == "None" || variant.name == "Some" || variant.name == "Ok" || variant.name == "Err" {
                                    v_idx += 1;
                                    continue;
                                }

                                let mut param_types = Vec::new();
                                let mut t_idx = 0;
                                while t_idx < variant.types.len() {
                                    let qual_v_ty = self.qualify_type(&variant.types[t_idx].node, &local_type_names);
                                    param_types.push(qual_v_ty);
                                    t_idx += 1;
                                }

                                self.functions.insert(variant.name.clone(), (vec![], param_types.clone(), ret_path_ty.clone()));
                                if !self.module_stack.is_empty() {
                                    let qual_variant_name = format!("{}.{}", self.module_stack.join("."), variant.name);
                                    self.functions.insert(qual_variant_name, (vec![], param_types, ret_path_ty.clone()));
                                }
                                v_idx += 1;
                            }
                        }
                        TypeKind::Native | TypeKind::Unit => {}
                    }
                }
                ItemKind::Module { name, items: child_items, .. } => {
                    self.module_stack.push(name.clone());
                    self.collect_items(child_items);
                    self.module_stack.pop();
                }
                ItemKind::Trait { methods, .. } | ItemKind::Impl { methods, .. } => {
                    self.collect_items(methods);
                }
                ItemKind::Const { name, ty, .. } => {
                    self.declare_variable(name.clone(), ty.node.clone(), false);
                }
                ItemKind::Use { .. } => {}
            }
            idx += 1;
        }
    }

    pub fn resolve_uses(&mut self, items: &[Spanned<Item>]) {
        let mut idx = 0;
        while idx < items.len() {
            let item = &items[idx];
            match &item.node.kind {
                ItemKind::Use { path, alias, .. } => {
                    let full_path = path.join(".");
                    let local_name = match alias {
                        Some(alias_name) => alias_name.clone(),
                        None => match path.last() {
                            Some(last_seg) => last_seg.clone(),
                            None => "".to_string(),
                        },
                    };
                    if !local_name.is_empty() {
                        if let Some(func_sig) = self.functions.get(&full_path).cloned() {
                            self.functions.insert(local_name.clone(), func_sig);
                        }
                        if let Some(struct_def) = self.structs.get(&full_path).cloned() {
                            self.structs.insert(local_name, struct_def);
                        }
                    }
                }
                ItemKind::Module { items: child_items, .. } => {
                    self.resolve_uses(child_items);
                }
                ItemKind::Trait { methods, .. } | ItemKind::Impl { methods, .. } => {
                    self.resolve_uses(methods);
                }
                ItemKind::Function { .. } | ItemKind::TypeDecl { .. } | ItemKind::Const { .. } => {}
            }
            idx += 1;
        }
    }

    pub fn check_program(&mut self, items: &[Spanned<Item>]) -> Result<(), Vec<TypeError>> {
        self.collect_items(items);
        self.resolve_uses(items);

        let mut idx = 0;
        while idx < items.len() {
            let item = &items[idx];
            self.check_item(item);
            idx += 1;
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }
}
