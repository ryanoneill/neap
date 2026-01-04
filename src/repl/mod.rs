//! Interactive REPL for Neap
//!
//! This module provides:
//! - `ReplEngine`: Core evaluation logic separated from UI
//! - `ReplTestHarness`: Async API for programmatic testing
//! - `Response`: Structured response type for inspecting results
//! - `NeapReplApp`: Envision-based TUI application
//!
//! # Testing Example
//!
//! ```rust,ignore
//! use neap::repl::ReplTestHarness;
//!
//! #[tokio::test]
//! async fn test_addition() {
//!     let mut repl = ReplTestHarness::new();
//!     let response = repl.input("1 + 2").await;
//!     assert_eq!(response.value_str(), Some("3"));
//! }
//! ```

mod app;
mod engine;
mod harness;
mod response;

#[cfg(test)]
mod tests;

pub use app::{run_tui, HistoryEntry, NeapReplApp, ReplMsg, ReplState};
pub use engine::{CommandResult, EvalResult, ReplEngine};
pub use harness::ReplTestHarness;
pub use response::{Response, ResponseKind, TypeDefKind};

use std::io;

use thiserror::Error;

/// Errors that can occur in the REPL.
#[derive(Debug, Error)]
pub enum ReplError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

/// Run the interactive REPL.
///
/// This runs the envision-based TUI REPL with full terminal UI.
pub fn run() -> Result<(), ReplError> {
    run_tui().map_err(|e| ReplError::Io(io::Error::new(io::ErrorKind::Other, e.to_string())))
}
