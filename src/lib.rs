//! Neap: A statically-typed, ML-inspired language for shell scripting
//!
//! Neap emphasizes type-safe stream processing with generic `Stream<T>` and codecs,
//! functional programming with ADTs, pattern matching, and type inference,
//! and shell integration via bytecode VM with shell FFI.

pub mod codegen;
pub mod ir;
pub mod repl;
pub mod syntax;
pub mod types;
pub mod vm;
