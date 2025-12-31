//! Neap CLI entry point

use neap::syntax::Lexer;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: neap <command> [args...]");
        eprintln!("Commands:");
        eprintln!("  lex <file>     Tokenize a file and print tokens");
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
        "run" | "check" | "repl" => {
            eprintln!("Command '{}' not yet implemented", args[1]);
            process::exit(1);
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
