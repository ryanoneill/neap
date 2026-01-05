//! Code generation for Neap
//!
//! This module provides transpilation from Neap IR to target languages.
//! Currently supports zsh output.

mod runtime;
mod zsh;

pub use zsh::ZshCodegen;
