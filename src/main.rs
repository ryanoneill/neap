//! Neap CLI entry point

use neap::ir::{Lower, Optimizer};
use neap::syntax::{Lexer, Parser};
use neap::types::TypeChecker;
use neap::vm::VM;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: neap <command> [args...]");
        eprintln!("Commands:");
        eprintln!("  lex <file>     Tokenize a file and print tokens");
        eprintln!("  parse <file>   Parse a file and print the AST");
        eprintln!("  run <file>     Run a Neap program");
        eprintln!("  check <file>   Type check a Neap program");
        eprintln!("  repl           Start the interactive REPL");
        process::exit(1);
    }

    match args[1].as_str() {
        "lex" => {
            if args.len() < 3 {
                eprintln!("Usage: neap lex <file>");
                process::exit(1);
            }
            lex_file(&args[2]);
        }
        "parse" => {
            if args.len() < 3 {
                eprintln!("Usage: neap parse <file>");
                process::exit(1);
            }
            parse_file(&args[2]);
        }
        "check" => {
            if args.len() < 3 {
                eprintln!("Usage: neap check <file>");
                process::exit(1);
            }
            check_file(&args[2]);
        }
        "run" => {
            if args.len() < 3 {
                eprintln!("Usage: neap run <file>");
                process::exit(1);
            }
            run_file(&args[2]);
        }
        "repl" => {
            if let Err(e) = neap::repl::run() {
                eprintln!("REPL error: {e}");
                process::exit(1);
            }
        }
        cmd => {
            eprintln!("Unknown command: {cmd}");
            process::exit(1);
        }
    }
}

fn lex_file(path: &str) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file '{path}': {e}");
            process::exit(1);
        }
    };

    match Lexer::tokenize(&source) {
        Ok(tokens) => {
            for token in tokens {
                println!("{:?}", token);
            }
        }
        Err(e) => {
            eprintln!("Lexer error: {e}");
            process::exit(1);
        }
    }
}

fn parse_file(path: &str) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file '{path}': {e}");
            process::exit(1);
        }
    };

    let mut parser = match Parser::new(&source) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Lexer error: {e}");
            process::exit(1);
        }
    };

    match parser.parse_program() {
        Ok(program) => {
            println!("{:#?}", program);
        }
        Err(e) => {
            eprintln!("Parse error: {e}");
            process::exit(1);
        }
    }
}

fn check_file(path: &str) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file '{path}': {e}");
            process::exit(1);
        }
    };

    let mut parser = match Parser::new(&source) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Lexer error: {e}");
            process::exit(1);
        }
    };

    let program = match parser.parse_program() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Parse error: {e}");
            process::exit(1);
        }
    };

    let mut checker = TypeChecker::new();
    match checker.check_program(&program) {
        Ok(()) => {
            println!("Type check successful!");
        }
        Err(errors) => {
            eprintln!("Type errors:");
            for e in &errors {
                eprintln!("  {e}");
            }
            process::exit(1);
        }
    }
}

fn run_file(path: &str) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file '{path}': {e}");
            process::exit(1);
        }
    };

    // Parse
    let mut parser = match Parser::new(&source) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Lexer error: {e}");
            process::exit(1);
        }
    };

    let program = match parser.parse_program() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Parse error: {e}");
            process::exit(1);
        }
    };

    // Type check
    let mut checker = TypeChecker::new();
    if let Err(errors) = checker.check_program(&program) {
        eprintln!("Type errors:");
        for e in &errors {
            eprintln!("  {e}");
        }
        process::exit(1);
    }

    // Lower to IR
    let mut lowerer = Lower::new();
    let ir_program = match lowerer.lower_program(&program) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Lowering error: {e}");
            process::exit(1);
        }
    };

    // Optimize
    let optimizer = Optimizer::new();
    let optimized = optimizer.optimize(ir_program);

    // Run
    let stdout = std::io::stdout();
    let stdin = std::io::stdin();
    let mut vm = VM::new(stdout.lock(), stdin.lock());

    if let Err(e) = vm.eval_program(&optimized) {
        eprintln!("Runtime error: {e}");
        process::exit(1);
    }
}
