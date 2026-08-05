use crate::ast::BinOp;
use crate::ast::Expr;
use crate::ast::Item;
use crate::ast::ItemKind;
use crate::ast::Lit;
use crate::ast::Pattern;
use crate::ast::Spanned;
use crate::ast::Stmt;
use crate::ast::Type;
use crate::ast::UnOp;
use crate::lexer::Span;
use crate::typecheck::TypeChecker;
use crate::typecheck::TypeError;
use std::collections::HashMap;

impl TypeChecker {
    pub fn check_item(&mut self, item: &Spanned<Item>) {
        match &item.node.kind {
            ItemKind::Const { name, ty, init, .. } => {
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
                params,
                return_ty,
                body,
                ..
            } => {
                let decl_return_type = match return_ty {
                    Some(spanned_type) => self.qualify_type(&spanned_type.node, &self.current_module_types),
                    None => Type::Named("Unit".to_string()),
                };
                self.current_return_type = Some(decl_return_type);

                self.push_scope();

                let mut p_idx = 0;
                while p_idx < params.len() {
                    let param = &params[p_idx];
                    let qual_param_ty = self.qualify_type(&param.ty.node, &self.current_module_types);
                    self.declare_variable(param.name.clone(), qual_param_ty, false);
                    p_idx += 1;
                }

                if let Some(body_stmts) = body {
                    self.check_stmts(body_stmts);
                }

                self.pop_scope();
                self.current_return_type = None;
            }
            ItemKind::Module { name, items: child_items, .. } => {
                self.module_stack.push(name.clone());
                let new_module_types = self.collect_module_local_types(child_items);
                let parent_module_types = std::mem::replace(&mut self.current_module_types, new_module_types);
                let mut idx = 0;
                while idx < child_items.len() {
                    self.check_item(&child_items[idx]);
                    idx += 1;
                }
                self.current_module_types = parent_module_types;
                self.module_stack.pop();
            }
            ItemKind::Trait { methods: child_items, .. } |
            ItemKind::Impl { methods: child_items, .. } => {
                let mut idx = 0;
                while idx < child_items.len() {
                    self.check_item(&child_items[idx]);
                    idx += 1;
                }
            }
            ItemKind::TypeDecl { .. } | ItemKind::Use { .. } => {}
        }
    }

    pub fn check_stmts(&mut self, stmts: &[Spanned<Stmt>]) {
        let mut idx = 0;
        while idx < stmts.len() {
            self.check_stmt(&stmts[idx]);
            idx += 1;
        }
    }

    pub fn check_stmt(&mut self, stmt: &Spanned<Stmt>) {
        match &stmt.node {
            Stmt::Val { name, ty, init } => {
                let expected_ty_qualified = ty.as_ref().map(|spanned_type| self.qualify_type(&spanned_type.node, &self.current_module_types));
                let init_type = match self.check_expr(init, expected_ty_qualified.as_ref()) {
                    Ok(t) => t,
                    Err(err) => {
                        self.errors.push(err);
                        return;
                    }
                };

                if let Some(expected_ty) = expected_ty_qualified
                    && !self.types_match(&expected_ty, &init_type) {
                        self.errors.push(TypeError {
                            message: format!("Variable '{}' type mismatch: expected {:?}, found {:?}", name, expected_ty, init_type),
                            span: stmt.span,
                        });
                    }
                self.declare_variable(name.clone(), init_type, false);
            }
            Stmt::Var { name, ty, init } => {
                let expected_ty_qualified = ty.as_ref().map(|spanned_type| self.qualify_type(&spanned_type.node, &self.current_module_types));
                let init_type = match self.check_expr(init, expected_ty_qualified.as_ref()) {
                    Ok(t) => t,
                    Err(err) => {
                        self.errors.push(err);
                        return;
                    }
                };

                if let Some(expected_ty) = expected_ty_qualified
                    && !self.types_match(&expected_ty, &init_type) {
                        self.errors.push(TypeError {
                            message: format!("Mutable variable '{}' type mismatch: expected {:?}, found {:?}", name, expected_ty, init_type),
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

    pub fn check_expr(&mut self, expr: &Spanned<Expr>, expected_type: Option<&Type>) -> Result<Type, TypeError> {
        match &expr.node {
            Expr::Lit(lit) => match lit {
                Lit::Int(_) => {
                    if let Some(exp_type) = expected_type
                        && self.is_integer_type(exp_type) {
                            return Ok(exp_type.clone());
                        }
                    Ok(Type::Named("Int".to_string()))
                }
                Lit::Hex(_) => {
                    if let Some(exp_type) = expected_type
                        && self.is_integer_type(exp_type) {
                            return Ok(exp_type.clone());
                        }
                    Ok(Type::Named("U32".to_string()))
                }
                Lit::Bool(_) => Ok(Type::Named("Bool".to_string())),
            },
            Expr::Var(name) => {
                if let Some(var_info) = self.lookup_variable(name) {
                    return Ok(var_info.ty.clone());
                }
                if name == "Unit" {
                    return Ok(Type::Named("Unit".to_string()));
                }
                if name == "None" {
                    if let Some(exp_ty) = expected_type {
                        return Ok(exp_ty.clone());
                    }
                    let dummy_span = Span::new(0, 0, 1, 1);
                    return Ok(Type::Generic("Option".to_string(), vec![Spanned::new(Type::Named("Int".to_string()), dummy_span)]));
                }
                if let Some((_, _, ret_ty)) = self.functions.get(name) {
                    return Ok(ret_ty.clone());
                }
                Err(TypeError {
                    message: format!("Unknown variable or function symbol '{}'", name),
                    span: expr.span,
                })
            }
            Expr::Const(name) => {
                if let Some(var_info) = self.lookup_variable(name) {
                    return Ok(var_info.ty.clone());
                }
                if let Some((_, _, ret_ty)) = self.functions.get(name) {
                    return Ok(ret_ty.clone());
                }
                Err(TypeError {
                    message: format!("Unknown constant or function symbol '{}'", name),
                    span: expr.span,
                })
            }
            Expr::Path(path) => {
                let full_path = path.join(".");
                if let Some((_, _, ret_ty)) = self.functions.get(&full_path) {
                    return Ok(ret_ty.clone());
                }
                if path.len() == 1 && path[0] == "Unit" {
                    return Ok(Type::Named("Unit".to_string()));
                }
                if path.len() == 1 && path[0] == "None" {
                    if let Some(exp_ty) = expected_type {
                        return Ok(exp_ty.clone());
                    }
                    let dummy_span = Span::new(0, 0, 1, 1);
                    return Ok(Type::Generic("Option".to_string(), vec![Spanned::new(Type::Named("Int".to_string()), dummy_span)]));
                }
                if path.len() >= 2 {
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
                Err(TypeError {
                    message: format!("Unknown path symbol '{}'", full_path),
                    span: expr.span,
                })
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
                let func_name = match &func.node {
                    Expr::Var(name) => name.clone(),
                    Expr::Path(path) => path.join("."),
                    non_var_or_path => {
                        let mut idx = 0;
                        while idx < args.len() {
                            self.check_expr(&args[idx], None)?;
                            idx += 1;
                        }
                        let callee_spanned = Spanned::new(non_var_or_path.clone(), func.span);
                        return self.check_expr(&callee_spanned, None);
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
                        Some((_, params, _)) => {
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

                if let Some((gen_params, param_types, ret_type)) = self.functions.get(&func_name).cloned() {
                    let mut bindings = HashMap::new();

                    if let Some(t_args) = type_args {
                        let mut t_idx = 0;
                        while t_idx < t_args.len() && t_idx < gen_params.len() {
                            let qual_t_arg = self.qualify_type(&t_args[t_idx].node, &self.current_module_types);
                            bindings.insert(gen_params[t_idx].clone(), qual_t_arg);
                            t_idx += 1;
                        }
                    }

                    let mut p_idx = 0;
                    while p_idx < param_types.len() && p_idx < arg_types.len() {
                        self.infer_generic_bindings(&param_types[p_idx], &arg_types[p_idx], &mut bindings);
                        p_idx += 1;
                    }

                    if let Some(exp_ty) = expected_type {
                        self.infer_generic_bindings(&ret_type, exp_ty, &mut bindings);
                    }

                    let substituted_ret = self.substitute_generics(&ret_type, &bindings);

                    if let Type::Generic(ref gen_name, _) = substituted_ret
                        && (gen_name == "Result" || gen_name == "Option")
                        && expected_type.is_some()
                        && let Some(exp_ty) = expected_type
                    {
                        if let Some(unified) = self.unify_types(&substituted_ret, exp_ty) {
                            return Ok(unified);
                        }
                        return Ok(exp_ty.clone());
                    }
                    return Ok(substituted_ret);
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
                let mut target_type = self.check_expr(target, None)?;
                while let Type::Ref(inner) | Type::RefMut(inner) = target_type {
                    target_type = inner.node.clone();
                }
                if let Type::Named(ref type_name) = target_type
                    && let Some(fields) = self.structs.get(type_name)
                        && let Some(field_ty) = fields.get(field_name) {
                            return Ok(field_ty.clone());
                        }
                if let Type::Path(ref path) = target_type
                    && let Some(type_name) = path.last()
                    && let Some(fields) = self.structs.get(type_name)
                        && let Some(field_ty) = fields.get(field_name) {
                            return Ok(field_ty.clone());
                        }
                Ok(Type::Named("Int".to_string()))
            }
            Expr::ArrayLit(elements) => {
                if elements.is_empty() {
                    let dummy_span = Span::new(0, 0, 1, 1);
                    return Ok(Type::Array(Box::new(Spanned::new(Type::Named("U8".to_string()), dummy_span)), 0));
                }

                let mut elem_type = self.check_expr(&elements[0], None)?;
                if let Some(Type::Array(inner_type, _)) = expected_type {
                    elem_type = inner_type.node.clone();
                }

                let dummy_span = Span::new(0, 0, 1, 1);
                Ok(Type::Array(Box::new(Spanned::new(elem_type, dummy_span)), elements.len()))
            }
        }
    }

    pub fn check_pattern(&mut self, pattern: &Spanned<Pattern>, target_type: &Type) {
        match &pattern.node {
            Pattern::Lit(_) => {}
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
                    Type::Array(inner_type, _) => inner_type.node.clone(),
                    Type::Slice(inner_type) | Type::Ref(inner_type) => match &inner_type.node {
                        Type::Slice(slice_inner) => slice_inner.node.clone(),
                        other_type => other_type.clone(),
                    },
                    Type::Named(_) | Type::Path(_) | Type::Generic(..) | Type::GenericPath(..) | Type::RefMut(_) => {
                        Type::Named("U8".to_string())
                    }
                };
                let mut idx = 0;
                while idx < elements.len() {
                    if let Pattern::Rest(ref name) = elements[idx].node {
                        let rest_ty = match target_type {
                            Type::Ref(inner) => match &inner.node {
                                Type::Slice(_) => target_type.clone(),
                                _ => target_type.clone(),
                            },
                            Type::Array(inner, _) => {
                                let dummy_span = Span::new(0, 0, 1, 1);
                                Type::Slice(Box::new(Spanned::new(inner.node.clone(), dummy_span)))
                            }
                            _ => target_type.clone(),
                        };
                        self.declare_variable(name.clone(), rest_ty, false);
                    } else {
                        self.check_pattern(&elements[idx], &elem_ty);
                    }
                    idx += 1;
                }
            }
        }
    }

    pub fn get_variant_field_type(&self, variant_name: &str, target_type: &Type, field_idx: usize) -> Type {
        if (variant_name == "Ok" || variant_name == "Some")
            && let Type::Generic(_, gen_args) = target_type
            && field_idx < gen_args.len()
        {
            return gen_args[field_idx].node.clone();
        }

        if variant_name == "Err"
            && let Type::Generic(_, gen_args) = target_type
            && gen_args.len() >= 2
            && field_idx == 0
        {
            return gen_args[1].node.clone();
        }

        if let Some((_, param_types, ret_type)) = self.functions.get(variant_name)
            && field_idx < param_types.len()
            && self.types_match(target_type, ret_type)
        {
            return param_types[field_idx].clone();
        }

        target_type.clone()
    }

    pub fn is_diverging(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Return(_) | Expr::Break | Expr::Continue)
    }

    pub fn unify_types(&self, t1: &Type, t2: &Type) -> Option<Type> {
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
}
