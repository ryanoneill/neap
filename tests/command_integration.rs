//! Integration tests for shell command execution

use neap::ir::{Lower, Optimizer};
use neap::syntax::Parser;
use neap::types::TypeChecker;
use neap::vm::VM;
use std::io::Cursor;

/// Helper to run a Neap program and capture output
fn run_program(source: &str) -> Result<String, String> {
    // Parse
    let mut parser = Parser::new(source).map_err(|e| format!("parse error: {}", e))?;
    let program = parser.parse_program().map_err(|e| format!("parse error: {}", e))?;

    // Type check
    let mut checker = TypeChecker::new();
    checker
        .check_program(&program)
        .map_err(|errors| format!("type error: {}", errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")))?;

    // Lower to IR
    let mut lowerer = Lower::new();
    let ir_program = lowerer.lower_program(&program).map_err(|e| format!("lower error: {}", e))?;

    // Optimize
    let optimizer = Optimizer::new();
    let optimized = optimizer.optimize(ir_program);

    // Run with captured output
    let output = Vec::new();
    let input = Cursor::new(Vec::new());
    let mut vm = VM::new(output, input);

    vm.eval_program(&optimized).map_err(|e| format!("runtime error: {}", e))?;

    // Get captured output
    let output = vm.into_writer();
    String::from_utf8(output).map_err(|e| e.to_string())
}

#[test]
fn test_simple_command() {
    // Just run a command - verify we can capture and store result
    let source = r#"
        let result = `echo hello`
        fun getOut x = result.stdout
        let out = getOut 1
        let p = print out
    "#;

    let output = run_program(source).expect("Program should run");
    assert!(output.contains("hello"), "Output was: {}", output);
}

#[test]
fn test_command_stdout_field() {
    // Access the stdout field of a command result
    let source = r#"
        let result = `echo hello`
        let out = result.stdout
        let p = print out
    "#;

    let output = run_program(source).expect("Program should run");
    assert!(output.contains("hello"), "Output was: {}", output);
}

#[test]
fn test_command_with_interpolation() {
    let source = r#"
        let name = "world"
        let result = `echo {name}`
        let out = result.stdout
        let p = print out
    "#;

    let output = run_program(source).expect("Program should run");
    assert!(output.contains("world"), "Output was: {}", output);
}

#[test]
fn test_command_exit_code() {
    let source = r#"
        let result = `echo test`
        let code = result.exitCode
        let msg = if code == 0 then "success" else "failure"
        let p = print msg
    "#;

    let output = run_program(source).expect("Program should run");
    assert!(output.contains("success"), "Output was: {}", output);
}

// ========== Command Pipeline Tests ==========

#[test]
fn test_command_pipeline() {
    // Simple pipeline: echo | grep
    let source = r#"
        let result = `echo hello world` |> `grep world`
        let out = result.stdout
        let p = print out
    "#;

    let output = run_program(source).expect("Program should run");
    assert!(output.contains("hello world"), "Output was: {}", output);
}

#[test]
fn test_command_pipeline_filtering() {
    // Pipeline that filters content
    let source = r#"
        let result = `echo hello` |> `cat`
        let out = result.stdout
        let p = print out
    "#;

    let output = run_program(source).expect("Program should run");
    assert!(output.contains("hello"), "Output was: {}", output);
}

#[test]
fn test_command_pipeline_exit_code() {
    // The exit code should be from the last command in the pipeline
    let source = r#"
        let result = `echo test` |> `cat`
        let code = result.exitCode
        let p = print (if code == 0 then "success" else "failure")
    "#;

    let output = run_program(source).expect("Program should run");
    assert!(output.contains("success"), "Output was: {}", output);
}

// ========== Type Class Tests ==========

#[test]
fn test_show_int() {
    let source = r#"
        let s = show 42
        let p = print s
    "#;

    let output = run_program(source).expect("Program should run");
    assert!(output.contains("42"), "Output was: {}", output);
}

#[test]
fn test_show_float() {
    let source = r#"
        let s = show 3.14
        let p = print s
    "#;

    let output = run_program(source).expect("Program should run");
    assert!(output.contains("3.14"), "Output was: {}", output);
}

#[test]
fn test_show_bool() {
    let source = r#"
        let s = show true
        let p = print s
    "#;

    let output = run_program(source).expect("Program should run");
    assert!(output.contains("true"), "Output was: {}", output);
}

#[test]
fn test_show_string() {
    let source = r#"
        let s = show "hello"
        let p = print s
    "#;

    let output = run_program(source).expect("Program should run");
    assert!(output.contains("hello"), "Output was: {}", output);
}

#[test]
fn test_show_in_expression() {
    // Use show in a larger expression
    let source = r#"
        let s = "The answer is: " ++ show 42
        let p = print s
    "#;

    let output = run_program(source).expect("Program should run");
    assert!(output.contains("The answer is: 42"), "Output was: {}", output);
}
