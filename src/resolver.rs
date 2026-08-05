use crate::ast::Expr;
use crate::ast::Item;
use crate::ast::ItemKind;
use crate::ast::Pattern;
use crate::ast::Spanned;
use crate::ast::Stmt;
use crate::ast::Type;
use crate::ast::TypeKind;
use crate::lexer::Span;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone)]
pub struct ResolveError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Resolution Error at line {}, col {}: {}",
            self.span.line, self.span.col, self.message
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Const,
    Type,
    Function,
    Module,
    Trait,
    Val,
    Var,
    Param,
    Import,
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub is_pub: bool,
    pub span: Span,
    pub is_builtin: bool,
    pub used: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeKind {
    Root,
    Module(String, bool),
    Function(String),
    Block,
    Trait(String),
    Impl,
}

#[derive(Debug, Clone)]
pub struct Scope {
    pub kind: ScopeKind,
    pub symbols: HashMap<String, Symbol>,
}

pub struct Resolver {
    scopes: Vec<Scope>,
    errors: Vec<ResolveError>,
}

impl Resolver {
    pub fn new() -> Self {
        let mut root_symbols = HashMap::new();
        Self::register_builtin_type(&mut root_symbols, "Int");
        Self::register_builtin_type(&mut root_symbols, "U8");
        Self::register_builtin_type(&mut root_symbols, "U32");
        Self::register_builtin_type(&mut root_symbols, "Usize");
        Self::register_builtin_type(&mut root_symbols, "Bool");
        Self::register_builtin_type(&mut root_symbols, "Unit");
        Self::register_builtin_type(&mut root_symbols, "Self");
        Self::register_builtin_type(&mut root_symbols, "Result");
        Self::register_builtin_type(&mut root_symbols, "Option");
        Self::register_builtin_type(&mut root_symbols, "ExitCode");
        Self::register_builtin_type(&mut root_symbols, "String");
        Self::register_builtin_type(&mut root_symbols, "Vec");
        Self::register_builtin_type(&mut root_symbols, "HashMap");

        let root_scope = Scope {
            kind: ScopeKind::Root,
            symbols: root_symbols,
        };

        Self {
            scopes: vec![root_scope],
            errors: Vec::new(),
        }
    }

    fn register_builtin_type(symbols: &mut HashMap<String, Symbol>, name: &str) {
        let dummy_span = Span::new(0, 0, 1, 1);
        symbols.insert(
            name.to_string(),
            Symbol {
                name: name.to_string(),
                kind: SymbolKind::Type,
                is_pub: true,
                span: dummy_span,
                is_builtin: true,
                used: true,
            },
        );
    }

    pub fn resolve_program(&mut self, items: &[Spanned<Item>]) -> Result<(), Vec<ResolveError>> {
        self.declare_items(items);
        self.resolve_items(items);

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }

    fn declare_items(&mut self, items: &[Spanned<Item>]) {
        let mut idx = 0;
        while idx < items.len() {
            let spanned_item = &items[idx];
            self.declare_item(spanned_item);
            idx += 1;
        }
    }

    fn declare_item(&mut self, item: &Spanned<Item>) {
        match &item.node.kind {
            ItemKind::Const { is_pub, name, ty, init } => {
                self.declare_symbol(name.clone(), SymbolKind::Const, *is_pub, item.span);
                self.resolve_type(ty);
                self.resolve_expr(init);
            }
            ItemKind::TypeDecl { is_pub, name, kind, .. } => {
                self.declare_symbol(name.clone(), SymbolKind::Type, *is_pub, item.span);
                if let TypeKind::Enum(variants) = kind {
                    let mut variant_idx = 0;
                    while variant_idx < variants.len() {
                        let variant = &variants[variant_idx];
                        self.declare_symbol(
                            variant.name.clone(),
                            SymbolKind::Function,
                            variant.is_pub || *is_pub,
                            item.span,
                        );
                        variant_idx += 1;
                    }
                }
            }
            ItemKind::Function { is_pub, name, .. } => {
                self.declare_symbol(name.clone(), SymbolKind::Function, *is_pub, item.span);
            }
            ItemKind::Module { is_pub, name, .. } => {
                self.declare_symbol(name.clone(), SymbolKind::Module, *is_pub, item.span);
            }
            ItemKind::Use { is_pub, path, alias } => {
                let local_name = match alias {
                    Some(alias_name) => alias_name.clone(),
                    None => match path.last() {
                        Some(last_segment) => last_segment.clone(),
                        None => return,
                    },
                };
                self.declare_symbol(local_name, SymbolKind::Import, *is_pub, item.span);
            }
            ItemKind::Trait { is_pub, name, .. } => {
                self.declare_symbol(name.clone(), SymbolKind::Trait, *is_pub, item.span);
            }
            ItemKind::Impl { trait_name, target_type, .. } => {
                self.resolve_path(std::slice::from_ref(trait_name), item.span);
                self.resolve_type(target_type);
            }
        }
    }

    fn declare_symbol(&mut self, name: String, kind: SymbolKind, is_pub: bool, span: Span) {
        let current_scope = match self.scopes.last_mut() {
            Some(scope_ref) => scope_ref,
            None => return,
        };

        if let Some(existing_symbol) = current_scope.symbols.get(&name) {
            if existing_symbol.is_builtin {
                current_scope.symbols.insert(
                    name.clone(),
                    Symbol {
                        name,
                        kind,
                        is_pub,
                        span,
                        is_builtin: false,
                        used: false,
                    },
                );
                return;
            }

            if existing_symbol.kind == SymbolKind::Type && (kind == SymbolKind::Function || kind == SymbolKind::Const) {
                return;
            }

            self.errors.push(ResolveError {
                message: format!(
                    "Duplicate declaration of symbol '{}' in this scope (originally declared at line {}, col {})",
                    name, existing_symbol.span.line, existing_symbol.span.col
                ),
                span,
            });
            return;
        }

        current_scope.symbols.insert(
            name.clone(),
            Symbol {
                name,
                kind,
                is_pub,
                span,
                is_builtin: false,
                used: false,
            },
        );
    }

    fn resolve_items(&mut self, items: &[Spanned<Item>]) {
        let mut idx = 0;
        while idx < items.len() {
            let spanned_item = &items[idx];
            self.resolve_item(spanned_item);
            idx += 1;
        }
    }

    fn resolve_item(&mut self, item: &Spanned<Item>) {
        match &item.node.kind {
            ItemKind::Const { ty, init, .. } => {
                self.resolve_type(ty);
                self.resolve_expr(init);
            }
            ItemKind::TypeDecl { generics, kind, .. } => {
                self.push_scope(ScopeKind::Block);
                let mut idx = 0;
                while idx < generics.len() {
                    let gen_param = &generics[idx];
                    self.declare_symbol(gen_param.name.clone(), SymbolKind::Type, false, item.span);
                    idx += 1;
                }

                match kind {
                    TypeKind::Struct(fields) => {
                        let mut field_idx = 0;
                        while field_idx < fields.len() {
                            self.resolve_type(&fields[field_idx].ty);
                            field_idx += 1;
                        }
                    }
                    TypeKind::Enum(variants) => {
                        let mut variant_idx = 0;
                        while variant_idx < variants.len() {
                            let variant = &variants[variant_idx];
                            let mut tuple_idx = 0;
                            while tuple_idx < variant.types.len() {
                                self.resolve_type(&variant.types[tuple_idx]);
                                tuple_idx += 1;
                            }
                            variant_idx += 1;
                        }
                    }
                    TypeKind::Native | TypeKind::Unit => {}
                }
                self.pop_scope();
            }
            ItemKind::Function {
                name,
                generics,
                params,
                return_ty,
                body,
                ..
            } => {
                self.push_scope(ScopeKind::Function(name.clone()));

                let mut gen_idx = 0;
                while gen_idx < generics.len() {
                    let gen_param = &generics[gen_idx];
                    self.declare_symbol(gen_param.name.clone(), SymbolKind::Type, false, item.span);
                    gen_idx += 1;
                }

                let mut param_idx = 0;
                while param_idx < params.len() {
                    let param = &params[param_idx];
                    self.declare_symbol(param.name.clone(), SymbolKind::Param, false, item.span);
                    self.resolve_type(&param.ty);
                    param_idx += 1;
                }

                if let Some(ret_type) = return_ty {
                    self.resolve_type(ret_type);
                }

                if let Some(body_stmts) = body {
                    self.resolve_stmts(body_stmts);
                }

                self.pop_scope();
            }
            ItemKind::Module { name, is_pub, items: child_items } => {
                self.push_scope(ScopeKind::Module(name.clone(), *is_pub));
                self.declare_items(child_items);
                self.resolve_items(child_items);
                self.pop_scope();
            }
            ItemKind::Use { path, .. } => {
                self.resolve_use_path(path, item.span);
            }
            ItemKind::Trait { name, methods, .. } => {
                self.push_scope(ScopeKind::Trait(name.clone()));
                self.declare_items(methods);
                self.resolve_items(methods);
                self.pop_scope();
            }
            ItemKind::Impl { trait_name, target_type, methods, .. } => {
                self.push_scope(ScopeKind::Impl);
                self.resolve_path(std::slice::from_ref(trait_name), item.span);
                self.resolve_type(target_type);
                self.declare_items(methods);
                self.resolve_items(methods);
                self.pop_scope();
            }
        }
    }

    fn resolve_use_path(&mut self, path: &[String], span: Span) {
        if path.is_empty() {
            return;
        }
        let root_module_name = &path[0];
        if !self.symbol_exists(root_module_name) {
            self.errors.push(ResolveError {
                message: format!("Unknown module '{}' in use import", root_module_name),
                span,
            });
        }
    }

    fn resolve_stmts(&mut self, stmts: &[Spanned<Stmt>]) {
        let mut idx = 0;
        while idx < stmts.len() {
            self.resolve_stmt(&stmts[idx]);
            idx += 1;
        }
    }

    fn resolve_stmt(&mut self, stmt: &Spanned<Stmt>) {
        match &stmt.node {
            Stmt::Val { name, ty, init } => {
                if let Some(spanned_type) = ty {
                    self.resolve_type(spanned_type);
                }
                self.resolve_expr(init);
                self.declare_symbol(name.clone(), SymbolKind::Val, false, stmt.span);
            }
            Stmt::Var { name, ty, init } => {
                if let Some(spanned_type) = ty {
                    self.resolve_type(spanned_type);
                }
                self.resolve_expr(init);
                self.declare_symbol(name.clone(), SymbolKind::Var, false, stmt.span);
            }
            Stmt::Assign { name, expr } => {
                self.resolve_symbol_reference(name, stmt.span);
                self.resolve_expr(expr);
            }
            Stmt::Expr(expr) => {
                self.resolve_expr(expr);
            }
            Stmt::While { cond, body } => {
                self.resolve_expr(cond);
                self.push_scope(ScopeKind::Block);
                self.resolve_stmts(body);
                self.pop_scope();
            }
        }
    }

    fn resolve_expr(&mut self, expr: &Spanned<Expr>) {
        match &expr.node {
            Expr::Lit(_) | Expr::Break | Expr::Continue => {}
            Expr::Var(name) | Expr::Const(name) => {
                self.resolve_symbol_reference(name, expr.span);
            }
            Expr::Path(path) => {
                self.resolve_path(path, expr.span);
            }
            Expr::Binary(left, _, right) => {
                self.resolve_expr(left);
                self.resolve_expr(right);
            }
            Expr::Unary(_, inner) | Expr::Try(inner) => {
                self.resolve_expr(inner);
            }
            Expr::Return(opt_expr) => {
                if let Some(inner_expr) = opt_expr {
                    self.resolve_expr(inner_expr);
                }
            }
            Expr::Call(func, type_args, args) => {
                self.resolve_expr(func);
                if let Some(t_args) = type_args {
                    let mut type_idx = 0;
                    while type_idx < t_args.len() {
                        self.resolve_type(&t_args[type_idx]);
                        type_idx += 1;
                    }
                }
                let mut arg_idx = 0;
                while arg_idx < args.len() {
                    self.resolve_expr(&args[arg_idx]);
                    arg_idx += 1;
                }
            }
            Expr::StructInit(struct_name, fields) => {
                self.resolve_symbol_reference(struct_name, expr.span);
                let mut field_idx = 0;
                while field_idx < fields.len() {
                    self.resolve_expr(&fields[field_idx].1);
                    field_idx += 1;
                }
            }
            Expr::FieldAccess(target, _) => {
                self.resolve_expr(target);
            }
            Expr::ArrayLit(elements) => {
                let mut elem_idx = 0;
                while elem_idx < elements.len() {
                    self.resolve_expr(&elements[elem_idx]);
                    elem_idx += 1;
                }
            }
            Expr::Match(target, arms) => {
                self.resolve_expr(target);
                let mut arm_idx = 0;
                while arm_idx < arms.len() {
                    let (pattern, body_expr) = &arms[arm_idx];
                    self.push_scope(ScopeKind::Block);
                    self.resolve_pattern(pattern);
                    self.resolve_expr(body_expr);
                    self.pop_scope();
                    arm_idx += 1;
                }
            }
            Expr::If(cond, then_body, else_body) => {
                self.resolve_expr(cond);
                self.push_scope(ScopeKind::Block);
                self.resolve_stmts(then_body);
                self.pop_scope();

                if let Some(else_stmts) = else_body {
                    self.push_scope(ScopeKind::Block);
                    self.resolve_stmts(else_stmts);
                    self.pop_scope();
                }
            }
        }
    }

    fn resolve_pattern(&mut self, pattern: &Spanned<Pattern>) {
        match &pattern.node {
            Pattern::Lit(_) => {}
            Pattern::Var(name) | Pattern::Rest(name) => {
                self.declare_symbol(name.clone(), SymbolKind::Val, false, pattern.span);
            }
            Pattern::Variant(variant_name, sub_patterns) => {
                self.resolve_symbol_reference(variant_name, pattern.span);
                let mut sub_idx = 0;
                while sub_idx < sub_patterns.len() {
                    self.resolve_pattern(&sub_patterns[sub_idx]);
                    sub_idx += 1;
                }
            }
            Pattern::PathVariant(path, sub_patterns) => {
                self.resolve_path(path, pattern.span);
                let mut sub_idx = 0;
                while sub_idx < sub_patterns.len() {
                    self.resolve_pattern(&sub_patterns[sub_idx]);
                    sub_idx += 1;
                }
            }
            Pattern::Array(elements) => {
                let mut elem_idx = 0;
                while elem_idx < elements.len() {
                    self.resolve_pattern(&elements[elem_idx]);
                    elem_idx += 1;
                }
            }
        }
    }

    fn resolve_type(&mut self, ty: &Spanned<Type>) {
        match &ty.node {
            Type::Named(name) => {
                self.resolve_symbol_reference(name, ty.span);
            }
            Type::Path(path) => {
                self.resolve_path(path, ty.span);
            }
            Type::Generic(name, args) => {
                self.resolve_symbol_reference(name, ty.span);
                let mut idx = 0;
                while idx < args.len() {
                    self.resolve_type(&args[idx]);
                    idx += 1;
                }
            }
            Type::GenericPath(path, args) => {
                self.resolve_path(path, ty.span);
                let mut idx = 0;
                while idx < args.len() {
                    self.resolve_type(&args[idx]);
                    idx += 1;
                }
            }
            Type::Array(inner_type, _) => {
                self.resolve_type(inner_type);
            }
            Type::Slice(inner_type) | Type::Ref(inner_type) | Type::RefMut(inner_type) => {
                self.resolve_type(inner_type);
            }
        }
    }

    fn lookup_symbol(&self, name: &str) -> Option<(&Symbol, &ScopeKind)> {
        let mut idx = self.scopes.len();
        while idx > 0 {
            idx -= 1;
            if let Some(sym) = self.scopes[idx].symbols.get(name) {
                return Some((sym, &self.scopes[idx].kind));
            }
        }
        None
    }

    fn is_inside_scope(&self, target_scope_kind: &ScopeKind) -> bool {
        let mut idx = 0;
        while idx < self.scopes.len() {
            if &self.scopes[idx].kind == target_scope_kind {
                return true;
            }
            idx += 1;
        }
        false
    }

    fn resolve_symbol_reference(&mut self, name: &str, span: Span) {
        if let Some((symbol, decl_scope_kind)) = self.lookup_symbol(name) {
            if let ScopeKind::Module(mod_name, is_mod_pub) = decl_scope_kind
                && (!symbol.is_pub || !is_mod_pub)
                && !self.is_inside_scope(decl_scope_kind)
            {
                self.errors.push(ResolveError {
                    message: format!("Symbol '{}' is private in module '{}'", symbol.name, mod_name),
                    span,
                });
            }
            self.mark_symbol_used(name);
        } else {
            self.errors.push(ResolveError {
                message: format!("Unknown variable or symbol '{}'", name),
                span,
            });
        }
    }

    fn resolve_path(&mut self, path: &[String], span: Span) {
        if path.is_empty() {
            return;
        }
        let root_name = &path[0];
        if !self.symbol_exists(root_name) {
            self.errors.push(ResolveError {
                message: format!("Unknown root symbol '{}' in path", root_name),
                span,
            });
        }
    }

    fn symbol_exists(&self, name: &str) -> bool {
        self.lookup_symbol(name).is_some()
    }

    fn mark_symbol_used(&mut self, name: &str) {
        let mut idx = self.scopes.len();
        while idx > 0 {
            idx -= 1;
            if let Some(symbol_ref) = self.scopes[idx].symbols.get_mut(name) {
                symbol_ref.used = true;
                return;
            }
        }
    }

    fn push_scope(&mut self, kind: ScopeKind) {
        self.scopes.push(Scope {
            kind,
            symbols: HashMap::new(),
        });
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }
}