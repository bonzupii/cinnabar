//! Typed codegen errors.
//!
//! Every failure in codegen is reported as a `CodegenError` carrying the
//! real source span of the construct that failed and a typed kind.  Only
//! the final diagnostic may stringify, through `message()`.  A span of
//! `NO_FILE` marks a failure with no Cinnabar source origin (a toolchain
//! failure); the diagnostic model represents that explicitly and never
//! invents a source location.

/// A typed failure kind.  `Tool` carries the tool name, its exit status
/// when one was observed, and the tail of its standard error.
pub enum CodegenErrorKind {
    Builder(String),
    Io(String),
    Tool {
        tool: String,
        status: Option<i32>,
        detail: String,
    },
}

/// A codegen failure: `(file, start, end)` is the source span of the
/// failing construct, or `(-1, 0, 0)` when the failure has no source
/// origin.
pub struct CodegenError {
    pub span: (i64, i64, i64),
    pub kind: CodegenErrorKind,
}

/// Renders the final diagnostic text for a codegen failure.
pub fn codegen_error_message(err: &CodegenError) -> String {
    match &err.kind {
        CodegenErrorKind::Builder(detail) => format!("LLVM builder failure: {}", detail),
        CodegenErrorKind::Io(detail) => format!("I/O failure: {}", detail),
        CodegenErrorKind::Tool {
            tool,
            status,
            detail,
        } => {
            let status_text = match status {
                Some(code) => format!("exit status {}", code),
                None => "terminated by signal".to_string(),
            };
            format!("{} failed with {}: {}", tool, status_text, detail)
        }
    }
}

/// Maps an LLVM builder failure into a codegen error.  Builder failures
/// are internal LLVM errors with no Cinnabar source origin, so the span
/// is the sanctioned source-less `(-1, 0, 0)`.
pub fn builder_fail(err: inkwell::builder::BuilderError) -> CodegenError {
    CodegenError {
        span: (-1, 0, 0),
        kind: CodegenErrorKind::Builder(err.to_string()),
    }
}

/// Builds a builder-failure error at `(file, start, end)`.
pub fn builder_error(file: i64, start: i64, end: i64, detail: &str) -> CodegenError {
    CodegenError {
        span: (file, start, end),
        kind: CodegenErrorKind::Builder(detail.to_string()),
    }
}

/// Builds an I/O failure error with no source origin.
pub fn io_error(detail: &str) -> CodegenError {
    CodegenError {
        span: (-1, 0, 0),
        kind: CodegenErrorKind::Io(detail.to_string()),
    }
}

/// Builds a tool-failure error with no source origin.
pub fn tool_error(tool: &str, status: Option<i32>, detail: &str) -> CodegenError {
    CodegenError {
        span: (-1, 0, 0),
        kind: CodegenErrorKind::Tool {
            tool: tool.to_string(),
            status,
            detail: detail.to_string(),
        },
    }
}
