//! Interactive REPL for Neap
//!
//! This module provides:
//! - `ReplEngine`: Core evaluation logic separated from UI
//! - `ReplTestHarness`: Async API for programmatic testing
//! - `Response`: Structured response type for inspecting results
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

mod engine;
mod harness;
mod response;

#[cfg(test)]
mod tests;

pub use engine::{CommandResult, EvalResult, ReplEngine};
pub use harness::ReplTestHarness;
pub use response::{Response, ResponseKind, TypeDefKind};

use std::io::{self, BufReader, Stdin, Stdout};

use thiserror::Error;

/// Errors that can occur in the REPL.
#[derive(Debug, Error)]
pub enum ReplError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

/// Run the interactive REPL.
///
/// This is a placeholder that will be replaced with the envision TUI.
/// For now, it provides a simple stdin/stdout based REPL.
pub fn run() -> Result<(), ReplError> {
    use std::io::{BufRead, Write};

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut engine: ReplEngine<Stdout, BufReader<Stdin>> =
        ReplEngine::new(stdout, BufReader::new(stdin));

    println!("Neap REPL v0.1.0");
    println!("Type :help for help, :quit to exit\n");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("neap> ");
        stdout.flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            // EOF
            println!("\nBye!");
            break;
        }

        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        // Handle commands
        if line.starts_with(':') {
            let result = engine.eval_command(line);
            match result {
                CommandResult::Quit => {
                    println!("Bye!");
                    break;
                }
                CommandResult::Help(text) => println!("{text}"),
                CommandResult::Cleared => println!("Environment cleared."),
                CommandResult::TypeOf { ty } => println!("{ty}"),
                CommandResult::Unknown { cmd } => eprintln!("{cmd}"),
            }
            continue;
        }

        // Evaluate expression or declaration
        match engine.eval(line) {
            Ok(Some(result)) => {
                let response = Response::from_eval_result(result);
                if !response.is_empty() {
                    println!("{}", response.text());
                }
            }
            Ok(None) => {
                eprintln!("Error: incomplete input");
            }
            Err(e) => {
                eprintln!("Error: {e}");
            }
        }
    }

    Ok(())
}
