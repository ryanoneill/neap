//! Test harness for the REPL.
//!
//! This module provides a convenient async API for testing the REPL.
//!
//! # Example
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

use std::io::Cursor;

use super::engine::ReplEngine;
use super::response::Response;

/// A test harness for the REPL.
///
/// This provides a convenient async API for testing REPL functionality.
/// The harness uses in-memory buffers for IO, making it suitable for
/// automated testing.
pub struct ReplTestHarness {
    /// The underlying REPL engine with captured IO
    engine: ReplEngine<Vec<u8>, Cursor<Vec<u8>>>,
}

impl ReplTestHarness {
    /// Create a new test harness.
    pub fn new() -> Self {
        Self {
            engine: ReplEngine::new(Vec::new(), Cursor::new(Vec::new())),
        }
    }

    /// Evaluate an input line and return the response.
    ///
    /// This handles both REPL commands (starting with `:`) and
    /// regular expressions/declarations.
    ///
    /// The async API is for future expansion (async evaluation, timeouts).
    /// Currently, evaluation is synchronous internally.
    pub async fn input(&mut self, line: &str) -> Response {
        // Handle REPL commands
        if line.starts_with(':') {
            let result = self.engine.eval_command(line);
            return Response::from_command_result(result);
        }

        // Handle expressions and declarations
        match self.engine.eval(line) {
            Ok(Some(result)) => Response::from_eval_result(result),
            Ok(None) => Response::error("incomplete input"),
            Err(e) => Response::error(e),
        }
    }

    /// Clear all definitions and reset the environment.
    pub async fn clear(&mut self) {
        self.engine.clear();
    }

    /// Get the captured stdout output.
    ///
    /// This returns any output from `print` statements.
    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(self.engine.stdout()).to_string()
    }

    /// Clear the captured stdout buffer.
    pub fn clear_stdout(&mut self) {
        self.engine.stdout_mut().clear();
    }

    /// Provide input for stdin (for `readLine` calls).
    pub fn provide_stdin(&mut self, input: &str) {
        // Create a new engine with the input
        // This is a bit awkward, but works for testing
        let current_stdout = std::mem::take(self.engine.stdout_mut());
        self.engine = ReplEngine::new(current_stdout, Cursor::new(input.as_bytes().to_vec()));
    }
}

impl Default for ReplTestHarness {
    fn default() -> Self {
        Self::new()
    }
}
