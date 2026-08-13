//! The typed error codegen carries until the driver renders it.
//!
//! Every failure at or below IR emission — an LLVM builder rejection, an
//! I/O failure, a failed `opt`, `llc`, or `clang` invocation — becomes a
//! `CodegenError` with its kind preserved, so no intermediate step has to
//! stringify and no detail is lost on the way up. `codegen_error_message`
//! is the single place one is rendered, and the driver calls it, not the
//! code that raised the error.
//!
//! **Invariants:**
//! - Codegen failures are values, never panics. That is what lets the CLI
//!   report a tool failure as a diagnostic instead of a backtrace.
//! - A failure with no Cinnabar source origin carries `NO_FILE` rather than
//!   a plausible-looking location. `clang` failing to link is not a fact
//!   about line 1 of the user's program, and saying so would be a
//!   fabricated span.

pub enum CodegenErrorKind {
    Builder(String),
    Io(String),
    Tool {
        tool: String,
        status: Option<i32>,
        detail: String,
    },
}

pub struct CodegenError {
    pub span: (i64, i64, i64),
    pub kind: CodegenErrorKind,
}

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

pub fn builder_fail(err: inkwell::builder::BuilderError) -> CodegenError {
    CodegenError {
        span: (-1, 0, 0),
        kind: CodegenErrorKind::Builder(err.to_string()),
    }
}

pub fn builder_error(file: i64, start: i64, end: i64, detail: &str) -> CodegenError {
    CodegenError {
        span: (file, start, end),
        kind: CodegenErrorKind::Builder(detail.to_string()),
    }
}

pub fn io_error(detail: &str) -> CodegenError {
    CodegenError {
        span: (-1, 0, 0),
        kind: CodegenErrorKind::Io(detail.to_string()),
    }
}

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
