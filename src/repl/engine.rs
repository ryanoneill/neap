//! Core REPL evaluation engine.
//!
//! This module contains the evaluation logic separated from any UI concerns.
//! It can be used by both the interactive TUI and the test harness.

use std::io::{BufRead, Write};

use crate::ir::{Lower, Optimizer};
use crate::syntax::{Decl, Parser, Pattern, Spanned};
use crate::types::{Type, TypeChecker, TypeScheme};
use crate::vm::{Value, VM};

/// Result of evaluating a REPL input.
#[derive(Debug, Clone)]
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

/// Result of a REPL command.
#[derive(Debug, Clone)]
pub enum CommandResult {
    /// :type command result
    TypeOf { ty: Type },
    /// :clear command executed
    Cleared,
    /// :help command (text to display)
    Help(String),
    /// :quit command
    Quit,
    /// Unknown command
    Unknown { cmd: String },
}

/// The core REPL evaluation engine.
///
/// This struct holds the persistent state needed for evaluation:
/// - Type checker with environment
/// - Lowerer for IR generation
/// - VM for execution
///
/// The engine is generic over the VM's writer and reader types,
/// allowing it to be used with real IO or captured buffers for testing.
pub struct ReplEngine<W: Write, R: BufRead> {
    /// Type checker with persistent environment
    checker: TypeChecker,
    /// Lowerer for converting AST to IR
    lowerer: Lower,
    /// VM with persistent globals
    vm: VM<W, R>,
}

impl<W: Write, R: BufRead> ReplEngine<W, R> {
    /// Create a new REPL engine with the given writer and reader.
    pub fn new(writer: W, reader: R) -> Self {
        Self {
            checker: TypeChecker::new(),
            lowerer: Lower::new(),
            vm: VM::new(writer, reader),
        }
    }

    /// Evaluate an input string.
    ///
    /// This handles both expressions and declarations:
    /// - Declarations are parsed, type-checked, lowered, and evaluated
    /// - Expressions are parsed, type-checked, lowered, optimized, and evaluated
    ///
    /// Returns `Ok(None)` if the input is incomplete (needs more lines).
    pub fn eval(&mut self, input: &str) -> Result<Option<EvalResult>, String> {
        if input.trim().is_empty() {
            return Ok(Some(EvalResult::Empty));
        }

        // Try parsing as a declaration first
        if let Some(result) = self.try_eval_decl(input)? {
            return Ok(Some(result));
        }

        // Try parsing as an expression
        self.try_eval_expr(input)
    }

    /// Execute a REPL command (lines starting with :).
    pub fn eval_command(&mut self, cmd: &str) -> CommandResult {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let command = parts.first().copied().unwrap_or("");

        match command {
            ":quit" | ":q" => CommandResult::Quit,
            ":help" | ":h" => CommandResult::Help(Self::help_text()),
            ":type" | ":t" => {
                if parts.len() < 2 {
                    CommandResult::Unknown {
                        cmd: "Usage: :type <expression>".to_string(),
                    }
                } else {
                    let expr_str = parts[1..].join(" ");
                    match self.type_of(&expr_str) {
                        Ok(ty) => CommandResult::TypeOf { ty },
                        Err(e) => CommandResult::Unknown { cmd: e },
                    }
                }
            }
            ":clear" => {
                self.clear();
                CommandResult::Cleared
            }
            _ => CommandResult::Unknown {
                cmd: format!("Unknown command: {command}"),
            },
        }
    }

    /// Get the type of an expression without evaluating it.
    pub fn type_of(&mut self, expr_str: &str) -> Result<Type, String> {
        let mut parser = Parser::new(expr_str).map_err(|e| format!("Parse error: {e}"))?;

        let expr = parser
            .parse_expr()
            .map_err(|e| format!("Parse error: {e}"))?;

        self.checker
            .infer_expr(&expr)
            .map_err(|e| format!("Type error: {e}"))
    }

    /// Clear all definitions and reset the environment.
    pub fn clear(&mut self) {
        self.checker = TypeChecker::new();
        self.lowerer = Lower::new();
        // We can't easily reset the VM without knowing how to create a new writer/reader,
        // so we just clear its globals
        self.vm.clear_globals();
    }

    /// Get a reference to the VM's stdout (for capturing output).
    pub fn stdout(&self) -> &W {
        self.vm.stdout()
    }

    /// Get a mutable reference to the VM's stdout.
    pub fn stdout_mut(&mut self) -> &mut W {
        self.vm.stdout_mut()
    }

    /// Get the help text.
    fn help_text() -> String {
        let mut help = String::new();
        help.push_str("Neap REPL Commands:\n");
        help.push_str("  :quit, :q     Exit the REPL\n");
        help.push_str("  :help, :h     Show this help\n");
        help.push_str("  :type, :t     Show type of expression without evaluating\n");
        help.push_str("  :clear        Clear all definitions\n");
        help.push('\n');
        help.push_str("Enter expressions to evaluate, or declarations to define:\n");
        help.push_str("  1 + 2              Expression (prints result)\n");
        help.push_str("  let x = 42         Value declaration\n");
        help.push_str("  fun double n = n * 2   Function declaration\n");
        help
    }

    /// Try to evaluate as a declaration.
    fn try_eval_decl(&mut self, input: &str) -> Result<Option<EvalResult>, String> {
        let mut parser = Parser::new(input).map_err(|e| e.to_string())?;

        // Try to parse a single declaration
        let decl = match parser.parse_decl() {
            Ok(d) => d,
            Err(_) => return Ok(None), // Not a declaration, try expression
        };

        // Check that we consumed all input - if there's leftover input,
        // this might be a local let expression (e.g., `let x = 1 in x + 1`)
        // rather than a top-level declaration
        if !parser.is_at_end() {
            return Ok(None); // Try as expression instead
        }

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
                let ty = self
                    .checker
                    .lookup_type(&name)
                    .ok_or_else(|| format!("Type not found for {name}"))?;

                // Lower the expression with the known type
                let ir_expr = self
                    .lowerer
                    .lower_expr_with_type(&val_decl.expr, &ty)
                    .map_err(|e| e.to_string())?;

                // Optimize
                let optimizer = Optimizer::new();
                let optimized = optimizer.optimize_expr(ir_expr);

                // Evaluate
                let value = self
                    .vm
                    .eval_expr_standalone(&optimized)
                    .map_err(|e| e.to_string())?;

                // Store in VM globals
                self.vm.define_global(&name, value.clone());

                // Register with lowerer so future expressions can reference it
                self.lowerer.register_global(&name, ty.clone());

                Ok(EvalResult::ValDecl { name, ty, value })
            }
            Decl::Fun(fun_decl) => {
                let name = fun_decl.name.value.clone();

                // Get the type
                let ty = self
                    .checker
                    .lookup_type(&name)
                    .ok_or_else(|| format!("Type not found for {name}"))?;

                // Register with lowerer first so recursive calls work
                self.lowerer.register_global(&name, ty.clone());

                // Lower the function
                let ir_decl = self
                    .lowerer
                    .lower_fun_standalone(fun_decl)
                    .map_err(|e| e.to_string())?;

                // Evaluate (define the function in VM)
                self.vm
                    .eval_decl_standalone(&ir_decl)
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
    fn try_eval_expr(&mut self, input: &str) -> Result<Option<EvalResult>, String> {
        let mut parser = Parser::new(input).map_err(|e| e.to_string())?;

        let expr = match parser.parse_expr() {
            Ok(e) => e,
            Err(e) => {
                // Check if it looks like incomplete input
                if self.is_incomplete_input(input, &e.to_string()) {
                    return Ok(None);
                }
                return Err(e.to_string());
            }
        };

        // Type check
        let ty = self
            .checker
            .infer_expr(&expr)
            .map_err(|e| e.to_string())?;

        // Lower with the inferred type
        let ir_expr = self
            .lowerer
            .lower_expr_with_type(&expr, &ty)
            .map_err(|e| e.to_string())?;

        // Optimize
        let optimizer = Optimizer::new();
        let optimized = optimizer.optimize_expr(ir_expr);

        // Evaluate
        let value = self
            .vm
            .eval_expr_standalone(&optimized)
            .map_err(|e| e.to_string())?;

        // Bind to `it` - must register with all three: type checker, lowerer, and VM
        self.checker.register_global("it", TypeScheme::mono(ty.clone()));
        self.lowerer.register_global("it", ty.clone());
        self.vm.define_global("it", value.clone());

        Ok(Some(EvalResult::Expression { ty, value }))
    }

    /// Check if an error message suggests incomplete input.
    fn is_incomplete_input(&self, input: &str, error: &str) -> bool {
        // Common patterns for incomplete input
        error.contains("unexpected end of input")
            || error.contains("expected")
            || (input.contains("let") && !input.contains("in"))
    }

    /// Get the name from a pattern (for simple variable patterns).
    fn get_pattern_name(&self, pattern: &Spanned<Pattern>) -> Result<String, String> {
        match &pattern.value {
            Pattern::Var(name) => Ok(name.clone()),
            _ => Err("Complex patterns not supported in REPL declarations".to_string()),
        }
    }
}
