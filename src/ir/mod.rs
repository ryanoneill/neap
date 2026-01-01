//! Intermediate Representation for Neap
//!
//! The IR is a typed, simplified representation of Neap programs.
//! It uses A-Normal Form (ANF) where all intermediate computations
//! are explicitly named, making code generation and optimization easier.

mod core;
mod lower;
mod optimize;

pub use core::{
    Binding, IRCommandPart, IRDecl, IRExpr, IRLiteral, IRPattern, IRProgram, Primitive, VarId,
};
pub use lower::{Lower, LowerError};
pub use optimize::Optimizer;
