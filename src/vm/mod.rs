//! Virtual Machine for Neap
//!
//! A tree-walking interpreter that executes the IR directly.
//! The ANF form of the IR makes execution straightforward since
//! all intermediate values are explicitly named.

mod env;
mod error;
mod eval;
mod pattern;
mod primitive;
mod value;

pub use env::Env;
pub use error::RuntimeError;
pub use eval::VM;
pub use value::Value;
