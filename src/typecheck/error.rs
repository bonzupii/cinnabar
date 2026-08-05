use crate::lexer::Span;
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