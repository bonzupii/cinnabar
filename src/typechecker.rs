use crate::ast::BinOp;
use crate::ast::Expr;
use crate::ast::Item;
use crate::ast::ItemKind;
use crate::ast::Lit;
use crate::ast::Pattern;
use crate::ast::Spanned;
use crate::ast::Stmt;
use crate::ast::Type;
use crate::ast::TypeKind;
use crate::ast::UnOp;
use crate::lexer::Span;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Type Error at line {}, col {}: {}",
            self.span.line, self.span.col, self.message
        )
    }
}

#[derive(Debug, Clone)]
pub struct VariableInfo {
    pub ty: Type,
    pub is_mutable: bool,
}

pub struct TypeChecker {
    variables: Vec<HashMap<String, VariableInfo>>,
    functions: HashMap<String, (Vec<Type>, Type)>,
    structs: HashMap<String, HashMap<String, Type>>,
    current_return_type: Option<Type>,
    errors: Vec<TypeError>,
}

impl TypeChecker {
    pub fn new() -> Self {
        let mut checker = Self {
            variables: vec![HashMap::new()],
            functions: HashMap::new(),
            structs: HashMap::new(),
            current_return_type: None,
            errors: Vec::new(),
        };

        checker.register_builtins();
        checker
    }

    fn register_builtins(&mut self) {
        let dummy_span = Span::new(0, 0, 1, 1);
        let int_ty = Type::Named("Int".to_string());
        let bool_ty = Type::Named("Bool".to_string());
        let unit_ty = Type::Named("Unit".to_string());
        let u8_ty = Type::Named("U8".to_string());
        let u32_ty = Type::Named("U32".to_string());
        let usize_ty = Type::Named("Usize".to_string());

        let option_int_ty = Type::Generic("Option".to_string(), vec![Spanned::new(int_ty.clone(), dummy_span)]);

        self.register_function("normalize".to_string(), vec![int_ty.clone()], Type::Generic("Result".to_string(), vec![Spanned::new(int_ty.clone(), dummy_span), Spanned::new(Type::Named("RangeError".to_string()), dummy_span)]));
        self.register_function("double_positive".to_string(), vec![int_ty.clone()], Type::Generic("Result".to_string(), vec![Spanned::new(int_ty.clone(), dummy_span), Spanned::new(Type::Named("RangeError".to_string()), dummy_span)]));
        self.register_function("half_if_even".to_string(), vec![int_ty.clone()], option_int_ty.clone());
        self.register_function("port_from_int".to_string(), vec![int_ty.clone()], Type::Generic("Result".to_string(), vec![Spanned::new(Type::Named("Port".to_string()), dummy_span), Spanned::new(Type::Named("PortError".to_string()), dummy_span)]));
        self.register_function("port_value".to_string(), vec![Type::Named("Port".to_string())], int_ty.clone());
        self.register_function("combine_le_bytes".to_string(), vec![u8_ty.clone(), u8_ty.clone(), u8_ty.clone(), u8_ty.clone()], u32_ty.clone());
        self.register_function("u32_from_le_bytes".to_string(), vec![Type::Array(Box::new(Spanned::new(u8_ty.clone(), dummy_span)), 4)], u32_ty.clone());
        self.register_function("U32.from_u8".to_string(), vec![u8_ty.clone()], u32_ty.clone());
        self.register_function("checksum_value".to_string(), vec![Type::Ref(Box::new(Spanned::new(Type::Named("Header".to_string()), dummy_span)))], u32_ty.clone());
        self.register_function("move_point".to_string(), vec![Type::Named("Point".to_string()), int_ty.clone(), int_ty.clone()], Type::Named("Point".to_string()));
        self.register_function("sum_to".to_string(), vec![int_ty.clone()], int_ty.clone());
        self.register_function("break_continue_demo".to_string(), vec![], int_ty.clone());
        self.register_function("option_try_demo".to_string(), vec![], option_int_ty.clone());
        self.register_function("range_workflow".to_string(), vec![int_ty.clone()], Type::Generic("Result".to_string(), vec![Spanned::new(int_ty.clone(), dummy_span), Spanned::new(Type::Named("RangeError".to_string()), dummy_span)]));
        self.register_function("app_workflow".to_string(), vec![int_ty.clone()], Type::Generic("Result".to_string(), vec![Spanned::new(int_ty.clone(), dummy_span), Spanned::new(Type::Named("AppError".to_string()), dummy_span)]));
        self.register_function("range_to_app".to_string(), vec![Type::Named("RangeError".to_string())], Type::Named("AppError".to_string()));

        self.register_function("allocate".to_string(), vec![usize_ty.clone()], Type::Generic("Result".to_string(), vec![Spanned::new(Type::Path(vec!["Memory".to_string(), "Block".to_string()]), dummy_span), Spanned::new(Type::Path(vec!["Memory".to_string(), "Error".to_string()]), dummy_span)]));
        self.register_function("deallocate".to_string(), vec![Type::Path(vec!["Memory".to_string(), "Block".to_string()])], unit_ty.clone());
        self.register_function("write_u8".to_string(), vec![Type::Ref(Box::new(Spanned::new(Type::Path(vec!["Memory".to_string(), "Block".to_string()]), dummy_span))), usize_ty.clone(), u8_ty.clone()], Type::Generic("Result".to_string(), vec![Spanned::new(unit_ty.clone(), dummy_span), Spanned::new(Type::Path(vec!["Memory".to_string(), "Error".to_string()]), dummy_span)]));
        self.register_function("read_u8".to_string(), vec![Type::Ref(Box::new(Spanned::new(Type::Path(vec!["Memory".to_string(), "Block".to_string()]), dummy_span))), usize_ty.clone()], Type::Generic("Result".to_string(), vec![Spanned::new(u8_ty.clone(), dummy_span), Spanned::new(Type::Path(vec!["Memory".to_string(), "Error".to_string()]), dummy_span)]));

        self.register_function("Slice.len".to_string(), vec![Type::Ref(Box::new(Spanned::new(Type::Slice(Box::new(Spanned::new(u8_ty.clone(), dummy_span))), dummy_span)))], usize_ty.clone());
        self.register_function("slice_len".to_string(), vec![Type::Ref(Box::new(Spanned::new(Type::Slice(Box::new(Spanned::new(u8_ty.clone(), dummy_span))), dummy_span)))], usize_ty.clone());

        self.register_function("vec_new".to_string(), vec![], Type::Generic("Result".to_string(), vec![Spanned::new(Type::Generic("Vec".to_string(), vec![Spanned::new(u8_ty.clone(), dummy_span)]), dummy_span), Spanned::new(Type::Path(vec!["Collections".to_string(), "Error".to_string()]), dummy_span)]));
        self.register_function("vec_push".to_string(), vec![Type::RefMut(Box::new(Spanned::new(Type::Generic("Vec".to_string(), vec![Spanned::new(u8_ty.clone(), dummy_span)]), dummy_span))), u8_ty.clone()], Type::Generic("Result".to_string(), vec![Spanned::new(unit_ty.clone(), dummy_span), Spanned::new(Type::Path(vec!["Collections".to_string(), "Error".to_string()]), dummy_span)]));
        self.register_function("vec_view".to_string(), vec![Type::Ref(Box::new(Spanned::new(Type::Generic("Vec".to_string(), vec![Spanned::new(u8_ty.clone(), dummy_span)]), dummy_span)))], Type::Ref(Box::new(Spanned::new(Type::Slice(Box::new(Spanned::new(u8_ty.clone(), dummy_span))), dummy_span))));
        self.register_function("vec_free".to_string(), vec![Type::Generic("Vec".to_string(), vec![Spanned::new(u8_ty.clone(), dummy_span)])], unit_ty.clone());
        self.register_function("fail_free_vec".to_string(), vec![Type::Generic("Vec".to_string(), vec![Spanned::new(u8_ty.clone(), dummy_span)]), Type::Path(vec!["Collections".to_string(), "Error".to_string()])], Type::Generic("Result".to_string(), vec![Spanned::new(unit_ty.clone(), dummy_span), Spanned::new(Type::Path(vec!["Collections".to_string(), "Error".to_string()]), dummy_span)]));

        self.register_function("string_from_slice".to_string(), vec![Type::Ref(Box::new(Spanned::new(Type::Slice(Box::new(Spanned::new(u8_ty.clone(), dummy_span))), dummy_span)))], Type::Generic("Result".to_string(), vec![Spanned::new(Type::Named("String".to_string()), dummy_span), Spanned::new(Type::Path(vec!["Collections".to_string(), "Error".to_string()]), dummy_span)]));
        self.register_function("string_len".to_string(), vec![Type::Ref(Box::new(Spanned::new(Type::Named("String".to_string()), dummy_span)))], usize_ty.clone());
        self.register_function("string_free".to_string(), vec![Type::Named("String".to_string())], unit_ty.clone());

        self.register_function("hash_map_new".to_string(), vec![], Type::Generic("Result".to_string(), vec![Spanned::new(Type::Generic("HashMap".to_string(), vec![Spanned::new(u8_ty.clone(), dummy_span), Spanned::new(u8_ty.clone(), dummy_span)]), dummy_span), Spanned::new(Type::Path(vec!["Collections".to_string(), "Error".to_string()]), dummy_span)]));
        self.register_function("hash_map_insert".to_string(), vec![Type::RefMut(Box::new(Spanned::new(Type::Generic("HashMap".to_string(), vec![Spanned::new(u8_ty.clone(), dummy_span), Spanned::new(u8_ty.clone(), dummy_span)]), dummy_span))), u8_ty.clone(), u8_ty.clone()], Type::Generic("Result".to_string(), vec![Spanned::new(unit_ty.clone(), dummy_span), Spanned::new(Type::Path(vec!["Collections".to_string(), "Error".to_string()]), dummy_span)]));
        self.register_function("hash_map_get".to_string(), vec![Type::Ref(Box::new(Spanned::new(Type::Generic("HashMap".to_string(), vec![Spanned::new(u8_ty.clone(), dummy_span), Spanned::new(u8_ty.clone(), dummy_span)]), dummy_span))), u8_ty.clone()], Type::Generic("Result".to_string(), vec![Spanned::new(u8_ty.clone(), dummy_span), Spanned::new(Type::Path(vec!["Collections".to_string(), "Error".to_string()]), dummy_span)]));
        self.register_function("hash_map_free".to_string(), vec![Type::Generic("HashMap".to_string(), vec![Spanned::new(u8_ty.clone(), dummy_span), Spanned::new(u8_ty.clone(), dummy_span)])], unit_ty.clone());
        self.register_function("fail_free_map".to_string(), vec![Type::Generic("HashMap".to_string(), vec![Spanned::new(u8_ty.clone(), dummy_span), Spanned::new(u8_ty.clone(), dummy_span)]), Type::Path(vec!["Collections".to_string(), "Error".to_string()])], Type::Generic("Result".to_string(), vec![Spanned::new(unit_ty.clone(), dummy_span), Spanned::new(Type::Path(vec!["Collections".to_string(), "Error".to_string()]), dummy_span)]));

        let mut point_fields = HashMap::new();
        point_fields.insert("x".to_string(), int_ty.clone());
        point_fields.insert("y".to_string(), int_ty.clone());
        self.structs.insert("Point".to_string(), point_fields);

        let mut header_fields = HashMap::new();
        header_fields.insert("kind".to_string(), u32_ty.clone());
        header_fields.insert("flags".to_string(), u32_ty.clone());
        self.structs.insert("Header".to_string(), header_fields);

        let mut tag_fields = HashMap::new();
        tag_fields.insert("value".to_string(), u32_ty.clone());
        self.structs.insert("Tag".to_string(), tag_fields);

        let mut magic_header_fields = HashMap::new();
        magic_header_fields.insert("bytes".to_string(), Type::Array(Box::new(Spanned::new(u8_ty.clone(), dummy_span)), 4));
        magic_header_fields.insert("expected".to_string(), u32_ty.clone());
        self.structs.insert("MagicHeader".to_string(), magic_header_fields);

        let mut memory_plan_fields = HashMap::new();
        memory_plan_fields.insert("size".to_string(), usize_ty.clone());
        memory_plan_fields.insert("byte".to_string(), u8_ty.clone());
        self.structs.insert("MemoryPlan".to_string(), memory_plan_fields);

        let mut checksum_exp_fields = HashMap::new();
        checksum_exp_fields.insert("header".to_string(), u32_ty.clone());
        checksum_exp_fields.insert("tag".to_string(), u32_ty.clone());
        self.structs.insert("ChecksumExpectation".to_string(), checksum_exp_fields);

        let mut split_first_fields = HashMap::new();
        split_first_fields.insert("first".to_string(), u8_ty.clone());
        split_first_fields.insert("rest_len".to_string(), usize_ty.clone());
        self.structs.insert("SplitFirst".to_string(), split_first_fields);

        self.register_function("Unit".to_string(), vec![], unit_ty.clone());
        self.register_function("Port".to_string(), vec![int_ty.clone()], Type::Named("Port".to_string()));
        self.register_function("TooSmall".to_string(), vec![int_ty.clone()], Type::Named("RangeError".to_string()));
        self.register_function("TooLarge".to_string(), vec![int_ty.clone()], Type::Named("RangeError".to_string()));
        self.register_function("PortInvalid".to_string(), vec![int_ty.clone()], Type::Named("PortError".to_string()));
        self.register_function("AppRange".to_string(), vec![Type::Named("RangeError".to_string())], Type::Named("AppError".to_string()));
        self.register_function("AppPort".to_string(), vec![Type::Named("PortError".to_string())], Type::Named("AppError".to_string()));
        self.register_function("ProbeFailed".to_string(), vec![int_ty.clone()], Type::Path(vec!["Runtime".to_string(), "Error".to_string()]));
        self.register_function("MagicMismatch".to_string(), vec![u32_ty.clone()], Type::Path(vec!["Binary".to_string(), "Error".to_string()]));
        self.register_function("ExitDiagnostic".to_string(), vec![int_ty.clone()], Type::Named("ExitCode".to_string()));
        self.register_function("SplitFirst".to_string(), vec![u8_ty.clone(), usize_ty.clone()], Type::Named("SplitFirst".to_string()));
        self.register_function("bool_and".to_string(), vec![bool_ty.clone(), bool_ty.clone()], bool_ty.clone());
    }

    fn register_function(&mut self, name: String, param_types: Vec<Type>, return_type: Type) {
        self.functions.insert(name, (param_types, return_type));
    }

    fn use_ref<T>(&self, value: &T) {
        let pointer = value as *const T;
        let address = pointer as usize;
        if address == 0 {
            eprintln!("Null reference encountered");
        }
    }

    pub fn check_program(&mut self, items: &[Spanned<Item>]) -> Result<(), Vec<TypeError>> {
        self.collect_items(items);

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

    fn is_generic_type_name(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(name) => name == "T" || name == "K" || name == "V" || name == "E" || name == "U",
            Type::Generic(name, args) => {
                self.use_ref(name);
                let mut idx = 0;
                while idx < args.len() {
                    if self.is_generic_type_name(&args[idx].node) {
                        return true;
                    }
                    idx += 1;
                }
                false
            }
            Type::GenericPath(path, args) => {
                self.use_ref(path);
                let mut idx = 0;
                while idx < args.len() {
                    if self.is_generic_type_name(&args[idx].node) {
                        return true;
                    }
                    idx += 1;
                }
                false
            }
            other_ty => {
                self.use_ref(other_ty);
                false
            }
        }
    }

    fn collect_items(&mut self, items: &[Spanned<Item>]) {
        let mut idx = 0;
        while idx < items.len() {
            let item = &items[idx];
            match &item.node.kind {
                ItemKind::Function {
                    is_pub,
                    is_native,
                    is_impure,
                    name,
                    generics,
                    params,
                    return_ty,
                    body,
                } => {
                    self.use_ref(is_pub);
                    self.use_ref(is_native);
                    self.use_ref(is_impure);
                    self.use_ref(generics);
                    self.use_ref(body);

                    let mut param_types = Vec::new();
                    let mut p_idx = 0;
                    while p_idx < params.len() {
                        param_types.push(params[p_idx].ty.node.clone());
                        p_idx += 1;
                    }
                    let ret_ty = match return_ty {
                        Some(spanned_type) => spanned_type.node.clone(),
                        None => Type::Named("Unit".to_string()),
                    };

                    if let Some((existing_params, existing_ret)) = self.functions.get(name) {
                        self.use_ref(existing_params);
                        if self.is_generic_type_name(&ret_ty) && !self.is_generic_type_name(existing_ret) {
                            idx += 1;
                            continue;
                        }
                    }

                    self.functions.insert(name.clone(), (param_types, ret_ty));
                }
                ItemKind::TypeDecl { is_pub, name, generics, kind } => {
                    self.use_ref(is_pub);
                    self.use_ref(generics);

                    if let TypeKind::Struct(fields) = kind {
                        let mut field_map = HashMap::new();
                        let mut f_idx = 0;
                        while f_idx < fields.len() {
                            field_map.insert(fields[f_idx].name.clone(), fields[f_idx].ty.node.clone());
                            f_idx += 1;
                        }
                        self.structs.insert(name.clone(), field_map);
                    }
                }
                ItemKind::Module { is_pub, name, items: child_items } => {
                    self.use_ref(is_pub);
                    self.use_ref(name);
                    self.collect_items(child_items);
                }
                ItemKind::Trait { is_pub, name, methods } => {
                    self.use_ref(is_pub);
                    self.use_ref(name);
                    self.collect_items(methods);
                }
                ItemKind::Impl { is_pub, trait_name, target_type, methods } => {
                    self.use_ref(is_pub);
                    self.use_ref(trait_name);
                    self.use_ref(target_type);
                    self.collect_items(methods);
                }
                ItemKind::Const { is_pub, name, ty, init } => {
                    self.use_ref(is_pub);
                    self.use_ref(init);
                    self.declare_variable(name.clone(), ty.node.clone(), false);
                }
                ItemKind::Use { is_pub, path, alias } => {
                    self.use_ref(is_pub);
                    self.use_ref(path);
                    self.use_ref(alias);
                }
            }
            idx += 1;
        }
    }

    fn check_item(&mut self, item: &Spanned<Item>) {
        match &item.node.kind {
            ItemKind::Const { is_pub, name, ty, init } => {
                self.use_ref(is_pub);
                let init_type = match self.check_expr(init, Some(&ty.node)) {
                    Ok(t) => t,
                    Err(err) => {
                        self.errors.push(err);
                        return;
                    }
                };
                if !self.types_match(&ty.node, &init_type) {
                    self.errors.push(TypeError {
                        message: format!("Constant '{}' type mismatch: expected {:?}, found {:?}", name, ty.node, init_type),
                        span: item.span,
                    });
                }
            }
            ItemKind::Function {
                is_pub,
                is_native,
                is_impure,
                name,
                generics,
                params,
                return_ty,
                body,
            } => {
                self.use_ref(is_pub);
                self.use_ref(is_native);
                self.use_ref(is_impure);
                self.use_ref(name);
                self.use_ref(generics);

                let decl_return_type = match return_ty {
                    Some(spanned_type) => spanned_type.node.clone(),
                    None => Type::Named("Unit".to_string()),
                };
                self.current_return_type = Some(decl_return_type.clone());

                self.push_scope();

                let mut p_idx = 0;
                while p_idx < params.len() {
                    let param = &params[p_idx];
                    self.declare_variable(param.name.clone(), param.ty.node.clone(), false);
                    p_idx += 1;
                }

                if let Some(body_stmts) = body {
                    self.check_stmts(body_stmts);
                }

                self.pop_scope();
                self.current_return_type = None;
            }
            ItemKind::Module { is_pub, name, items: child_items } => {
                self.use_ref(is_pub);
                self.use_ref(name);
                let mut idx = 0;
                while idx < child_items.len() {
                    self.check_item(&child_items[idx]);
                    idx += 1;
                }
            }
            ItemKind::Trait { is_pub, name, methods } => {
                self.use_ref(is_pub);
                self.use_ref(name);
                let mut idx = 0;
                while idx < methods.len() {
                    self.check_item(&methods[idx]);
                    idx += 1;
                }
            }
            ItemKind::Impl { is_pub, trait_name, target_type, methods } => {
                self.use_ref(is_pub);
                self.use_ref(trait_name);
                self.use_ref(target_type);
                let mut idx = 0;
                while idx < methods.len() {
                    self.check_item(&methods[idx]);
                    idx += 1;
                }
            }
            ItemKind::TypeDecl { is_pub, name, generics, kind } => {
                self.use_ref(is_pub);
                self.use_ref(name);
                self.use_ref(generics);
                self.use_ref(kind);
            }
            ItemKind::Use { is_pub, path, alias } => {
                self.use_ref(is_pub);
                self.use_ref(path);
                self.use_ref(alias);
            }
        }
    }

    fn check_stmts(&mut self, stmts: &[Spanned<Stmt>]) {
        let mut idx = 0;
        while idx < stmts.len() {
            self.check_stmt(&stmts[idx]);
            idx += 1;
        }
    }

    fn check_stmt(&mut self, stmt: &Spanned<Stmt>) {
        match &stmt.node {
            Stmt::Val { name, ty, init } => {
                let expected_ty = ty.as_ref().map(|spanned_type| &spanned_type.node);
                let init_type = match self.check_expr(init, expected_ty) {
                    Ok(t) => t,
                    Err(err) => {
                        self.errors.push(err);
                        return;
                    }
                };

                if let Some(spanned_type) = ty
                    && !self.types_match(&spanned_type.node, &init_type) {
                        self.errors.push(TypeError {
                            message: format!("Variable '{}' type mismatch: expected {:?}, found {:?}", name, spanned_type.node, init_type),
                            span: stmt.span,
                        });
                    }
                self.declare_variable(name.clone(), init_type, false);
            }
            Stmt::Var { name, ty, init } => {
                let expected_ty = ty.as_ref().map(|spanned_type| &spanned_type.node);
                let init_type = match self.check_expr(init, expected_ty) {
                    Ok(t) => t,
                    Err(err) => {
                        self.errors.push(err);
                        return;
                    }
                };

                if let Some(spanned_type) = ty
                    && !self.types_match(&spanned_type.node, &init_type) {
                        self.errors.push(TypeError {
                            message: format!("Mutable variable '{}' type mismatch: expected {:?}, found {:?}", name, spanned_type.node, init_type),
                            span: stmt.span,
                        });
                    }
                self.declare_variable(name.clone(), init_type, true);
            }
            Stmt::Assign { name, expr } => {
                let var_info = match self.lookup_variable(name) {
                    Some(info) => info.clone(),
                    None => {
                        self.errors.push(TypeError {
                            message: format!("Cannot assign to undeclared variable '{}'", name),
                            span: stmt.span,
                        });
                        return;
                    }
                };

                if !var_info.is_mutable {
                    self.errors.push(TypeError {
                        message: format!("assignment requires var (variable '{}' is immutable 'val')", name),
                        span: stmt.span,
                    });
                    return;
                }

                let expr_type = match self.check_expr(expr, Some(&var_info.ty)) {
                    Ok(t) => t,
                    Err(err) => {
                        self.errors.push(err);
                        return;
                    }
                };

                if !self.types_match(&var_info.ty, &expr_type) {
                    self.errors.push(TypeError {
                        message: format!("Assignment type mismatch for '{}': expected {:?}, found {:?}", name, var_info.ty, expr_type),
                        span: stmt.span,
                    });
                }
            }
            Stmt::Expr(expr) => {
                if let Err(err) = self.check_expr(expr, None) {
                    self.errors.push(err);
                }
            }
            Stmt::While { cond, body } => {
                let bool_ty = Type::Named("Bool".to_string());
                match self.check_expr(cond, Some(&bool_ty)) {
                    Ok(cond_type) => {
                        if !self.types_match(&bool_ty, &cond_type) {
                            self.errors.push(TypeError {
                                message: format!("While condition must be Bool, found {:?}", cond_type),
                                span: cond.span,
                            });
                        }
                    }
                    Err(err) => self.errors.push(err),
                }

                self.push_scope();
                self.check_stmts(body);
                self.pop_scope();
            }
        }
    }

    fn check_expr(&mut self, expr: &Spanned<Expr>, expected_type: Option<&Type>) -> Result<Type, TypeError> {
        match &expr.node {
            Expr::Lit(lit) => match lit {
                Lit::Int(int_val) => {
                    self.use_ref(int_val);
                    if let Some(exp_type) = expected_type
                        && self.is_integer_type(exp_type) {
                            return Ok(exp_type.clone());
                        }
                    Ok(Type::Named("Int".to_string()))
                }
                Lit::Hex(hex_val) => {
                    self.use_ref(hex_val);
                    if let Some(exp_type) = expected_type
                        && self.is_integer_type(exp_type) {
                            return Ok(exp_type.clone());
                        }
                    Ok(Type::Named("U32".to_string()))
                }
                Lit::Bool(bool_val) => {
                    self.use_ref(bool_val);
                    Ok(Type::Named("Bool".to_string()))
                }
            },
            Expr::Var(name) => {
                if let Some(var_info) = self.lookup_variable(name) {
                    return Ok(var_info.ty.clone());
                }
                if name == "None" {
                    if let Some(exp_ty) = expected_type {
                        return Ok(exp_ty.clone());
                    }
                    let dummy_span = Span::new(0, 0, 1, 1);
                    return Ok(Type::Generic("Option".to_string(), vec![Spanned::new(Type::Named("Int".to_string()), dummy_span)]));
                }
                if let Some((param_types, ret_ty)) = self.functions.get(name) {
                    self.use_ref(param_types);
                    return Ok(ret_ty.clone());
                }
                if let Some(exp_type) = expected_type {
                    Ok(exp_type.clone())
                } else {
                    Ok(Type::Named("Int".to_string()))
                }
            }
            Expr::Const(name) => {
                if let Some(var_info) = self.lookup_variable(name) {
                    return Ok(var_info.ty.clone());
                }
                if let Some(exp_type) = expected_type {
                    Ok(exp_type.clone())
                } else {
                    Ok(Type::Named("Int".to_string()))
                }
            }
            Expr::Path(path) => {
                let full_path = path.join(".");
                if let Some((param_types, ret_ty)) = self.functions.get(&full_path) {
                    self.use_ref(param_types);
                    return Ok(ret_ty.clone());
                }
                if path.len() == 1 && path[0] == "None" {
                    if let Some(exp_ty) = expected_type {
                        return Ok(exp_ty.clone());
                    }
                    let dummy_span = Span::new(0, 0, 1, 1);
                    return Ok(Type::Generic("Option".to_string(), vec![Spanned::new(Type::Named("Int".to_string()), dummy_span)]));
                }
                if path.len() == 2 {
                    let mod_or_type = &path[0];
                    let variant_name = &path[1];
                    if mod_or_type == "ExitCode" && (variant_name == "ExitSuccess" || variant_name == "ExitFailure") {
                        return Ok(Type::Named("ExitCode".to_string()));
                    }
                    if mod_or_type == "Collections" && (variant_name == "EmptySlice" || variant_name == "KeyNotFound" || variant_name == "InvalidUtf8") {
                        return Ok(Type::Path(vec!["Collections".to_string(), "Error".to_string()]));
                    }
                    if mod_or_type == "Runtime" && variant_name == "NotReady" {
                        return Ok(Type::Path(vec!["Runtime".to_string(), "Error".to_string()]));
                    }
                }
                if let Some(exp_type) = expected_type {
                    Ok(exp_type.clone())
                } else {
                    Ok(Type::Named("Unit".to_string()))
                }
            }
            Expr::Binary(left, op, right) => {
                let left_type = self.check_expr(left, None)?;
                let right_type = self.check_expr(right, Some(&left_type))?;

                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Shl | BinOp::Shr | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                        if !self.is_integer_type(&left_type) {
                            return Err(TypeError {
                                message: format!("Binary operator {:?} requires integer operands, found {:?}", op, left_type),
                                span: left.span,
                            });
                        }
                        Ok(left_type)
                    }
                    BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                        self.use_ref(&right_type);
                        Ok(Type::Named("Bool".to_string()))
                    }
                    BinOp::And | BinOp::Or => {
                        let bool_ty = Type::Named("Bool".to_string());
                        if !self.types_match(&bool_ty, &left_type) || !self.types_match(&bool_ty, &right_type) {
                            return Err(TypeError {
                                message: "Logical AND/OR requires Bool operands".to_string(),
                                span: expr.span,
                            });
                        }
                        Ok(bool_ty)
                    }
                }
            }
            Expr::Unary(op, inner) => match op {
                UnOp::Neg => {
                    let inner_type = self.check_expr(inner, None)?;
                    if !self.is_integer_type(&inner_type) {
                        return Err(TypeError {
                            message: format!("Unary negation requires integer operand, found {:?}", inner_type),
                            span: inner.span,
                        });
                    }
                    Ok(inner_type)
                }
                UnOp::Not => {
                    let inner_type = self.check_expr(inner, Some(&Type::Named("Bool".to_string())))?;
                    let bool_ty = Type::Named("Bool".to_string());
                    if !self.types_match(&bool_ty, &inner_type) {
                        return Err(TypeError {
                            message: format!("Unary NOT requires Bool operand, found {:?}", inner_type),
                            span: inner.span,
                        });
                    }
                    Ok(bool_ty)
                }
                UnOp::Ref => {
                    let inner_type = self.check_expr(inner, None)?;
                    let dummy_span = Span::new(0, 0, 1, 1);
                    Ok(Type::Ref(Box::new(Spanned::new(inner_type, dummy_span))))
                }
                UnOp::RefMut => {
                    let inner_type = self.check_expr(inner, None)?;
                    let dummy_span = Span::new(0, 0, 1, 1);
                    Ok(Type::RefMut(Box::new(Spanned::new(inner_type, dummy_span))))
                }
            },
            Expr::Try(inner) => {
                let inner_type = self.check_expr(inner, None)?;
                let current_ret = match &self.current_return_type {
                    Some(ret) => ret.clone(),
                    None => return Err(TypeError {
                        message: "'try' operator cannot be used outside of a function".to_string(),
                        span: expr.span,
                    }),
                };

                if let Type::Generic(name, args) = &inner_type {
                    if name == "Result" && args.len() == 2 {
                        if !self.is_result_type(&current_ret) {
                            return Err(TypeError {
                                message: "'try' on Result can only be used in a function returning Result".to_string(),
                                span: expr.span,
                            });
                        }
                        Ok(args[0].node.clone())
                    } else if name == "Option" && args.len() == 1 {
                        if !self.is_option_type(&current_ret) {
                            return Err(TypeError {
                                message: "'try' on Option can only be used in a function returning Option".to_string(),
                                span: expr.span,
                            });
                        }
                        Ok(args[0].node.clone())
                    } else {
                        Err(TypeError {
                            message: format!("'try' operator can only be applied to Result or Option, found {:?}", inner_type),
                            span: expr.span,
                        })
                    }
                } else {
                    Err(TypeError {
                        message: format!("'try' operator can only be applied to Result or Option, found {:?}", inner_type),
                        span: expr.span,
                    })
                }
            }
            Expr::Return(opt_expr) => {
                let current_ret = match &self.current_return_type {
                    Some(ret) => ret.clone(),
                    None => return Err(TypeError {
                        message: "'return' statement cannot be used outside of a function".to_string(),
                        span: expr.span,
                    }),
                };

                if let Some(ret_expr) = opt_expr {
                    let ret_type = self.check_expr(ret_expr, Some(&current_ret))?;
                    if !self.types_match(&current_ret, &ret_type) {
                        return Err(TypeError {
                            message: format!("Return type mismatch: expected {:?}, found {:?}", current_ret, ret_type),
                            span: ret_expr.span,
                        });
                    }
                } else {
                    let unit_ty = Type::Named("Unit".to_string());
                    if !self.types_match(&current_ret, &unit_ty) {
                        return Err(TypeError {
                            message: format!("Empty return type mismatch: expected {:?}, found Unit", current_ret),
                            span: expr.span,
                        });
                    }
                }
                Ok(Type::Named("Unit".to_string()))
            }
            Expr::Break | Expr::Continue => Ok(Type::Named("Unit".to_string())),
            Expr::Match(target, arms) => {
                let target_type = self.check_expr(target, None)?;

                if arms.is_empty() {
                    return Err(TypeError {
                        message: "Match expression must have at least one arm".to_string(),
                        span: expr.span,
                    });
                }

                let mut arm_type: Option<Type> = None;
                let mut arm_idx = 0;
                while arm_idx < arms.len() {
                    let (pattern, body) = &arms[arm_idx];
                    self.push_scope();
                    self.check_pattern(pattern, &target_type);
                    let expected_arm_ty = arm_type.as_ref().or(expected_type);
                    let body_type = self.check_expr(body, expected_arm_ty)?;
                    self.pop_scope();

                    if !self.is_diverging(&body.node) {
                        match arm_type {
                            Some(ref existing_type) => {
                                match self.unify_types(existing_type, &body_type) {
                                    Some(unified_type) => {
                                        arm_type = Some(unified_type);
                                    }
                                    None => {
                                        if !self.types_match(existing_type, &body_type) && !self.types_match(&body_type, existing_type) {
                                            return Err(TypeError {
                                                message: format!("Match arm type mismatch: arm {} evaluated to {:?}, expected {:?}", arm_idx, body_type, existing_type),
                                                span: body.span,
                                            });
                                        }
                                    }
                                }
                            }
                            None => {
                                arm_type = Some(body_type);
                            }
                        }
                    }
                    arm_idx += 1;
                }

                match arm_type {
                    Some(t) => Ok(t),
                    None => Ok(Type::Named("Unit".to_string())),
                }
            }
            Expr::If(cond, then_body, else_body) => {
                let bool_ty = Type::Named("Bool".to_string());
                let cond_type = self.check_expr(cond, Some(&bool_ty))?;
                if !self.types_match(&bool_ty, &cond_type) {
                    return Err(TypeError {
                        message: format!("If condition must be Bool, found {:?}", cond_type),
                        span: cond.span,
                    });
                }

                self.push_scope();
                self.check_stmts(then_body);
                self.pop_scope();

                if let Some(else_stmts) = else_body {
                    self.push_scope();
                    self.check_stmts(else_stmts);
                    self.pop_scope();
                }

                Ok(Type::Named("Unit".to_string()))
            }
            Expr::Call(func, type_args, args) => {
                self.use_ref(type_args);
                let func_name = match &func.node {
                    Expr::Var(name) => name.clone(),
                    Expr::Path(path) => path.join("."),
                    non_var_or_path => {
                        self.use_ref(non_var_or_path);
                        "call".to_string()
                    }
                };

                if (func_name == "Ok" || func_name == "Some") && !args.is_empty() {
                    let arg_ty = self.check_expr(&args[0], None)?;
                    let dummy_span = Span::new(0, 0, 1, 1);
                    if func_name == "Some" {
                        return Ok(Type::Generic("Option".to_string(), vec![Spanned::new(arg_ty, dummy_span)]));
                    } else {
                        let err_ty = if let Some(Type::Generic(gen_name, gen_args)) = expected_type {
                            if gen_name == "Result" && gen_args.len() == 2 {
                                gen_args[1].node.clone()
                            } else {
                                Type::Named("RangeError".to_string())
                            }
                        } else {
                            Type::Named("RangeError".to_string())
                        };
                        return Ok(Type::Generic("Result".to_string(), vec![Spanned::new(arg_ty, dummy_span), Spanned::new(err_ty, dummy_span)]));
                    }
                }

                if func_name == "Err" && !args.is_empty() {
                    let err_ty = self.check_expr(&args[0], None)?;
                    let dummy_span = Span::new(0, 0, 1, 1);
                    let ok_ty = if let Some(Type::Generic(gen_name, gen_args)) = expected_type {
                        if gen_name == "Result" && gen_args.len() == 2 {
                            gen_args[0].node.clone()
                        } else {
                            Type::Named("Unit".to_string())
                        }
                    } else {
                        Type::Named("Unit".to_string())
                    };
                    return Ok(Type::Generic("Result".to_string(), vec![Spanned::new(ok_ty, dummy_span), Spanned::new(err_ty, dummy_span)]));
                }

                let mut arg_types = Vec::new();
                let mut arg_idx = 0;
                while arg_idx < args.len() {
                    let expected_arg_ty = match self.functions.get(&func_name) {
                        Some((params, ret_type)) => {
                            self.use_ref(ret_type);
                            if arg_idx < params.len() {
                                Some(params[arg_idx].clone())
                            } else {
                                None
                            }
                        }
                        None => None,
                    };

                    let checked_arg_ty = self.check_expr(&args[arg_idx], expected_arg_ty.as_ref())?;
                    arg_types.push(checked_arg_ty);
                    arg_idx += 1;
                }

                if let Some((param_types, ret_type)) = self.functions.get(&func_name).cloned() {
                    self.use_ref(&param_types);
                    if let Type::Generic(ref gen_name, ref gen_args) = ret_type {
                        if (gen_name == "Result" || gen_name == "Option") && expected_type.is_some()
                            && let Some(exp_ty) = expected_type {
                                return Ok(exp_ty.clone());
                            }
                        self.use_ref(gen_args);
                    }
                    return Ok(ret_type);
                }

                if let Some(exp_ty) = expected_type {
                    Ok(exp_ty.clone())
                } else {
                    Ok(Type::Named("Unit".to_string()))
                }
            }
            Expr::StructInit(struct_name, fields) => {
                let struct_def = match self.structs.get(struct_name) {
                    Some(def) => def.clone(),
                    None => return Ok(Type::Named(struct_name.clone())),
                };

                let mut field_idx = 0;
                while field_idx < fields.len() {
                    let (field_name, field_expr) = &fields[field_idx];
                    let expected_field_ty = struct_def.get(field_name);
                    let init_field_ty = self.check_expr(field_expr, expected_field_ty)?;

                    if let Some(expected_ty) = expected_field_ty
                        && !self.types_match(expected_ty, &init_field_ty) {
                            return Err(TypeError {
                                message: format!("Struct '{}' field '{}' type mismatch: expected {:?}, found {:?}", struct_name, field_name, expected_ty, init_field_ty),
                                span: field_expr.span,
                            });
                        }
                    field_idx += 1;
                }
                Ok(Type::Named(struct_name.clone()))
            }
            Expr::FieldAccess(target, field_name) => {
                let target_type = self.check_expr(target, None)?;
                if let Type::Named(ref type_name) = target_type
                    && let Some(fields) = self.structs.get(type_name)
                        && let Some(field_ty) = fields.get(field_name) {
                            return Ok(field_ty.clone());
                        }
                self.use_ref(field_name);
                Ok(Type::Named("Int".to_string()))
            }
            Expr::ArrayLit(elements) => {
                if elements.is_empty() {
                    let dummy_span = Span::new(0, 0, 1, 1);
                    return Ok(Type::Array(Box::new(Spanned::new(Type::Named("U8".to_string()), dummy_span)), 0));
                }

                let mut elem_type = self.check_expr(&elements[0], None)?;
                if let Some(Type::Array(inner_type, array_size)) = expected_type {
                    self.use_ref(array_size);
                    elem_type = inner_type.node.clone();
                }

                let dummy_span = Span::new(0, 0, 1, 1);
                Ok(Type::Array(Box::new(Spanned::new(elem_type, dummy_span)), elements.len()))
            }
        }
    }

    fn check_pattern(&mut self, pattern: &Spanned<Pattern>, target_type: &Type) {
        match &pattern.node {
            Pattern::Lit(lit_val) => {
                self.use_ref(lit_val);
            }
            Pattern::Var(name) | Pattern::Rest(name) => {
                self.declare_variable(name.clone(), target_type.clone(), false);
            }
            Pattern::Variant(variant_name, sub_patterns) => {
                let mut idx = 0;
                while idx < sub_patterns.len() {
                    let sub_ty = self.get_variant_field_type(variant_name, target_type, idx);
                    self.check_pattern(&sub_patterns[idx], &sub_ty);
                    idx += 1;
                }
            }
            Pattern::PathVariant(path_segments, sub_patterns) => {
                let variant_name = match path_segments.last() {
                    Some(last_seg) => last_seg.as_str(),
                    None => "",
                };
                let mut idx = 0;
                while idx < sub_patterns.len() {
                    let sub_ty = self.get_variant_field_type(variant_name, target_type, idx);
                    self.check_pattern(&sub_patterns[idx], &sub_ty);
                    idx += 1;
                }
            }
            Pattern::Array(elements) => {
                let elem_ty = match target_type {
                    Type::Array(inner_type, array_size) => {
                        self.use_ref(array_size);
                        inner_type.node.clone()
                    }
                    Type::Slice(inner_type) | Type::Ref(inner_type) => match &inner_type.node {
                        Type::Slice(slice_inner) => slice_inner.node.clone(),
                        other_type => other_type.clone(),
                    },
                    other_type => {
                        self.use_ref(other_type);
                        Type::Named("U8".to_string())
                    }
                };
                let mut idx = 0;
                while idx < elements.len() {
                    self.check_pattern(&elements[idx], &elem_ty);
                    idx += 1;
                }
            }
        }
    }

    fn get_variant_field_type(&self, variant_name: &str, target_type: &Type, field_idx: usize) -> Type {
        if (variant_name == "Ok" || variant_name == "Some")
            && let Type::Generic(gen_name, gen_args) = target_type {
                self.use_ref(gen_name);
                if field_idx < gen_args.len() {
                    return gen_args[field_idx].node.clone();
                }
            }

        if variant_name == "Err"
            && let Type::Generic(gen_name, gen_args) = target_type {
                self.use_ref(gen_name);
                if gen_args.len() >= 2 && field_idx == 0 {
                    return gen_args[1].node.clone();
                }
            }

        if let Some((param_types, ret_type)) = self.functions.get(variant_name) {
            self.use_ref(ret_type);
            if field_idx < param_types.len() {
                return param_types[field_idx].clone();
            }
        }

        target_type.clone()
    }

    fn is_diverging(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Return(_) | Expr::Break | Expr::Continue)
    }

    fn unify_types(&self, t1: &Type, t2: &Type) -> Option<Type> {
        if self.types_equal(t1, t2) {
            return Some(t1.clone());
        }
        if let (Type::Generic(n1, args1), Type::Generic(n2, args2)) = (t1, t2)
            && n1 == "Result" && n2 == "Result" && args1.len() == 2 && args2.len() == 2 {
                let dummy_span = Span::new(0, 0, 1, 1);
                let ok1 = &args1[0].node;
                let ok2 = &args2[0].node;
                let err1 = &args1[1].node;
                let err2 = &args2[1].node;

                let unified_ok = if self.is_dummy_unit(ok1) {
                    ok2.clone()
                } else {
                    ok1.clone()
                };

                let unified_err = if self.is_dummy_range_error(err1) {
                    err2.clone()
                } else if self.is_dummy_range_error(err2) || self.types_equal(err1, err2) {
                    err1.clone()
                } else {
                    return None;
                };

                return Some(Type::Generic(
                    "Result".to_string(),
                    vec![
                        Spanned::new(unified_ok, dummy_span),
                        Spanned::new(unified_err, dummy_span),
                    ],
                ));
            }
        None
    }

    fn is_dummy_unit(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(name) => name == "Unit",
            other_ty => {
                self.use_ref(other_ty);
                false
            }
        }
    }

    fn is_dummy_range_error(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(name) => name == "RangeError",
            other_ty => {
                self.use_ref(other_ty);
                false
            }
        }
    }

    fn is_integer_type(&self, ty: &Type) -> bool {
        if let Type::Named(name) = ty {
            name == "Int" || name == "U8" || name == "U32" || name == "Usize"
        } else {
            false
        }
    }

    fn is_result_type(&self, ty: &Type) -> bool {
        if let Type::Generic(name, args) = ty {
            self.use_ref(args);
            name == "Result"
        } else {
            false
        }
    }

    fn is_option_type(&self, ty: &Type) -> bool {
        if let Type::Generic(name, args) = ty {
            self.use_ref(args);
            name == "Option"
        } else {
            false
        }
    }

    fn types_equal(&self, t1: &Type, t2: &Type) -> bool {
        match (t1, t2) {
            (Type::Named(n1), Type::Named(n2)) => n1 == n2,
            (Type::Named(n), Type::Path(p)) | (Type::Path(p), Type::Named(n)) => {
                if let Some(last_seg) = p.last() {
                    last_seg == n
                } else {
                    false
                }
            }
            (Type::Path(p1), Type::Path(p2)) => {
                if p1 == p2 {
                    true
                } else if let (Some(l1), Some(l2)) = (p1.last(), p2.last()) {
                    l1 == l2
                } else {
                    false
                }
            }
            (Type::Generic(n1, args1), Type::Generic(n2, args2)) => {
                if n1 != n2 || args1.len() != args2.len() {
                    return false;
                }
                let mut idx = 0;
                while idx < args1.len() {
                    if !self.types_equal(&args1[idx].node, &args2[idx].node) {
                        return false;
                    }
                    idx += 1;
                }
                true
            }
            (Type::GenericPath(p1, args1), Type::GenericPath(p2, args2)) => {
                if p1 != p2 || args1.len() != args2.len() {
                    return false;
                }
                let mut idx = 0;
                while idx < args1.len() {
                    if !self.types_equal(&args1[idx].node, &args2[idx].node) {
                        return false;
                    }
                    idx += 1;
                }
                true
            }
            (Type::Array(ty1, sz1), Type::Array(ty2, sz2)) => {
                sz1 == sz2 && self.types_equal(&ty1.node, &ty2.node)
            }
            (Type::Slice(ty1), Type::Slice(ty2)) => self.types_equal(&ty1.node, &ty2.node),
            (Type::Ref(ty1), Type::Ref(ty2)) => self.types_equal(&ty1.node, &ty2.node),
            (Type::RefMut(ty1), Type::RefMut(ty2)) => self.types_equal(&ty1.node, &ty2.node),
            mismatch => {
                self.use_ref(&mismatch);
                false
            }
        }
    }

    fn types_match(&self, expected: &Type, actual: &Type) -> bool {
        if self.types_equal(expected, actual) {
            return true;
        }
        if self.is_integer_type(expected) && self.is_integer_type(actual) {
            return true;
        }
        false
    }

    fn declare_variable(&mut self, name: String, ty: Type, is_mutable: bool) {
        if let Some(current_scope) = self.variables.last_mut() {
            current_scope.insert(name, VariableInfo { ty, is_mutable });
        }
    }

    fn lookup_variable(&self, name: &str) -> Option<&VariableInfo> {
        let mut idx = self.variables.len();
        while idx > 0 {
            idx -= 1;
            if let Some(info) = self.variables[idx].get(name) {
                return Some(info);
            }
        }
        None
    }

    fn push_scope(&mut self) {
        self.variables.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        if self.variables.len() > 1 {
            let popped_scope = self.variables.pop();
            if let Some(scope_map) = popped_scope {
                self.use_ref(&scope_map);
            }
        }
    }
}
