//! Type system module: Type representations, inference, and checking for Neap
//!
//! Implements Hindley-Milner type inference with:
//! - Polymorphic types with type variables
//! - Type constructors (int, string, list, etc.)
//! - Function types
//! - Record and tuple types
//! - Algebraic data types

mod core;
mod subst;
mod unify;
mod env;
mod infer;
mod error;

pub use core::{Type, TypeScheme, TypeVar};
pub use subst::Substitution;
pub use unify::unify;
pub use env::TypeEnv;
pub use infer::TypeChecker;
pub use error::TypeError;
