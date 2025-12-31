//! Runtime errors for the Neap VM

use std::fmt;

use crate::ir::VarId;

/// Errors that can occur during program execution.
#[derive(Debug, Clone)]
pub enum RuntimeError {
    /// Division by zero
    DivisionByZero,

    /// Unbound variable
    UnboundVariable { var: VarId },

    /// Unbound global variable
    UnboundGlobal { name: String },

    /// Type error at runtime (shouldn't happen with type checking)
    TypeError { expected: String, got: String },

    /// Pattern match failure (non-exhaustive patterns)
    MatchFailure,

    /// Index out of bounds (tuple projection)
    IndexOutOfBounds { index: usize, len: usize },

    /// Unknown field in record
    UnknownField { field: String },

    /// IO error
    IoError { message: String },

    /// Assertion failure
    AssertionFailed { message: String },

    /// Stack overflow (too deep recursion)
    StackOverflow,

    /// User-defined error (from Result::Err)
    UserError { message: String },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DivisionByZero => write!(f, "division by zero"),
            Self::UnboundVariable { var } => write!(f, "unbound variable: {var}"),
            Self::UnboundGlobal { name } => write!(f, "unbound global: {name}"),
            Self::TypeError { expected, got } => {
                write!(f, "type error: expected {expected}, got {got}")
            }
            Self::MatchFailure => write!(f, "pattern match failure"),
            Self::IndexOutOfBounds { index, len } => {
                write!(f, "index {index} out of bounds (length {len})")
            }
            Self::UnknownField { field } => write!(f, "unknown field: {field}"),
            Self::IoError { message } => write!(f, "IO error: {message}"),
            Self::AssertionFailed { message } => write!(f, "assertion failed: {message}"),
            Self::StackOverflow => write!(f, "stack overflow"),
            Self::UserError { message } => write!(f, "error: {message}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<std::io::Error> for RuntimeError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError {
            message: e.to_string(),
        }
    }
}
