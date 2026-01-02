//! Interactive REPL for Neap
//!
//! Provides a Read-Eval-Print Loop with:
//! - Expression evaluation
//! - Declaration persistence
//! - Multi-line input
//! - Line editing and history

use std::io::{self, BufReader, Stdin, Stdout};

use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use thiserror::Error;

use crate::ir::{Lower, Optimizer};
use crate::syntax::{Decl, Parser, Spanned};
use crate::types::{Type, TypeChecker};
use crate::vm::{Value, VM};

/// Errors that can occur in the REPL.
#[derive(Debug, Error)]
pub enum ReplError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Readline error: {0}")]
    Readline(#[from] ReadlineError),
}

/// Result of evaluating a REPL input.
pub enum EvalResult {
    /// An expression was evaluated
    Expression { ty: Type, value: Value },
    /// A value declaration was processed
    ValDecl { name: String, ty: Type, value: Value },
    /// A function declaration was processed
    FunDecl { name: String, ty: Type },
    /// A type declaration was processed
    TypeDecl { name: String },
    /// A datatype declaration was processed
    DatatypeDecl { name: String },
    /// Empty input (nothing to do)
    Empty,
}

/// The interactive REPL.
pub struct Repl {
    /// Type checker with persistent environment
    checker: TypeChecker,
    /// Lowerer for converting AST to IR
    lowerer: Lower,
    /// VM with persistent globals
    vm: VM<Stdout, BufReader<Stdin>>,
    /// Line editor
    editor: DefaultEditor,
    /// Accumulated input for multi-line
    buffer: String,
}

impl Repl {
    /// Create a new REPL instance.
    pub fn new() -> Result<Self, ReplError> {
        let editor = DefaultEditor::new()?;

        Ok(Self {
            checker: TypeChecker::new(),
            lowerer: Lower::new(),
            vm: Self::create_vm(),
            editor,
            buffer: String::new(),
        })
    }

    /// Create a new VM with standard IO.
    fn create_vm() -> VM<Stdout, BufReader<Stdin>> {
        VM::new(io::stdout(), BufReader::new(io::stdin()))
    }

    /// Run the REPL main loop.
    pub fn run(&mut self) -> Result<(), ReplError> {
        println!("Neap REPL v0.1.0");
        println!("Type :help for help, :quit to exit\n");

        loop {
            let prompt = if self.buffer.is_empty() {
                "neap> "
            } else {
                "....> "
            };

            match self.editor.readline(prompt) {
                Ok(line) => {
                    // Add to history
                    let _ = self.editor.add_history_entry(&line);

                    // Check for REPL commands
                    if self.buffer.is_empty() && line.starts_with(':') {
                        if self.handle_command(&line) {
                            break;
                        }
                        continue;
                    }

                    // Accumulate input
                    if !self.buffer.is_empty() {
                        self.buffer.push('\n');
                    }
                    self.buffer.push_str(&line);

                    // Try to evaluate
                    match self.try_eval() {
                        Ok(Some(result)) => {
                            self.print_result(&result);
                            self.buffer.clear();
                        }
                        Ok(None) => {
                            // Incomplete input, continue accumulating
                        }
                        Err(e) => {
                            eprintln!("Error: {e}");
                            self.buffer.clear();
                        }
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    // Ctrl-C: clear current input
                    if !self.buffer.is_empty() {
                        self.buffer.clear();
                        println!("(interrupted)");
                    }
                }
                Err(ReadlineError::Eof) => {
                    // Ctrl-D: exit
                    println!("\nBye!");
                    break;
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }

        Ok(())
    }

    /// Handle a REPL command (lines starting with :).
    /// Returns true if the REPL should exit.
    fn handle_command(&mut self, line: &str) -> bool {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let cmd = parts.first().map(|s| *s).unwrap_or("");

        match cmd {
            ":quit" | ":q" => {
                println!("Bye!");
                true
            }
            ":help" | ":h" => {
                self.print_help();
                false
            }
            ":type" | ":t" => {
                if parts.len() < 2 {
                    eprintln!("Usage: :type <expression>");
                } else {
                    let expr_str = parts[1..].join(" ");
                    self.show_type(&expr_str);
                }
                false
            }
            ":clear" => {
                // Reset state
                self.checker = TypeChecker::new();
                self.lowerer = Lower::new();
                self.vm = Self::create_vm();
                println!("Environment cleared.");
                false
            }
            _ => {
                eprintln!("Unknown command: {cmd}");
                eprintln!("Type :help for available commands.");
                false
            }
        }
    }

    /// Print help message.
    fn print_help(&self) {
        println!("Neap REPL Commands:");
        println!("  :quit, :q     Exit the REPL");
        println!("  :help, :h     Show this help");
        println!("  :type, :t     Show type of expression without evaluating");
        println!("  :clear        Clear all definitions");
        println!();
        println!("Enter expressions to evaluate, or declarations to define:");
        println!("  1 + 2              Expression (prints result)");
        println!("  val x = 42         Value declaration");
        println!("  fun double n = n * 2   Function declaration");
        println!();
        println!("Keyboard shortcuts:");
        println!("  Ctrl-C        Cancel current input");
        println!("  Ctrl-D        Exit REPL");
        println!("  Up/Down       Navigate history");
    }

    /// Show the type of an expression without evaluating.
    fn show_type(&mut self, expr_str: &str) {
        let mut parser = match Parser::new(expr_str) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Parse error: {e}");
                return;
            }
        };

        let expr = match parser.parse_expr() {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Parse error: {e}");
                return;
            }
        };

        match self.checker.infer_expr(&expr) {
            Ok(ty) => println!("{ty}"),
            Err(e) => eprintln!("Type error: {e}"),
        }
    }

    /// Try to evaluate the current buffer.
    /// Returns None if input is incomplete (needs more lines).
    fn try_eval(&mut self) -> Result<Option<EvalResult>, String> {
        if self.buffer.trim().is_empty() {
            return Ok(Some(EvalResult::Empty));
        }

        // Try parsing as a declaration first
        if let Some(result) = self.try_eval_decl()? {
            return Ok(Some(result));
        }

        // Try parsing as an expression
        self.try_eval_expr()
    }

    /// Try to evaluate as a declaration.
    fn try_eval_decl(&mut self) -> Result<Option<EvalResult>, String> {
        let mut parser = Parser::new(&self.buffer).map_err(|e| e.to_string())?;

        // Try to parse a single declaration
        let decl = match parser.parse_decl() {
            Ok(d) => d,
            Err(_) => return Ok(None), // Not a declaration, try expression
        };

        // Type check the declaration
        if let Err(e) = self.checker.check_decl(&decl) {
            return Err(e.to_string());
        }

        // Lower and evaluate
        let result = self.eval_decl(&decl)?;
        Ok(Some(result))
    }

    /// Evaluate a declaration and return the result.
    fn eval_decl(&mut self, decl: &Spanned<Decl>) -> Result<EvalResult, String> {
        match &decl.value {
            Decl::Val(val_decl) => {
                // Get the name from the pattern
                let name = self.get_pattern_name(&val_decl.pattern)?;

                // Get the type (which was added to the env during check_decl)
                let ty = self.checker.lookup_type(&name)
                    .ok_or_else(|| format!("Type not found for {name}"))?;

                // Lower the expression with the known type
                let ir_expr = self.lowerer.lower_expr_with_type(&val_decl.expr, &ty)
                    .map_err(|e| e.to_string())?;

                // Optimize
                let optimizer = Optimizer::new();
                let optimized = optimizer.optimize_expr(ir_expr);

                // Evaluate
                let value = self.vm.eval_expr_standalone(&optimized)
                    .map_err(|e| e.to_string())?;

                // Store in VM globals
                self.vm.define_global(&name, value.clone());

                Ok(EvalResult::ValDecl { name, ty, value })
            }
            Decl::Fun(fun_decl) => {
                let name = fun_decl.name.value.clone();

                // Get the type
                let ty = self.checker.lookup_type(&name)
                    .ok_or_else(|| format!("Type not found for {name}"))?;

                // Lower the function
                let ir_decl = self.lowerer.lower_fun_standalone(fun_decl)
                    .map_err(|e| e.to_string())?;

                // Evaluate (define the function in VM)
                self.vm.eval_decl_standalone(&ir_decl)
                    .map_err(|e| e.to_string())?;

                Ok(EvalResult::FunDecl { name, ty })
            }
            Decl::Type(type_decl) => {
                let name = type_decl.name.value.clone();
                Ok(EvalResult::TypeDecl { name })
            }
            Decl::Datatype(dt_decl) => {
                let name = dt_decl.name.value.clone();
                Ok(EvalResult::DatatypeDecl { name })
            }
            Decl::Trait(_) | Decl::Impl(_) => {
                // For now, just acknowledge
                Ok(EvalResult::Empty)
            }
        }
    }

    /// Try to evaluate as an expression.
    fn try_eval_expr(&mut self) -> Result<Option<EvalResult>, String> {
        let mut parser = Parser::new(&self.buffer).map_err(|e| e.to_string())?;

        let expr = match parser.parse_expr() {
            Ok(e) => e,
            Err(e) => {
                // Check if it looks like incomplete input
                if self.is_incomplete_input(&e.to_string()) {
                    return Ok(None);
                }
                return Err(e.to_string());
            }
        };

        // Type check
        let ty = self.checker.infer_expr(&expr).map_err(|e| e.to_string())?;

        // Lower with the inferred type
        let ir_expr = self.lowerer.lower_expr_with_type(&expr, &ty)
            .map_err(|e| e.to_string())?;

        // Optimize
        let optimizer = Optimizer::new();
        let optimized = optimizer.optimize_expr(ir_expr);

        // Evaluate
        let value = self.vm.eval_expr_standalone(&optimized)
            .map_err(|e| e.to_string())?;

        // Bind to `it`
        self.vm.define_global("it", value.clone());

        Ok(Some(EvalResult::Expression { ty, value }))
    }

    /// Check if an error message suggests incomplete input.
    fn is_incomplete_input(&self, error: &str) -> bool {
        // Common patterns for incomplete input
        error.contains("unexpected end of input")
            || error.contains("expected")
            || (self.buffer.contains("let") && !self.buffer.contains("in"))
    }

    /// Get the name from a pattern (for simple variable patterns).
    fn get_pattern_name(&self, pattern: &Spanned<crate::syntax::Pattern>) -> Result<String, String> {
        use crate::syntax::Pattern;
        match &pattern.value {
            Pattern::Var(name) => Ok(name.clone()),
            _ => Err("Complex patterns not supported in REPL declarations".to_string()),
        }
    }

    /// Print the result of an evaluation.
    fn print_result(&mut self, result: &EvalResult) {
        match result {
            EvalResult::Expression { ty, value } => {
                println!("val it : {ty} = {value}");
            }
            EvalResult::ValDecl { name, ty, value } => {
                println!("val {name} : {ty} = {value}");
            }
            EvalResult::FunDecl { name, ty } => {
                println!("fun {name} : {ty}");
            }
            EvalResult::TypeDecl { name } => {
                println!("type {name}");
            }
            EvalResult::DatatypeDecl { name } => {
                println!("datatype {name}");
            }
            EvalResult::Empty => {}
        }
    }
}

impl Default for Repl {
    fn default() -> Self {
        Self::new().expect("Failed to create REPL")
    }
}

/// Run the REPL.
pub fn run() -> Result<(), ReplError> {
    let mut repl = Repl::new()?;
    repl.run()
}
