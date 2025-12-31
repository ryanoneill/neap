//! Syntax module: Lexing, parsing, and AST definitions for Neap

mod span;
mod token;
mod lexer;
mod ast;
mod parser;

pub use span::{Span, Spanned};
pub use token::{Token, TokenKind};
pub use lexer::{Lexer, LexerError};
pub use ast::*;
pub use parser::{Parser, ParseError};
