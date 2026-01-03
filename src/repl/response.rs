//! Response types for REPL evaluation.
//!
//! This module provides structured response types that make it easy to
//! inspect evaluation results in tests.

use std::fmt;

use super::engine::{CommandResult, EvalResult};
use crate::types::Type;
use crate::vm::Value;

/// A structured response from REPL evaluation.
///
/// This type provides convenient accessors for testing and inspection.
#[derive(Debug, Clone)]
pub struct Response {
    /// The formatted text output
    text: String,
    /// The structured response kind
    kind: ResponseKind,
}

/// The kind of response.
#[derive(Debug, Clone)]
pub enum ResponseKind {
    /// A value was bound: `val <name> : <type> = <value>`
    Value {
        name: String,
        ty: String,
        value: String,
    },
    /// A function was defined: `fun <name> : <type>`
    Function { name: String, ty: String },
    /// A type was defined: `type <name>` or `datatype <name>`
    TypeDef { name: String, kind: TypeDefKind },
    /// A type query result: `:type <expr>` -> `<type>`
    TypeQuery { ty: String },
    /// An error occurred
    Error { message: String },
    /// Empty input or acknowledgement
    Empty,
}

/// Kind of type definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeDefKind {
    /// A type alias
    Type,
    /// A datatype definition
    Datatype,
}

impl Response {
    /// Create a response from an evaluation result.
    pub fn from_eval_result(result: EvalResult) -> Self {
        match result {
            EvalResult::Expression { ty, value } => Self::value("it", ty, value),
            EvalResult::ValDecl { name, ty, value } => Self::value(&name, ty, value),
            EvalResult::FunDecl { name, ty } => Self::function(&name, ty),
            EvalResult::TypeDecl { name } => Self::type_def(&name, TypeDefKind::Type),
            EvalResult::DatatypeDecl { name } => Self::type_def(&name, TypeDefKind::Datatype),
            EvalResult::Empty => Self::empty(),
        }
    }

    /// Create a response from a command result.
    pub fn from_command_result(result: CommandResult) -> Self {
        match result {
            CommandResult::TypeOf { ty } => Self {
                text: ty.to_string(),
                kind: ResponseKind::TypeQuery {
                    ty: ty.to_string(),
                },
            },
            CommandResult::Cleared => Self {
                text: "Environment cleared.".to_string(),
                kind: ResponseKind::Empty,
            },
            CommandResult::Help(text) => Self {
                text,
                kind: ResponseKind::Empty,
            },
            CommandResult::Quit => Self {
                text: "Bye!".to_string(),
                kind: ResponseKind::Empty,
            },
            CommandResult::Unknown { cmd } => Self::error(cmd),
        }
    }

    /// Create an error response.
    pub fn error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            text: format!("Error: {message}"),
            kind: ResponseKind::Error { message },
        }
    }

    /// Create a value response.
    fn value(name: &str, ty: Type, value: Value) -> Self {
        let ty_str = ty.to_string();
        let value_str = value.to_string();
        Self {
            text: format!("val {name} : {ty_str} = {value_str}"),
            kind: ResponseKind::Value {
                name: name.to_string(),
                ty: ty_str,
                value: value_str,
            },
        }
    }

    /// Create a function response.
    fn function(name: &str, ty: Type) -> Self {
        let ty_str = ty.to_string();
        Self {
            text: format!("fun {name} : {ty_str}"),
            kind: ResponseKind::Function {
                name: name.to_string(),
                ty: ty_str,
            },
        }
    }

    /// Create a type definition response.
    fn type_def(name: &str, kind: TypeDefKind) -> Self {
        let keyword = match kind {
            TypeDefKind::Type => "type",
            TypeDefKind::Datatype => "datatype",
        };
        Self {
            text: format!("{keyword} {name}"),
            kind: ResponseKind::TypeDef {
                name: name.to_string(),
                kind,
            },
        }
    }

    /// Create an empty response.
    fn empty() -> Self {
        Self {
            text: String::new(),
            kind: ResponseKind::Empty,
        }
    }

    // ========== Accessor Methods ==========

    /// Get the formatted text output.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Check if this is an error response.
    pub fn is_error(&self) -> bool {
        matches!(self.kind, ResponseKind::Error { .. })
    }

    /// Get the error message, if this is an error.
    pub fn error_message(&self) -> Option<&str> {
        match &self.kind {
            ResponseKind::Error { message } => Some(message),
            _ => None,
        }
    }

    /// Get the binding name (e.g., "it", "x", "double").
    pub fn binding_name(&self) -> Option<&str> {
        match &self.kind {
            ResponseKind::Value { name, .. } => Some(name),
            ResponseKind::Function { name, .. } => Some(name),
            ResponseKind::TypeDef { name, .. } => Some(name),
            _ => None,
        }
    }

    /// Get the type as a string.
    pub fn type_str(&self) -> Option<&str> {
        match &self.kind {
            ResponseKind::Value { ty, .. } => Some(ty),
            ResponseKind::Function { ty, .. } => Some(ty),
            ResponseKind::TypeQuery { ty } => Some(ty),
            _ => None,
        }
    }

    /// Get the value as a string.
    pub fn value_str(&self) -> Option<&str> {
        match &self.kind {
            ResponseKind::Value { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Check if this is a function definition.
    pub fn is_function(&self) -> bool {
        matches!(self.kind, ResponseKind::Function { .. })
    }

    /// Check if this is a type definition.
    pub fn is_type_def(&self) -> bool {
        matches!(self.kind, ResponseKind::TypeDef { .. })
    }

    /// Check if this is empty (no output).
    pub fn is_empty(&self) -> bool {
        matches!(self.kind, ResponseKind::Empty)
    }
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_response() {
        let response = Response::error("unbound variable: x");
        assert!(response.is_error());
        assert_eq!(response.error_message(), Some("unbound variable: x"));
        assert_eq!(response.text(), "Error: unbound variable: x");
    }

    #[test]
    fn test_value_response() {
        let response = Response::value("it", Type::int(), Value::Int(42));
        assert!(!response.is_error());
        assert_eq!(response.binding_name(), Some("it"));
        assert_eq!(response.type_str(), Some("int"));
        assert_eq!(response.value_str(), Some("42"));
        assert_eq!(response.text(), "val it : int = 42");
    }

    #[test]
    fn test_function_response() {
        let ty = Type::arrow(Type::int(), Type::int());
        let response = Response::function("double", ty);
        assert!(response.is_function());
        assert_eq!(response.binding_name(), Some("double"));
        assert_eq!(response.type_str(), Some("int -> int"));
        assert_eq!(response.value_str(), None);
    }

    #[test]
    fn test_empty_response() {
        let response = Response::empty();
        assert!(response.is_empty());
        assert!(!response.is_error());
        assert_eq!(response.text(), "");
    }
}
