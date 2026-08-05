use crate::ast::Expr;
use crate::ast::Item;
use crate::ast::ItemKind;
use crate::ast::Pattern;
use crate::ast::Spanned;
use crate::ast::Stmt;
use crate::ast::Type;
use crate::ast::TypeKind;
use crate::ast::UnOp;
use crate::lexer::Span;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;

#[derive(Debug, Clone)]
pub struct BorrowError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for BorrowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Borrow Check Error at line {}, col {}: {}",
            self.span.line, self.span.col, self.message
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BorrowKind {
    Shared,
    Mutable,
}

#[derive(Debug, Clone)]
pub struct BindingInfo {
    pub ty: Type,
    pub is_mutable: bool,
    pub is_linear: bool,
    pub is_moved: bool,
    pub is_param: bool,
    pub holds_borrows_of: Vec<(String, BorrowKind)>,
    pub active_borrows: Vec<BorrowKind>,
    pub last_used_stmt: usize,
    pub span: Span,
}

pub struct NewBinding {
    pub name: String,
    pub ty: Type,
    pub is_mutable: bool,
    pub is_linear: bool,
    pub is_param: bool,
    pub holds_borrows_of: Vec<(String, BorrowKind)>,
    pub span: Span,
}

pub struct BorrowChecker {
    linear_types: HashSet<String>,
    linear_functions: HashSet<String>,
    scopes: Vec<HashMap<String, BindingInfo>>,
    module_stack: Vec<String>,
    current_stmt_idx: usize,
    errors: Vec<BorrowError>,
}

impl BorrowChecker {
    pub fn new() -> Self {
        Self {
            linear_types: HashSet::new(),
            linear_functions: HashSet::new(),
            scopes: vec![HashMap::new()],
            module_stack: Vec::new(),
            current_stmt_idx: 0,
            errors: Vec::new(),
        }
    }

    pub fn check_program(&mut self, items: &[Spanned<Item>]) -> Result<(), Vec<BorrowError>> {
        self.collect_linear_types(items);

        let mut idx = 0;
        while idx < items.len() {
            self.check_item(&items[idx]);
            idx += 1;
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }

    fn collect_linear_types(&mut self, items: &[Spanned<Item>]) {
        let mut idx = 0;
        while idx < items.len() {
            let item = &items[idx];
            match &item.node.kind {
                ItemKind::TypeDecl { name, kind, .. } => {
                    if let TypeKind::Native = kind {
                        self.linear_types.insert(name.clone());
                        if !self.module_stack.is_empty() {
                            let full_name = format!("{}.{}", self.module_stack.join("."), name);
                            self.linear_types.insert(full_name);
                        }
                    }
                }
                ItemKind::Module { name, items: child_items, .. } => {
                    self.module_stack.push(name.clone());
                    self.collect_linear_types(child_items);
                    self.module_stack.pop();
                }
                ItemKind::Trait { methods, .. } | ItemKind::Impl { methods, .. } => {
                    self.collect_linear_types(methods);
                }
                ItemKind::Function { .. } | ItemKind::Const { .. } | ItemKind::Use { .. } => {}
            }
            idx += 1;
        }

        let mut f_idx = 0;
        while f_idx < items.len() {
            let item = &items[f_idx];
            self.collect_linear_functions(item);
            f_idx += 1;
        }
    }

    fn collect_linear_functions(&mut self, item: &Spanned<Item>) {
        match &item.node.kind {
            ItemKind::Function { name, return_ty, .. } => {
                if let Some(ret_ty) = return_ty
                    && self.is_linear_type(&ret_ty.node)
                {
                    self.linear_functions.insert(name.clone());
                    if !self.module_stack.is_empty() {
                        let full_name = format!("{}.{}", self.module_stack.join("."), name);
                        self.linear_functions.insert(full_name);
                    }
                }
            }
            ItemKind::Module { name, items: child_items, .. } => {
                self.module_stack.push(name.clone());
                let mut idx = 0;
                while idx < child_items.len() {
                    self.collect_linear_functions(&child_items[idx]);
                    idx += 1;
                }
                self.module_stack.pop();
            }
            ItemKind::Trait { methods, .. } | ItemKind::Impl { methods, .. } => {
                let mut idx = 0;
                while idx < methods.len() {
                    self.collect_linear_functions(&methods[idx]);
                    idx += 1;
                }
            }
            ItemKind::TypeDecl { .. } | ItemKind::Const { .. } | ItemKind::Use { .. } => {}
        }
    }

    fn is_linear_type(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(name) => self.linear_types.contains(name),
            Type::Path(path) => {
                let full_path = path.join(".");
                if self.linear_types.contains(&full_path) {
                    true
                } else if let Some(last_seg) = path.last() {
                    self.linear_types.contains(last_seg)
                } else {
                    false
                }
            }
            Type::Generic(name, args) => {
                if self.linear_types.contains(name) {
                    return true;
                }
                let mut idx = 0;
                while idx < args.len() {
                    if self.is_linear_type(&args[idx].node) {
                        return true;
                    }
                    idx += 1;
                }
                false
            }
            Type::GenericPath(path, args) => {
                let full_path = path.join(".");
                if self.linear_types.contains(&full_path) {
                    return true;
                }
                let mut idx = 0;
                while idx < args.len() {
                    if self.is_linear_type(&args[idx].node) {
                        return true;
                    }
                    idx += 1;
                }
                false
            }
            Type::Array(inner, _) | Type::Slice(inner) => self.is_linear_type(&inner.node),
            Type::Ref(_) | Type::RefMut(_) => false,
        }
    }

    fn is_linear_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Var(name) => {
                if let Some(info) = self.lookup_binding(name) {
                    info.is_linear
                } else {
                    self.linear_types.contains(name)
                }
            }
            Expr::Path(path) => {
                let full_path = path.join(".");
                if self.linear_types.contains(&full_path) {
                    return true;
                }
                if let Some(last_seg) = path.last()
                    && self.linear_types.contains(last_seg) {
                        return true;
                    }
                false
            }
            Expr::Try(inner) => self.is_linear_expr(&inner.node),
            Expr::Call(func, _, _) => {
                let func_name = match &func.node {
                    Expr::Var(name) => name.clone(),
                    Expr::Path(path) => path.join("."),
                    Expr::Lit(_) | Expr::Const(_) | Expr::Binary(..) | Expr::Unary(..) | Expr::Try(..) | Expr::Return(_) | Expr::Break | Expr::Continue | Expr::Call(..) | Expr::StructInit(..) | Expr::FieldAccess(..) | Expr::ArrayLit(_) | Expr::Match(..) | Expr::If(..) => String::new(),
                };
                if self.linear_functions.contains(&func_name) {
                    return true;
                }
                if let Expr::Path(path) = &func.node
                    && let Some(last_seg) = path.last()
                        && self.linear_functions.contains(last_seg) {
                            return true;
                        }
                false
            }
            Expr::Unary(_, inner) => self.is_linear_expr(&inner.node),
            Expr::Lit(_) | Expr::Const(_) | Expr::Binary(..) | Expr::Return(_) | Expr::Break | Expr::Continue | Expr::StructInit(..) | Expr::FieldAccess(..) | Expr::ArrayLit(_) | Expr::Match(..) | Expr::If(..) => false,
        }
    }

    fn check_item(&mut self, item: &Spanned<Item>) {
        match &item.node.kind {
            ItemKind::Function {
                params,
                return_ty,
                body,
                ..
            } => {
                self.push_scope();
                self.current_stmt_idx = 0;

                let mut ref_param_count = 0;
                let mut p_idx = 0;
                while p_idx < params.len() {
                    let param = &params[p_idx];
                    let is_linear = self.is_linear_type(&param.ty.node);

                    if matches!(param.ty.node, Type::Ref(_) | Type::RefMut(_)) {
                        ref_param_count += 1;
                    }

                    self.declare_binding(NewBinding {
                        name: param.name.clone(),
                        ty: param.ty.node.clone(),
                        is_mutable: false,
                        is_linear,
                        is_param: true,
                        holds_borrows_of: Vec::new(),
                        span: item.span,
                    });
                    p_idx += 1;
                }

                if let Some(ret_type) = return_ty
                    && matches!(ret_type.node, Type::Ref(_) | Type::RefMut(_))
                    && ref_param_count > 1
                {
                    self.errors.push(BorrowError {
                        message: "Ambiguous returned reference from function with multiple reference parameters".to_string(),
                        span: ret_type.span,
                    });
                }

                if let Some(body_stmts) = body {
                    self.update_last_used_stmts(body_stmts);
                    self.check_stmts(body_stmts);
                    self.verify_linear_handles_consumed();
                }

                self.pop_scope();
            }
            ItemKind::Module { name, items: child_items, .. } => {
                self.module_stack.push(name.clone());
                let mut idx = 0;
                while idx < child_items.len() {
                    self.check_item(&child_items[idx]);
                    idx += 1;
                }
                self.module_stack.pop();
            }
            ItemKind::Trait { methods: child_items, .. } | ItemKind::Impl { methods: child_items, .. } => {
                let mut idx = 0;
                while idx < child_items.len() {
                    self.check_item(&child_items[idx]);
                    idx += 1;
                }
            }
            ItemKind::Const { init, .. } => {
                self.check_expr(init, false);
            }
            ItemKind::TypeDecl { .. } | ItemKind::Use { .. } => {}
        }
    }

    fn update_last_used_stmts(&mut self, stmts: &[Spanned<Stmt>]) {
        let mut idx = 0;
        while idx < stmts.len() {
            self.collect_vars_in_stmt(&stmts[idx], idx);
            idx += 1;
        }
    }

    fn collect_vars_in_stmt(&mut self, stmt: &Spanned<Stmt>, stmt_idx: usize) {
        match &stmt.node {
            Stmt::Val { init, .. } | Stmt::Var { init, .. } => {
                self.collect_vars_in_expr(init, stmt_idx);
            }
            Stmt::Assign { name, expr } => {
                self.mark_var_used(name, stmt_idx);
                self.collect_vars_in_expr(expr, stmt_idx);
            }
            Stmt::Expr(expr) => {
                self.collect_vars_in_expr(expr, stmt_idx);
            }
            Stmt::While { cond, body } => {
                self.collect_vars_in_expr(cond, stmt_idx);
                let mut b_idx = 0;
                while b_idx < body.len() {
                    self.collect_vars_in_stmt(&body[b_idx], stmt_idx);
                    b_idx += 1;
                }
            }
        }
    }

    fn collect_vars_in_expr(&mut self, expr: &Spanned<Expr>, stmt_idx: usize) {
        match &expr.node {
            Expr::Var(name) => {
                self.mark_var_used(name, stmt_idx);
            }
            Expr::Const(_) => {}
            Expr::Path(path) => {
                if path.len() == 1 {
                    self.mark_var_used(&path[0], stmt_idx);
                }
            }
            Expr::Binary(left, _, right) => {
                self.collect_vars_in_expr(left, stmt_idx);
                self.collect_vars_in_expr(right, stmt_idx);
            }
            Expr::Unary(_, inner) | Expr::Try(inner) => {
                self.collect_vars_in_expr(inner, stmt_idx);
            }
            Expr::Return(opt_expr) => {
                if let Some(inner) = opt_expr {
                    self.collect_vars_in_expr(inner, stmt_idx);
                }
            }
            Expr::Call(func, _, args) => {
                self.collect_vars_in_expr(func, stmt_idx);
                let mut idx = 0;
                while idx < args.len() {
                    self.collect_vars_in_expr(&args[idx], stmt_idx);
                    idx += 1;
                }
            }
            Expr::StructInit(_, fields) => {
                let mut idx = 0;
                while idx < fields.len() {
                    self.collect_vars_in_expr(&fields[idx].1, stmt_idx);
                    idx += 1;
                }
            }
            Expr::FieldAccess(target, _) => {
                self.collect_vars_in_expr(target, stmt_idx);
            }
            Expr::ArrayLit(elements) => {
                let mut idx = 0;
                while idx < elements.len() {
                    self.collect_vars_in_expr(&elements[idx], stmt_idx);
                    idx += 1;
                }
            }
            Expr::Match(target, arms) => {
                self.collect_vars_in_expr(target, stmt_idx);
                let mut idx = 0;
                while idx < arms.len() {
                    self.collect_vars_in_expr(&arms[idx].1, stmt_idx);
                    idx += 1;
                }
            }
            Expr::If(cond, then_body, else_body) => {
                self.collect_vars_in_expr(cond, stmt_idx);
                let mut idx = 0;
                while idx < then_body.len() {
                    self.collect_vars_in_stmt(&then_body[idx], stmt_idx);
                    idx += 1;
                }
                if let Some(else_stmts) = else_body {
                    let mut e_idx = 0;
                    while e_idx < else_stmts.len() {
                        self.collect_vars_in_stmt(&else_stmts[e_idx], stmt_idx);
                        e_idx += 1;
                    }
                }
            }
            Expr::Lit(_) | Expr::Break | Expr::Continue => {}
        }
    }

    fn mark_var_used(&mut self, name: &str, stmt_idx: usize) {
        if let Some(info) = self.lookup_binding_mut(name)
            && stmt_idx > info.last_used_stmt
        {
            info.last_used_stmt = stmt_idx;
        }
    }

    fn expire_borrows(&mut self) {
        let curr_idx = self.current_stmt_idx;
        let mut scope_idx = 0;
        while scope_idx < self.scopes.len() {
            let keys: Vec<String> = self.scopes[scope_idx].keys().cloned().collect();
            let mut k_idx = 0;
            while k_idx < keys.len() {
                let holder_name = &keys[k_idx];
                let should_expire = if let Some(info) = self.scopes[scope_idx].get(holder_name) {
                    info.last_used_stmt < curr_idx && !info.holds_borrows_of.is_empty()
                } else {
                    false
                };

                if should_expire {
                    let holds = if let Some(info) = self.scopes[scope_idx].get_mut(holder_name) {
                        std::mem::take(&mut info.holds_borrows_of)
                    } else {
                        Vec::new()
                    };

                    let mut h_idx = 0;
                    while h_idx < holds.len() {
                        let (origin_name, kind) = &holds[h_idx];
                        if let Some(origin_info) = self.lookup_binding_mut(origin_name)
                            && let Some(pos) = origin_info.active_borrows.iter().position(|b| b == kind)
                        {
                            origin_info.active_borrows.remove(pos);
                        }
                        h_idx += 1;
                    }
                }
                k_idx += 1;
            }
            scope_idx += 1;
        }
    }

    fn check_stmts(&mut self, stmts: &[Spanned<Stmt>]) {
        let mut idx = 0;
        while idx < stmts.len() {
            self.current_stmt_idx = idx;
            self.expire_borrows();
            self.check_stmt(&stmts[idx]);
            idx += 1;
        }
    }

    fn check_stmt(&mut self, stmt: &Spanned<Stmt>) {
        match &stmt.node {
            Stmt::Val { name, ty, init } => {
                self.check_expr(init, false);
                let is_linear = match ty {
                    Some(spanned_ty) => self.is_linear_type(&spanned_ty.node),
                    None => self.is_linear_expr(&init.node),
                };
                let binding_ty = match ty {
                    Some(spanned_ty) => spanned_ty.node.clone(),
                    None => Type::Named("Unit".to_string()),
                };
                let mut holds = Vec::new();
                self.extract_borrows_from_expr(init, &mut holds);

                self.declare_binding(NewBinding {
                    name: name.clone(),
                    ty: binding_ty,
                    is_mutable: false,
                    is_linear,
                    is_param: false,
                    holds_borrows_of: holds,
                    span: stmt.span,
                });
            }
            Stmt::Var { name, ty, init } => {
                self.check_expr(init, false);
                let is_linear = match ty {
                    Some(spanned_ty) => self.is_linear_type(&spanned_ty.node),
                    None => self.is_linear_expr(&init.node),
                };
                let binding_ty = match ty {
                    Some(spanned_ty) => spanned_ty.node.clone(),
                    None => Type::Named("Unit".to_string()),
                };
                let mut holds = Vec::new();
                self.extract_borrows_from_expr(init, &mut holds);

                self.declare_binding(NewBinding {
                    name: name.clone(),
                    ty: binding_ty,
                    is_mutable: true,
                    is_linear,
                    is_param: false,
                    holds_borrows_of: holds,
                    span: stmt.span,
                });
            }
            Stmt::Assign { name, expr } => {
                self.check_expr(expr, false);
                let mut is_immutable_err = false;
                if let Some(info) = self.lookup_binding_mut(name) {
                    if !info.is_mutable {
                        is_immutable_err = true;
                    }
                    if info.is_moved {
                        info.is_moved = false;
                    }
                }
                if is_immutable_err {
                    self.errors.push(BorrowError {
                        message: format!("Cannot assign to immutable variable '{}'", name),
                        span: stmt.span,
                    });
                }
            }
            Stmt::Expr(expr) => {
                self.check_expr(expr, false);
            }
            Stmt::While { cond, body } => {
                self.check_expr(cond, false);
                self.push_scope();
                self.check_stmts(body);
                self.verify_linear_handles_consumed();
                self.pop_scope();
            }
        }
    }

    fn extract_borrows_from_expr(&self, expr: &Spanned<Expr>, holds: &mut Vec<(String, BorrowKind)>) {
        match &expr.node {
            Expr::Unary(UnOp::Ref, inner) => {
                if let Expr::Var(ref var_name) = inner.node {
                    holds.push((var_name.clone(), BorrowKind::Shared));
                }
            }
            Expr::Unary(UnOp::RefMut, inner) => {
                if let Expr::Var(ref var_name) = inner.node {
                    holds.push((var_name.clone(), BorrowKind::Mutable));
                }
            }
            Expr::Unary(UnOp::Not, inner) | Expr::Unary(UnOp::Neg, inner) => {
                self.extract_borrows_from_expr(inner, holds);
            }
            Expr::Call(func, _, args) => {
                self.extract_borrows_from_expr(func, holds);
                let mut idx = 0;
                while idx < args.len() {
                    self.extract_borrows_from_expr(&args[idx], holds);
                    idx += 1;
                }
            }
            Expr::Try(inner) | Expr::FieldAccess(inner, _) => {
                self.extract_borrows_from_expr(inner, holds);
            }
            Expr::Var(_) | Expr::Const(_) | Expr::Path(_) | Expr::Lit(_) | Expr::Binary(..) | Expr::Return(_) | Expr::Break | Expr::Continue | Expr::StructInit(..) | Expr::ArrayLit(_) | Expr::Match(..) | Expr::If(..) => {}
        }
    }

    fn check_expr(&mut self, expr: &Spanned<Expr>, is_inside_ref: bool) {
        match &expr.node {
            Expr::Lit(_) | Expr::Break | Expr::Continue => {}
            Expr::Var(name) => {
                if is_inside_ref {
                    self.verify_variable_access(name, expr.span);
                } else {
                    self.consume_variable_if_linear(name, expr.span);
                }
            }
            Expr::Const(_) => {}
            Expr::Path(path) => {
                if path.len() == 1 {
                    if is_inside_ref {
                        self.verify_variable_access(&path[0], expr.span);
                    } else {
                        self.consume_variable_if_linear(&path[0], expr.span);
                    }
                }
            }
            Expr::Binary(left, _, right) => {
                self.check_expr(left, false);
                self.check_expr(right, false);
            }
            Expr::Unary(op, inner) => match op {
                UnOp::Ref => {
                    if let Expr::Var(ref var_name) = inner.node {
                        self.verify_borrow(var_name, BorrowKind::Shared, inner.span);
                    } else if let Expr::Path(ref path) = inner.node {
                        if path.len() == 1 {
                            self.verify_borrow(&path[0], BorrowKind::Shared, inner.span);
                        }
                    } else {
                        self.check_expr(inner, true);
                    }
                }
                UnOp::RefMut => {
                    if let Expr::Var(ref var_name) = inner.node {
                        self.verify_borrow(var_name, BorrowKind::Mutable, inner.span);
                    } else if let Expr::Path(ref path) = inner.node {
                        if path.len() == 1 {
                            self.verify_borrow(&path[0], BorrowKind::Mutable, inner.span);
                        }
                    } else {
                        self.check_expr(inner, true);
                    }
                }
                UnOp::Not | UnOp::Neg => {
                    self.check_expr(inner, false);
                }
            },
            Expr::Try(inner) => {
                self.check_expr(inner, false);
            }
            Expr::Return(opt_expr) => {
                if let Some(inner_expr) = opt_expr {
                    if let Expr::Unary(UnOp::Ref, ref inner_var) = inner_expr.node
                        && let Expr::Var(ref var_name) = inner_var.node
                        && let Some(info) = self.lookup_binding(var_name)
                        && !info.is_param
                    {
                        self.errors.push(BorrowError {
                            message: format!("Cannot return reference to local variable '{}'", var_name),
                            span: expr.span,
                        });
                    }
                    self.check_expr(inner_expr, false);
                }
            }
            Expr::Call(func, type_args, args) => {
                self.check_expr(func, false);
                if let Some(t_args) = type_args {
                    let mut t_idx = 0;
                    while t_idx < t_args.len() {
                        t_idx += 1;
                    }
                }
                let mut arg_idx = 0;
                while arg_idx < args.len() {
                    self.check_expr(&args[arg_idx], false);
                    arg_idx += 1;
                }
            }
            Expr::StructInit(_, fields) => {
                let mut field_idx = 0;
                while field_idx < fields.len() {
                    self.check_expr(&fields[field_idx].1, false);
                    field_idx += 1;
                }
            }
            Expr::FieldAccess(target, _) => {
                self.check_expr(target, is_inside_ref);
            }
            Expr::ArrayLit(elements) => {
                let mut elem_idx = 0;
                while elem_idx < elements.len() {
                    self.check_expr(&elements[elem_idx], false);
                    elem_idx += 1;
                }
            }
            Expr::Match(target, arms) => {
                self.check_expr(target, false);
                let parent_snapshot = self.snapshot_scopes();
                let mut branch_states = Vec::new();

                let mut arm_idx = 0;
                while arm_idx < arms.len() {
                    let (pattern, body_expr) = &arms[arm_idx];
                    self.restore_scopes(&parent_snapshot);
                    self.push_scope();
                    self.check_pattern(pattern);
                    self.check_expr(body_expr, false);
                    self.verify_linear_handles_consumed();

                    let is_diverging = self.is_diverging_expr(&body_expr.node);
                    let state = self.snapshot_scopes();
                    branch_states.push((state, is_diverging));

                    self.pop_scope();
                    arm_idx += 1;
                }

                self.restore_scopes(&parent_snapshot);
                self.merge_branch_states(&parent_snapshot, &branch_states, expr.span);
            }
            Expr::If(cond, then_body, else_body) => {
                self.check_expr(cond, false);
                let parent_snapshot = self.snapshot_scopes();
                let mut branch_states = Vec::new();

                self.push_scope();
                self.check_stmts(then_body);
                self.verify_linear_handles_consumed();

                let then_diverges = !then_body.is_empty() && self.is_diverging_stmt(&then_body.last().unwrap().node);
                branch_states.push((self.snapshot_scopes(), then_diverges));
                self.pop_scope();

                self.restore_scopes(&parent_snapshot);

                if let Some(else_stmts) = else_body {
                    self.push_scope();
                    self.check_stmts(else_stmts);
                    self.verify_linear_handles_consumed();

                    let else_diverges = !else_stmts.is_empty() && self.is_diverging_stmt(&else_stmts.last().unwrap().node);
                    branch_states.push((self.snapshot_scopes(), else_diverges));
                    self.pop_scope();
                }

                self.restore_scopes(&parent_snapshot);
                self.merge_branch_states(&parent_snapshot, &branch_states, expr.span);
            }
        }
    }

    fn snapshot_scopes(&self) -> Vec<HashMap<String, BindingInfo>> {
        self.scopes.clone()
    }

    fn restore_scopes(&mut self, snapshot: &[HashMap<String, BindingInfo>]) {
        self.scopes = snapshot.to_vec();
    }

    fn is_diverging_expr(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Return(_) | Expr::Break | Expr::Continue)
    }

    fn is_diverging_stmt(&self, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Expr(expr) => self.is_diverging_expr(&expr.node),
            _ => false,
        }
    }

    fn merge_branch_states(&mut self, parent_snapshot: &[HashMap<String, BindingInfo>], branch_states: &[(Vec<HashMap<String, BindingInfo>>, bool)], span: Span) {
        if branch_states.is_empty() {
            return;
        }

        let non_diverging_branches: Vec<&Vec<HashMap<String, BindingInfo>>> = branch_states
            .iter()
            .filter(|(_, is_diverging)| !*is_diverging)
            .map(|(state, _)| state)
            .collect();

        if non_diverging_branches.is_empty() {
            return;
        }

        let mut s_idx = 0;
        while s_idx < parent_snapshot.len() {
            let scope = &parent_snapshot[s_idx];
            let keys: Vec<String> = scope.keys().cloned().collect();
            let mut k_idx = 0;
            while k_idx < keys.len() {
                let name = &keys[k_idx];
                if let Some(parent_info) = scope.get(name)
                    && parent_info.is_linear
                    && !parent_info.is_moved
                {
                    let moved_in_all = non_diverging_branches.iter().all(|branch| {
                        if s_idx < branch.len() {
                            branch[s_idx].get(name).is_some_and(|info| info.is_moved)
                        } else {
                            false
                        }
                    });

                    let moved_in_some = non_diverging_branches.iter().any(|branch| {
                        if s_idx < branch.len() {
                            branch[s_idx].get(name).is_some_and(|info| info.is_moved)
                        } else {
                            false
                        }
                    });

                    if moved_in_all {
                        if let Some(target_info) = self.lookup_binding_mut(name) {
                            target_info.is_moved = true;
                        }
                    } else if moved_in_some {
                        self.errors.push(BorrowError {
                            message: format!("Linear value '{}' must be consumed on all execution paths", name),
                            span,
                        });
                    }
                }
                k_idx += 1;
            }
            s_idx += 1;
        }
    }

    fn check_pattern(&mut self, pattern: &Spanned<Pattern>) {
        match &pattern.node {
            Pattern::Lit(_) => {}
            Pattern::Var(name) | Pattern::Rest(name) => {
                let dummy_ty = Type::Named("Unit".to_string());
                self.declare_binding(NewBinding {
                    name: name.clone(),
                    ty: dummy_ty,
                    is_mutable: false,
                    is_linear: false,
                    is_param: false,
                    holds_borrows_of: Vec::new(),
                    span: pattern.span,
                });
            }
            Pattern::Variant(_, sub_patterns) | Pattern::PathVariant(_, sub_patterns) => {
                let mut sub_idx = 0;
                while sub_idx < sub_patterns.len() {
                    self.check_pattern(&sub_patterns[sub_idx]);
                    sub_idx += 1;
                }
            }
            Pattern::Array(elements) => {
                let mut elem_idx = 0;
                while elem_idx < elements.len() {
                    self.check_pattern(&elements[elem_idx]);
                    elem_idx += 1;
                }
            }
        }
    }

    fn verify_variable_access(&mut self, name: &str, span: Span) {
        if let Some(info) = self.lookup_binding(name)
            && info.is_moved
        {
            self.errors.push(BorrowError {
                message: format!("Use of moved value '{}'", name),
                span,
            });
        }
    }

    fn consume_variable_if_linear(&mut self, name: &str, span: Span) {
        let mut is_already_moved = false;

        if let Some(info) = self.lookup_binding_mut(name) {
            if info.is_moved {
                is_already_moved = true;
            } else if info.is_linear {
                info.is_moved = true;
            }
        }

        if is_already_moved {
            self.errors.push(BorrowError {
                message: format!("Use of moved value '{}'", name),
                span,
            });
        }
    }

    fn verify_borrow(&mut self, name: &str, kind: BorrowKind, span: Span) {
        let mut err_msg: Option<String> = None;

        if let Some(info) = self.lookup_binding_mut(name) {
            if info.is_moved {
                err_msg = Some(format!("Cannot borrow moved value '{}'", name));
            } else {
                match kind {
                    BorrowKind::Shared => {
                        let mut has_mut_borrow = false;
                        let mut b_idx = 0;
                        while b_idx < info.active_borrows.len() {
                            if info.active_borrows[b_idx] == BorrowKind::Mutable {
                                has_mut_borrow = true;
                                break;
                            }
                            b_idx += 1;
                        }
                        if has_mut_borrow {
                            err_msg = Some(format!("Cannot borrow '{}' as shared because it is already mutably borrowed", name));
                        } else {
                            info.active_borrows.push(BorrowKind::Shared);
                        }
                    }
                    BorrowKind::Mutable => {
                        if !info.is_mutable && !info.is_linear {
                            err_msg = Some(format!("Cannot borrow immutable variable '{}' as mutable", name));
                        } else if !info.active_borrows.is_empty() {
                            err_msg = Some(format!("Cannot borrow '{}' as mutable because it is already borrowed", name));
                        } else {
                            info.active_borrows.push(BorrowKind::Mutable);
                        }
                    }
                }
            }
        }

        if let Some(message) = err_msg {
            self.errors.push(BorrowError { message, span });
        }
    }

    fn verify_linear_handles_consumed(&mut self) {
        if let Some(current_scope) = self.scopes.last() {
            for (name, info) in current_scope {
                if info.is_linear && !info.is_moved {
                    self.errors.push(BorrowError {
                        message: format!("Linear value '{}' of type {:?} must be consumed", name, info.ty),
                        span: info.span,
                    });
                }
            }
        }
    }

    fn declare_binding(&mut self, binding: NewBinding) {
        if let Some(current_scope) = self.scopes.last_mut() {
            current_scope.insert(
                binding.name,
                BindingInfo {
                    ty: binding.ty,
                    is_mutable: binding.is_mutable,
                    is_linear: binding.is_linear,
                    is_moved: false,
                    is_param: binding.is_param,
                    holds_borrows_of: binding.holds_borrows_of,
                    active_borrows: Vec::new(),
                    last_used_stmt: self.current_stmt_idx,
                    span: binding.span,
                },
            );
        }
    }

    fn lookup_binding(&self, name: &str) -> Option<&BindingInfo> {
        let mut idx = self.scopes.len();
        while idx > 0 {
            idx -= 1;
            if let Some(info) = self.scopes[idx].get(name) {
                return Some(info);
            }
        }
        None
    }

    fn lookup_binding_mut(&mut self, name: &str) -> Option<&mut BindingInfo> {
        let mut idx = self.scopes.len();
        while idx > 0 {
            idx -= 1;
            if self.scopes[idx].contains_key(name) {
                return self.scopes[idx].get_mut(name);
            }
        }
        None
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }
}