use crate::ast::Type;

#[derive(Debug, Clone)]
pub struct VariableInfo {
    pub ty: Type,
    pub is_mutable: bool,
}