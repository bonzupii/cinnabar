use crate::lexer::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericParam {
    pub name: String,
    pub bounds: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Named(String),
    Path(Vec<String>),
    Generic(String, Vec<Spanned<Type>>),
    GenericPath(Vec<String>, Vec<Spanned<Type>>),
    Array(Box<Spanned<Type>>, usize),
    Slice(Box<Spanned<Type>>),
    Ref(Box<Spanned<Type>>),
    RefMut(Box<Spanned<Type>>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lit {
    Int(i64),
    Hex(u64),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinOp {
    Add, Sub, Mul, Div,
    Eq, NotEq, Lt, Gt, LtEq, GtEq,
    BitAnd, BitOr, BitXor, Shl, Shr,
    And, Or,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnOp {
    Not, Neg, Ref, RefMut,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Lit(Lit),
    Var(String),
    Variant(String, Vec<Spanned<Pattern>>),
    PathVariant(Vec<String>, Vec<Spanned<Pattern>>),
    Array(Vec<Spanned<Pattern>>),
    Rest(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Lit(Lit),
    Var(String),
    Const(String),
    Path(Vec<String>),
    Binary(Box<Spanned<Expr>>, BinOp, Box<Spanned<Expr>>),
    Unary(UnOp, Box<Spanned<Expr>>),
    Try(Box<Spanned<Expr>>),
    Return(Option<Box<Spanned<Expr>>>),
    Break,
    Continue,
    Call(Box<Spanned<Expr>>, Option<Vec<Spanned<Type>>>, Vec<Spanned<Expr>>),
    StructInit(String, Vec<(String, Spanned<Expr>)>),
    FieldAccess(Box<Spanned<Expr>>, String),
    ArrayLit(Vec<Spanned<Expr>>),
    Match(Box<Spanned<Expr>>, Vec<(Spanned<Pattern>, Spanned<Expr>)>),
    If(Box<Spanned<Expr>>, Vec<Spanned<Stmt>>, Option<Vec<Spanned<Stmt>>>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Val { name: String, ty: Option<Spanned<Type>>, init: Spanned<Expr> },
    Var { name: String, ty: Option<Spanned<Type>>, init: Spanned<Expr> },
    Assign { name: String, expr: Spanned<Expr> },
    Expr(Spanned<Expr>),
    While { cond: Spanned<Expr>, body: Vec<Spanned<Stmt>> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Spanned<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub is_pub: bool,
    pub name: String,
    pub ty: Spanned<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub is_pub: bool,
    pub name: String,
    pub types: Vec<Spanned<Type>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Native,
    Unit,
    Enum(Vec<EnumVariant>),
    Struct(Vec<StructField>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    Const {
        is_pub: bool,
        name: String,
        ty: Spanned<Type>,
        init: Spanned<Expr>,
    },
    TypeDecl {
        is_pub: bool,
        name: String,
        generics: Vec<GenericParam>,
        kind: TypeKind,
    },
    Function {
        is_pub: bool,
        is_native: bool,
        is_impure: bool,
        name: String,
        generics: Vec<GenericParam>,
        params: Vec<Param>,
        return_ty: Option<Spanned<Type>>,
        body: Option<Vec<Spanned<Stmt>>>,
    },
    Module {
        is_pub: bool,
        name: String,
        items: Vec<Spanned<Item>>,
    },
    Use {
        is_pub: bool,
        path: Vec<String>,
        alias: Option<String>,
    },
    Trait {
        is_pub: bool,
        name: String,
        methods: Vec<Spanned<Item>>,
    },
    Impl {
        is_pub: bool,
        trait_name: String,
        target_type: Spanned<Type>,
        methods: Vec<Spanned<Item>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub doc: Option<String>,
    pub kind: ItemKind,
}
