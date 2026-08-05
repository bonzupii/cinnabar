pub mod checker;
pub mod collector;
pub mod env;
pub mod error;
pub mod types;

use crate::ast::Type;
pub use env::VariableInfo;
pub use error::TypeError;
use std::collections::HashMap;

pub struct TypeChecker {
    pub variables: Vec<HashMap<String, VariableInfo>>,
    pub functions: HashMap<String, (Vec<String>, Vec<Type>, Type)>,
    pub structs: HashMap<String, HashMap<String, Type>>,
    pub module_stack: Vec<String>,
    pub current_module_types: Vec<String>,
    pub current_return_type: Option<Type>,
    pub errors: Vec<TypeError>,
}

impl TypeChecker {
    pub fn new() -> Self {
        let mut checker = Self {
            variables: vec![HashMap::new()],
            functions: HashMap::new(),
            structs: HashMap::new(),
            module_stack: Vec::new(),
            current_module_types: Vec::new(),
            current_return_type: None,
            errors: Vec::new(),
        };

        checker.register_builtins();
        checker
    }

    pub fn declare_variable(&mut self, name: String, ty: Type, is_mutable: bool) {
        if let Some(current_scope) = self.variables.last_mut() {
            current_scope.insert(name, VariableInfo { ty, is_mutable });
        }
    }

    pub fn lookup_variable(&self, name: &str) -> Option<&VariableInfo> {
        let mut idx = self.variables.len();
        while idx > 0 {
            idx -= 1;
            if let Some(info) = self.variables[idx].get(name) {
                return Some(info);
            }
        }
        None
    }

    pub fn push_scope(&mut self) {
        self.variables.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        if self.variables.len() > 1 {
            self.variables.pop();
        }
    }
}
