//! Type error definitions for the Neap type system

use super::core::{Type, TypeVar};
use crate::syntax::Span;
use thiserror::Error;

/// Errors that can occur during type checking.
#[derive(Debug, Clone, Error, PartialEq)]
pub enum TypeError {
    /// Type mismatch: expected one type, got another
    #[error("type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: Type, actual: Type },

    /// Infinite type detected (occurs check failure)
    #[error("infinite type: {var} occurs in {ty}")]
    InfiniteType { var: TypeVar, ty: Type },

    /// Unbound variable
    #[error("unbound variable: {name}")]
    UnboundVariable { name: String, span: Span },

    /// Unbound type variable
    #[error("unbound type variable: {name}")]
    UnboundTypeVariable { name: String, span: Span },

    /// Unbound constructor
    #[error("unbound constructor: {name}")]
    UnboundConstructor { name: String, span: Span },

    /// Unknown type constructor
    #[error("unknown type: {name}")]
    UnknownType { name: String, span: Span },

    /// Wrong number of type arguments
    #[error("type {name} expects {expected} argument(s), got {actual}")]
    WrongTypeArity {
        name: String,
        expected: usize,
        actual: usize,
        span: Span,
    },

    /// Pattern type mismatch
    #[error("pattern has type {pattern_ty}, but is matched against {scrutinee_ty}")]
    PatternTypeMismatch {
        pattern_ty: Type,
        scrutinee_ty: Type,
        span: Span,
    },

    /// Non-exhaustive pattern match
    #[error("non-exhaustive pattern match")]
    NonExhaustiveMatch { span: Span },

    /// Duplicate field in record
    #[error("duplicate field '{name}' in record")]
    DuplicateField { name: String, span: Span },

    /// Missing field in record
    #[error("missing field '{name}' in record")]
    MissingField {
        name: String,
        record_ty: Type,
        span: Span,
    },

    /// Extra field in record
    #[error("extra field '{name}' in record")]
    ExtraField { name: String, span: Span },

    /// Not a function type
    #[error("expected function type, got {ty}")]
    NotAFunction { ty: Type, span: Span },

    /// Not a record type
    #[error("expected record type, got {ty}")]
    NotARecord { ty: Type, span: Span },

    /// Recursive binding without rec keyword
    #[error("recursive binding requires 'rec' keyword")]
    RecursiveBindingWithoutRec { name: String, span: Span },

    /// Duplicate definition
    #[error("duplicate definition: {name}")]
    DuplicateDefinition { name: String, span: Span },

    /// Type annotation mismatch
    #[error("type annotation {annotation} doesn't match inferred type {inferred}")]
    AnnotationMismatch {
        annotation: Type,
        inferred: Type,
        span: Span,
    },

    /// Invalid pattern for this context
    #[error("invalid pattern")]
    InvalidPattern { span: Span },
}

impl TypeError {
    /// Get the span associated with this error, if any.
    #[must_use]
    pub fn span(&self) -> Option<Span> {
        match self {
            Self::TypeMismatch { .. } | Self::InfiniteType { .. } => None,
            Self::UnboundVariable { span, .. }
            | Self::UnboundTypeVariable { span, .. }
            | Self::UnboundConstructor { span, .. }
            | Self::UnknownType { span, .. }
            | Self::WrongTypeArity { span, .. }
            | Self::PatternTypeMismatch { span, .. }
            | Self::NonExhaustiveMatch { span, .. }
            | Self::DuplicateField { span, .. }
            | Self::MissingField { span, .. }
            | Self::ExtraField { span, .. }
            | Self::NotAFunction { span, .. }
            | Self::NotARecord { span, .. }
            | Self::RecursiveBindingWithoutRec { span, .. }
            | Self::DuplicateDefinition { span, .. }
            | Self::AnnotationMismatch { span, .. }
            | Self::InvalidPattern { span, .. } => Some(*span),
        }
    }
}
