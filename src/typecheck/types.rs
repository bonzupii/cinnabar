// ====== FILE: ./src/typecheck/types.rs ======
use crate::ast::Spanned;
use crate::ast::Type;
use crate::typecheck::TypeChecker;
use std::collections::HashMap;

impl TypeChecker {
    pub fn qualify_type(&self, ty: &Type, local_types: &[String]) -> Type {
        if self.module_stack.is_empty() {
            return ty.clone();
        }
        match ty {
            Type::Named(name) => {
                if local_types.contains(name) {
                    let mut path = self.module_stack.clone();
                    path.push(name.clone());
                    Type::Path(path)
                } else {
                    Type::Named(name.clone())
                }
            }
            Type::Generic(name, args) => {
                let is_local = local_types.contains(name);
                let mut qual_args = Vec::new();
                let mut idx = 0;
                while idx < args.len() {
                    let sub_node = self.qualify_type(&args[idx].node, local_types);
                    qual_args.push(Spanned::new(sub_node, args[idx].span));
                    idx += 1;
                }
                if is_local {
                    let mut path = self.module_stack.clone();
                    path.push(name.clone());
                    Type::GenericPath(path, qual_args)
                } else {
                    Type::Generic(name.clone(), qual_args)
                }
            }
            Type::Path(path) => Type::Path(path.clone()),
            Type::GenericPath(path, args) => {
                let mut qual_args = Vec::new();
                let mut idx = 0;
                while idx < args.len() {
                    let sub_node = self.qualify_type(&args[idx].node, local_types);
                    qual_args.push(Spanned::new(sub_node, args[idx].span));
                    idx += 1;
                }
                Type::GenericPath(path.clone(), qual_args)
            }
            Type::Array(inner, size) => {
                let qual_inner = self.qualify_type(&inner.node, local_types);
                Type::Array(Box::new(Spanned::new(qual_inner, inner.span)), *size)
            }
            Type::Slice(inner) => {
                let qual_inner = self.qualify_type(&inner.node, local_types);
                Type::Slice(Box::new(Spanned::new(qual_inner, inner.span)))
            }
            Type::Ref(inner) => {
                let qual_inner = self.qualify_type(&inner.node, local_types);
                Type::Ref(Box::new(Spanned::new(qual_inner, inner.span)))
            }
            Type::RefMut(inner) => {
                let qual_inner = self.qualify_type(&inner.node, local_types);
                Type::RefMut(Box::new(Spanned::new(qual_inner, inner.span)))
            }
        }
    }

    pub fn substitute_generics(&self, ty: &Type, bindings: &HashMap<String, Type>) -> Type {
        match ty {
            Type::Named(name) => {
                if let Some(sub_ty) = bindings.get(name) {
                    sub_ty.clone()
                } else {
                    Type::Named(name.clone())
                }
            }
            Type::Path(path) => {
                if path.len() == 1 && let Some(sub_ty) = bindings.get(&path[0]) {
                    sub_ty.clone()
                } else {
                    Type::Path(path.clone())
                }
            }
            Type::Generic(name, args) => {
                let mut new_args = Vec::new();
                let mut idx = 0;
                while idx < args.len() {
                    let sub_arg_node = self.substitute_generics(&args[idx].node, bindings);
                    new_args.push(Spanned::new(sub_arg_node, args[idx].span));
                    idx += 1;
                }
                Type::Generic(name.clone(), new_args)
            }
            Type::GenericPath(path, args) => {
                let mut new_args = Vec::new();
                let mut idx = 0;
                while idx < args.len() {
                    let sub_arg_node = self.substitute_generics(&args[idx].node, bindings);
                    new_args.push(Spanned::new(sub_arg_node, args[idx].span));
                    idx += 1;
                }
                Type::GenericPath(path.clone(), new_args)
            }
            Type::Array(inner, size) => {
                let sub_inner = self.substitute_generics(&inner.node, bindings);
                Type::Array(Box::new(Spanned::new(sub_inner, inner.span)), *size)
            }
            Type::Slice(inner) => {
                let sub_inner = self.substitute_generics(&inner.node, bindings);
                Type::Slice(Box::new(Spanned::new(sub_inner, inner.span)))
            }
            Type::Ref(inner) => {
                let sub_inner = self.substitute_generics(&inner.node, bindings);
                Type::Ref(Box::new(Spanned::new(sub_inner, inner.span)))
            }
            Type::RefMut(inner) => {
                let sub_inner = self.substitute_generics(&inner.node, bindings);
                Type::RefMut(Box::new(Spanned::new(sub_inner, inner.span)))
            }
        }
    }

    pub fn infer_generic_bindings(&self, param_ty: &Type, arg_ty: &Type, bindings: &mut HashMap<String, Type>) {
        match param_ty {
            Type::Named(name) => {
                if name == "T" || name == "K" || name == "V" || name == "E" || name == "U" {
                    bindings.insert(name.clone(), arg_ty.clone());
                }
            }
            Type::Path(path) => {
                if path.len() == 1 {
                    let name = &path[0];
                    if name == "T" || name == "K" || name == "V" || name == "E" || name == "U" {
                        bindings.insert(name.clone(), arg_ty.clone());
                    }
                }
            }
            Type::Ref(p_inner) => {
                if let Type::Ref(a_inner) = arg_ty {
                    self.infer_generic_bindings(&p_inner.node, &a_inner.node, bindings);
                }
            }
            Type::RefMut(p_inner) => {
                if let Type::RefMut(a_inner) = arg_ty {
                    self.infer_generic_bindings(&p_inner.node, &a_inner.node, bindings);
                }
            }
            Type::Slice(p_inner) => {
                if let Type::Slice(a_inner) = arg_ty {
                    self.infer_generic_bindings(&p_inner.node, &a_inner.node, bindings);
                }
            }
            Type::Array(p_inner, p_sz) => {
                if let Type::Array(a_inner, a_sz) = arg_ty
                    && p_sz == a_sz
                {
                    self.infer_generic_bindings(&p_inner.node, &a_inner.node, bindings);
                }
            }
            Type::Generic(p_name, p_args) => {
                let a_args = match arg_ty {
                    Type::Generic(a_name, args) if p_name == a_name => Some(args),
                    Type::GenericPath(a_path, args) if a_path.last() == Some(p_name) => Some(args),
                    _ => None,
                };
                if let Some(args) = a_args
                    && p_args.len() == args.len()
                {
                    let mut idx = 0;
                    while idx < p_args.len() {
                        self.infer_generic_bindings(&p_args[idx].node, &args[idx].node, bindings);
                        idx += 1;
                    }
                }
            }
            Type::GenericPath(p_path, p_args) => {
                let a_args = match arg_ty {
                    Type::GenericPath(a_path, args) if p_path == a_path || (p_path.len() == 1 && a_path.last() == p_path.first()) => Some(args),
                    Type::Generic(a_name, args) if p_path.last() == Some(a_name) => Some(args),
                    _ => None,
                };
                if let Some(args) = a_args
                    && p_args.len() == args.len()
                {
                    let mut idx = 0;
                    while idx < p_args.len() {
                        self.infer_generic_bindings(&p_args[idx].node, &args[idx].node, bindings);
                        idx += 1;
                    }
                }
            }
        }
    }

    pub fn is_dummy_unit(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(name) if name == "Unit")
    }

    pub fn is_dummy_range_error(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(name) if name == "RangeError")
    }

    pub fn is_integer_type(&self, ty: &Type) -> bool {
        if let Type::Named(name) = ty {
            name == "Int" || name == "U8" || name == "U32" || name == "Usize"
        } else {
            false
        }
    }

    pub fn is_result_type(&self, ty: &Type) -> bool {
        if let Type::Generic(name, args) = ty {
            !args.is_empty() && name == "Result"
        } else {
            false
        }
    }

    pub fn is_option_type(&self, ty: &Type) -> bool {
        if let Type::Generic(name, args) = ty {
            !args.is_empty() && name == "Option"
        } else {
            false
        }
    }

    pub fn types_equal(&self, t1: &Type, t2: &Type) -> bool {
        match t1 {
            Type::Named(n1) => {
                if let Type::Named(n2) = t2 {
                    n1 == n2
                } else {
                    false
                }
            }
            Type::Path(p1) => {
                if let Type::Path(p2) = t2 {
                    p1 == p2
                } else {
                    false
                }
            }
            Type::Generic(n1, args1) => {
                if let Type::Generic(n2, args2) = t2 {
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
                } else {
                    false
                }
            }
            Type::GenericPath(p1, args1) => {
                if let Type::GenericPath(p2, args2) = t2 {
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
                } else {
                    false
                }
            }
            Type::Array(ty1, sz1) => {
                if let Type::Array(ty2, sz2) = t2 {
                    sz1 == sz2 && self.types_equal(&ty1.node, &ty2.node)
                } else {
                    false
                }
            }
            Type::Slice(ty1) => {
                if let Type::Slice(ty2) = t2 {
                    self.types_equal(&ty1.node, &ty2.node)
                } else {
                    false
                }
            }
            Type::Ref(ty1) => {
                if let Type::Ref(ty2) = t2 {
                    self.types_equal(&ty1.node, &ty2.node)
                } else {
                    false
                }
            }
            Type::RefMut(ty1) => {
                if let Type::RefMut(ty2) = t2 {
                    self.types_equal(&ty1.node, &ty2.node)
                } else {
                    false
                }
            }
        }
    }

    pub fn types_match(&self, expected: &Type, actual: &Type) -> bool {
        if self.types_equal(expected, actual) {
            return true;
        }
        if self.is_integer_type(expected) && self.is_integer_type(actual) {
            return true;
        }
        false
    }
}