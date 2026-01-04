//! Lexer for the Neap language
//!
//! Tokenizes source code into a stream of tokens, handling ML syntax
//! and shell-specific constructs.

use super::span::Span;
use super::token::{Token, TokenKind};
use thiserror::Error;

/// Errors that can occur during lexing.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum LexerError {
    /// Unexpected character in input
    #[error("unexpected character '{ch}' at position {pos}")]
    UnexpectedChar { ch: char, pos: usize },

    /// Unterminated string literal
    #[error("unterminated string literal starting at position {start}")]
    UnterminatedString { start: usize },

    /// Unterminated character literal
    #[error("unterminated character literal starting at position {start}")]
    UnterminatedChar { start: usize },

    /// Unterminated comment
    #[error("unterminated comment starting at position {start}")]
    UnterminatedComment { start: usize },

    /// Invalid escape sequence
    #[error("invalid escape sequence '\\{ch}' at position {pos}")]
    InvalidEscape { ch: char, pos: usize },

    /// Invalid number literal
    #[error("invalid number literal at position {pos}: {reason}")]
    InvalidNumber { pos: usize, reason: String },

    /// Empty character literal
    #[error("empty character literal at position {pos}")]
    EmptyCharLiteral { pos: usize },

    /// Character literal with multiple characters
    #[error("character literal at position {pos} contains multiple characters")]
    MultiCharLiteral { pos: usize },
}

/// The Neap lexer.
///
/// Tokenizes source code into a stream of tokens.
pub struct Lexer<'src> {
    /// The source code being lexed (kept for error reporting)
    #[allow(dead_code)]
    source: &'src str,
    /// The remaining source to be lexed
    rest: &'src str,
    /// Current byte position in source
    pos: usize,
    /// Whether we're inside a command (between backticks)
    in_command: bool,
    /// Depth of braces for interpolation (0 = not in interpolation)
    brace_depth: usize,
}

impl<'src> Lexer<'src> {
    /// Create a new lexer for the given source code.
    #[must_use]
    pub fn new(source: &'src str) -> Self {
        Self {
            source,
            rest: source,
            pos: 0,
            in_command: false,
            brace_depth: 0,
        }
    }

    /// Tokenize the entire source, returning all tokens or the first error.
    pub fn tokenize(source: &str) -> Result<Vec<Token>, LexerError> {
        let mut lexer = Lexer::new(source);
        let mut tokens = Vec::new();

        loop {
            let token = lexer.next_token()?;
            let is_eof = token.kind == TokenKind::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }

        Ok(tokens)
    }

    /// Get the next token from the input.
    pub fn next_token(&mut self) -> Result<Token, LexerError> {
        // In command mode (inside backticks), lex command text
        if self.in_command && self.brace_depth == 0 {
            return self.lex_command_content();
        }

        self.skip_whitespace_and_comments()?;

        if self.rest.is_empty() {
            return Ok(Token::new(TokenKind::Eof, Span::empty(self.pos)));
        }

        let start = self.pos;
        let first = self.peek_char().unwrap();

        // Standalone underscore is a wildcard token
        if first == '_' && !self.peek_char_at(1).is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            self.advance();
            return Ok(Token::new(TokenKind::Underscore, Span::new(start, self.pos)));
        }

        // Identifiers and keywords
        if first.is_ascii_alphabetic() || first == '_' {
            return Ok(self.lex_identifier());
        }

        // Type variables ('a, 'key, etc.)
        if first == '\'' {
            return Ok(self.lex_type_var());
        }

        // Numbers
        if first.is_ascii_digit() {
            return self.lex_number();
        }

        // String literals
        if first == '"' {
            return self.lex_string();
        }

        // Character literals (#"a")
        if first == '#' && self.peek_char_at(1) == Some('"') {
            return self.lex_char();
        }

        // Operators and punctuation
        self.lex_operator_or_punct(start)
    }

    /// Skip whitespace and comments.
    fn skip_whitespace_and_comments(&mut self) -> Result<(), LexerError> {
        loop {
            // Skip whitespace
            while let Some(ch) = self.peek_char() {
                if ch.is_whitespace() {
                    self.advance();
                } else {
                    break;
                }
            }

            // Check for ML-style comments (* ... *)
            if self.rest.starts_with("(*") {
                self.skip_comment()?;
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Skip an ML-style nested comment.
    fn skip_comment(&mut self) -> Result<(), LexerError> {
        let start = self.pos;
        self.advance(); // (
        self.advance(); // *

        let mut depth = 1;

        while depth > 0 {
            if self.rest.is_empty() {
                return Err(LexerError::UnterminatedComment { start });
            }

            if self.rest.starts_with("(*") {
                depth += 1;
                self.advance();
                self.advance();
            } else if self.rest.starts_with("*)") {
                depth -= 1;
                self.advance();
                self.advance();
            } else {
                self.advance();
            }
        }

        Ok(())
    }

    /// Lex an identifier or keyword.
    fn lex_identifier(&mut self) -> Token {
        let start = self.pos;
        let mut ident = String::new();

        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '\'' {
                ident.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let span = Span::new(start, self.pos);

        // Check if it's a keyword
        if let Some(keyword) = TokenKind::keyword_from_str(&ident) {
            return Token::new(keyword, span);
        }

        // Check if it starts with uppercase (constructor)
        let kind = if ident.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            TokenKind::UpperIdent(ident)
        } else {
            TokenKind::Ident(ident)
        };

        Token::new(kind, span)
    }

    /// Lex a type variable ('a, 'key, etc.).
    fn lex_type_var(&mut self) -> Token {
        let start = self.pos;
        self.advance(); // skip '

        let mut name = String::new();
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                name.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        Token::new(TokenKind::TyVar(name), Span::new(start, self.pos))
    }

    /// Lex a number (integer or float).
    fn lex_number(&mut self) -> Result<Token, LexerError> {
        let start = self.pos;

        // Check for hex or binary prefix
        if self.peek_char() == Some('0') {
            match self.peek_char_at(1) {
                Some('x') | Some('X') => return self.lex_hex_number(start),
                Some('b') | Some('B') => return self.lex_binary_number(start),
                Some('o') | Some('O') => return self.lex_octal_number(start),
                _ => {}
            }
        }

        // Lex decimal digits
        let mut num_str = String::new();
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() || ch == '_' {
                if ch != '_' {
                    num_str.push(ch);
                }
                self.advance();
            } else {
                break;
            }
        }

        // Check for float
        let is_float = self.peek_char() == Some('.')
            && self.peek_char_at(1).is_some_and(|c| c.is_ascii_digit());

        if is_float {
            self.advance(); // skip .
            num_str.push('.');

            while let Some(ch) = self.peek_char() {
                if ch.is_ascii_digit() || ch == '_' {
                    if ch != '_' {
                        num_str.push(ch);
                    }
                    self.advance();
                } else {
                    break;
                }
            }

            // Check for exponent
            if let Some('e' | 'E') = self.peek_char() {
                num_str.push('e');
                self.advance();

                if let Some('+' | '-') = self.peek_char() {
                    num_str.push(self.peek_char().unwrap());
                    self.advance();
                }

                while let Some(ch) = self.peek_char() {
                    if ch.is_ascii_digit() {
                        num_str.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }

            let value: f64 = num_str.parse().map_err(|_| LexerError::InvalidNumber {
                pos: start,
                reason: format!("invalid float literal: {num_str}"),
            })?;

            Ok(Token::new(TokenKind::Float(value), Span::new(start, self.pos)))
        } else {
            // Check for exponent (makes it a float)
            if let Some('e' | 'E') = self.peek_char() {
                num_str.push('e');
                self.advance();

                if let Some('+' | '-') = self.peek_char() {
                    num_str.push(self.peek_char().unwrap());
                    self.advance();
                }

                while let Some(ch) = self.peek_char() {
                    if ch.is_ascii_digit() {
                        num_str.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                }

                let value: f64 = num_str.parse().map_err(|_| LexerError::InvalidNumber {
                    pos: start,
                    reason: format!("invalid float literal: {num_str}"),
                })?;

                return Ok(Token::new(
                    TokenKind::Float(value),
                    Span::new(start, self.pos),
                ));
            }

            let value: i64 = num_str.parse().map_err(|_| LexerError::InvalidNumber {
                pos: start,
                reason: format!("invalid integer literal: {num_str}"),
            })?;

            Ok(Token::new(TokenKind::Int(value), Span::new(start, self.pos)))
        }
    }

    /// Lex a hexadecimal number.
    fn lex_hex_number(&mut self, start: usize) -> Result<Token, LexerError> {
        self.advance(); // 0
        self.advance(); // x

        let mut num_str = String::new();
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_hexdigit() || ch == '_' {
                if ch != '_' {
                    num_str.push(ch);
                }
                self.advance();
            } else {
                break;
            }
        }

        if num_str.is_empty() {
            return Err(LexerError::InvalidNumber {
                pos: start,
                reason: "expected hex digits after 0x".to_string(),
            });
        }

        let value = i64::from_str_radix(&num_str, 16).map_err(|_| LexerError::InvalidNumber {
            pos: start,
            reason: format!("invalid hex literal: 0x{num_str}"),
        })?;

        Ok(Token::new(TokenKind::Int(value), Span::new(start, self.pos)))
    }

    /// Lex a binary number.
    fn lex_binary_number(&mut self, start: usize) -> Result<Token, LexerError> {
        self.advance(); // 0
        self.advance(); // b

        let mut num_str = String::new();
        while let Some(ch) = self.peek_char() {
            if ch == '0' || ch == '1' || ch == '_' {
                if ch != '_' {
                    num_str.push(ch);
                }
                self.advance();
            } else {
                break;
            }
        }

        if num_str.is_empty() {
            return Err(LexerError::InvalidNumber {
                pos: start,
                reason: "expected binary digits after 0b".to_string(),
            });
        }

        let value = i64::from_str_radix(&num_str, 2).map_err(|_| LexerError::InvalidNumber {
            pos: start,
            reason: format!("invalid binary literal: 0b{num_str}"),
        })?;

        Ok(Token::new(TokenKind::Int(value), Span::new(start, self.pos)))
    }

    /// Lex an octal number.
    fn lex_octal_number(&mut self, start: usize) -> Result<Token, LexerError> {
        self.advance(); // 0
        self.advance(); // o

        let mut num_str = String::new();
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() && ch < '8' || ch == '_' {
                if ch != '_' {
                    num_str.push(ch);
                }
                self.advance();
            } else {
                break;
            }
        }

        if num_str.is_empty() {
            return Err(LexerError::InvalidNumber {
                pos: start,
                reason: "expected octal digits after 0o".to_string(),
            });
        }

        let value = i64::from_str_radix(&num_str, 8).map_err(|_| LexerError::InvalidNumber {
            pos: start,
            reason: format!("invalid octal literal: 0o{num_str}"),
        })?;

        Ok(Token::new(TokenKind::Int(value), Span::new(start, self.pos)))
    }

    /// Lex a string literal.
    fn lex_string(&mut self) -> Result<Token, LexerError> {
        let start = self.pos;
        self.advance(); // skip opening "

        let mut value = String::new();

        loop {
            match self.peek_char() {
                None => return Err(LexerError::UnterminatedString { start }),
                Some('"') => {
                    self.advance();
                    break;
                }
                Some('\\') => {
                    self.advance();
                    let escaped = self.lex_escape_sequence()?;
                    value.push(escaped);
                }
                Some(ch) => {
                    value.push(ch);
                    self.advance();
                }
            }
        }

        Ok(Token::new(
            TokenKind::String(value),
            Span::new(start, self.pos),
        ))
    }

    /// Lex command content (text inside backticks).
    ///
    /// Returns either:
    /// - `CommandText(String)` for literal text
    /// - `LBrace` when hitting `{` for interpolation
    /// - `Backtick` when hitting the closing backtick
    fn lex_command_content(&mut self) -> Result<Token, LexerError> {
        let start = self.pos;

        // Check for special characters first
        match self.peek_char() {
            None => {
                // Unterminated command - return EOF and let parser handle error
                return Ok(Token::new(TokenKind::Eof, Span::empty(self.pos)));
            }
            Some('`') => {
                // Closing backtick - exit command mode
                self.advance();
                self.in_command = false;
                return Ok(Token::new(TokenKind::Backtick, Span::new(start, self.pos)));
            }
            Some('{') => {
                // Start of interpolation
                self.advance();
                self.brace_depth = 1;
                return Ok(Token::new(TokenKind::LBrace, Span::new(start, self.pos)));
            }
            _ => {}
        }

        // Collect command text until we hit `, {, or end of input
        let mut text = String::new();
        while let Some(ch) = self.peek_char() {
            match ch {
                '`' | '{' => break,
                '\\' => {
                    // Escape sequence in command
                    self.advance();
                    if let Some(next) = self.peek_char() {
                        // Allow escaping ` and { in commands
                        if next == '`' || next == '{' || next == '\\' {
                            text.push(next);
                            self.advance();
                        } else {
                            // Other escapes: keep the backslash
                            text.push('\\');
                        }
                    }
                }
                _ => {
                    text.push(ch);
                    self.advance();
                }
            }
        }

        Ok(Token::new(
            TokenKind::CommandText(text),
            Span::new(start, self.pos),
        ))
    }

    /// Lex a character literal (#"a").
    fn lex_char(&mut self) -> Result<Token, LexerError> {
        let start = self.pos;
        self.advance(); // skip #
        self.advance(); // skip opening "

        let ch = match self.peek_char() {
            None => return Err(LexerError::UnterminatedChar { start }),
            Some('"') => return Err(LexerError::EmptyCharLiteral { pos: start }),
            Some('\\') => {
                self.advance();
                self.lex_escape_sequence()?
            }
            Some(ch) => {
                self.advance();
                ch
            }
        };

        match self.peek_char() {
            Some('"') => {
                self.advance();
            }
            Some(_) => return Err(LexerError::MultiCharLiteral { pos: start }),
            None => return Err(LexerError::UnterminatedChar { start }),
        }

        Ok(Token::new(TokenKind::Char(ch), Span::new(start, self.pos)))
    }

    /// Lex an escape sequence (after the backslash).
    fn lex_escape_sequence(&mut self) -> Result<char, LexerError> {
        let pos = self.pos;
        match self.peek_char() {
            None => Err(LexerError::InvalidEscape { ch: ' ', pos }),
            Some(ch) => {
                self.advance();
                match ch {
                    'n' => Ok('\n'),
                    'r' => Ok('\r'),
                    't' => Ok('\t'),
                    '\\' => Ok('\\'),
                    '"' => Ok('"'),
                    '\'' => Ok('\''),
                    '0' => Ok('\0'),
                    _ => Err(LexerError::InvalidEscape { ch, pos }),
                }
            }
        }
    }

    /// Lex an operator or punctuation.
    fn lex_operator_or_punct(&mut self, start: usize) -> Result<Token, LexerError> {
        let make_token = |kind: TokenKind, len: usize, lexer: &mut Self| {
            for _ in 0..len {
                lexer.advance();
            }
            Ok(Token::new(kind, Span::new(start, lexer.pos)))
        };

        // Three-character operators
        if self.rest.starts_with("...") {
            return make_token(TokenKind::Ellipsis, 3, self);
        }

        // Two-character operators
        if self.rest.starts_with("|>") {
            return make_token(TokenKind::Pipe, 2, self);
        }
        if self.rest.starts_with(">>") {
            return make_token(TokenKind::GtGt, 2, self);
        }
        if self.rest.starts_with(">=") {
            return make_token(TokenKind::Ge, 2, self);
        }
        if self.rest.starts_with("<=") {
            return make_token(TokenKind::Le, 2, self);
        }
        if self.rest.starts_with("!=") {
            return make_token(TokenKind::Neq, 2, self);
        }
        if self.rest.starts_with("<-") {
            return make_token(TokenKind::LeftArrow, 2, self);
        }
        if self.rest.starts_with("->") {
            return make_token(TokenKind::Arrow, 2, self);
        }
        if self.rest.starts_with("=>") {
            return make_token(TokenKind::FatArrow, 2, self);
        }
        if self.rest.starts_with("::") {
            return make_token(TokenKind::ColonColon, 2, self);
        }
        if self.rest.starts_with(":=") {
            return make_token(TokenKind::ColonEq, 2, self);
        }
        if self.rest.starts_with("&&") {
            return make_token(TokenKind::AndAlso, 2, self);
        }
        if self.rest.starts_with("||") {
            return make_token(TokenKind::OrElse, 2, self);
        }
        if self.rest.starts_with("++") {
            return make_token(TokenKind::PlusPlus, 2, self);
        }
        if self.rest.starts_with("==") {
            return make_token(TokenKind::EqEq, 2, self);
        }

        // Single-character operators and punctuation
        let ch = self.peek_char().unwrap();

        // Handle backtick specially - toggles command mode
        if ch == '`' {
            self.advance();
            self.in_command = true;
            return Ok(Token::new(TokenKind::Backtick, Span::new(start, self.pos)));
        }

        // Handle braces with depth tracking for command interpolation
        if ch == '{' {
            self.advance();
            if self.in_command {
                self.brace_depth += 1;
            }
            return Ok(Token::new(TokenKind::LBrace, Span::new(start, self.pos)));
        }

        if ch == '}' {
            self.advance();
            if self.in_command && self.brace_depth > 0 {
                self.brace_depth -= 1;
            }
            return Ok(Token::new(TokenKind::RBrace, Span::new(start, self.pos)));
        }

        let kind = match ch {
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            ';' => TokenKind::Semi,
            ':' => TokenKind::Colon,
            '|' => TokenKind::Bar,
            '.' => TokenKind::Dot,
            '+' => TokenKind::Plus,
            '-' => TokenKind::Minus,
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '%' => TokenKind::Percent,
            '=' => TokenKind::Eq,
            '<' => TokenKind::Lt,
            '>' => TokenKind::Gt,
            '^' => TokenKind::Caret,
            '@' => TokenKind::At,
            '!' => TokenKind::Bang,
            '$' => TokenKind::Dollar,
            _ => return Err(LexerError::UnexpectedChar { ch, pos: start }),
        };

        make_token(kind, 1, self)
    }

    /// Peek at the current character without consuming it.
    fn peek_char(&self) -> Option<char> {
        self.rest.chars().next()
    }

    /// Peek at the character at a given offset.
    fn peek_char_at(&self, offset: usize) -> Option<char> {
        self.rest.chars().nth(offset)
    }

    /// Advance to the next character.
    fn advance(&mut self) {
        if let Some(ch) = self.rest.chars().next() {
            let len = ch.len_utf8();
            self.pos += len;
            self.rest = &self.rest[len..];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to tokenize and collect just the token kinds.
    fn token_kinds(source: &str) -> Result<Vec<TokenKind>, LexerError> {
        let tokens = Lexer::tokenize(source)?;
        Ok(tokens.into_iter().map(|t| t.kind).collect())
    }

    // ========== Basic Tokens ==========

    #[test]
    fn empty_input() {
        let tokens = token_kinds("").unwrap();
        assert_eq!(tokens, vec![TokenKind::Eof]);
    }

    #[test]
    fn whitespace_only() {
        let tokens = token_kinds("   \n\t  ").unwrap();
        assert_eq!(tokens, vec![TokenKind::Eof]);
    }

    // ========== Keywords ==========

    #[test]
    fn keywords() {
        let tokens = token_kinds("let in fn fun rec and match with case of").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Let,
                TokenKind::In,
                TokenKind::Fn,
                TokenKind::Fun,
                TokenKind::Rec,
                TokenKind::And,
                TokenKind::Match,
                TokenKind::With,
                TokenKind::Case,
                TokenKind::Of,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn more_keywords() {
        let tokens = token_kinds("datatype type if then else do end true false").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Datatype,
                TokenKind::Type,
                TokenKind::If,
                TokenKind::Then,
                TokenKind::Else,
                TokenKind::Do,
                TokenKind::End,
                TokenKind::True,
                TokenKind::False,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn keyword_operators() {
        let tokens = token_kinds("andalso orelse not").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::AndAlso,
                TokenKind::OrElse,
                TokenKind::Bang,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn type_class_keywords() {
        let tokens = token_kinds("trait impl for").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Trait,
                TokenKind::Impl,
                TokenKind::For,
                TokenKind::Eof,
            ]
        );
    }

    // ========== Identifiers ==========

    #[test]
    fn identifiers() {
        let tokens = token_kinds("foo bar_baz x123 _private").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("foo".to_string()),
                TokenKind::Ident("bar_baz".to_string()),
                TokenKind::Ident("x123".to_string()),
                TokenKind::Ident("_private".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn upper_identifiers() {
        let tokens = token_kinds("Some None True' MyType").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::UpperIdent("Some".to_string()),
                TokenKind::UpperIdent("None".to_string()),
                TokenKind::UpperIdent("True'".to_string()),
                TokenKind::UpperIdent("MyType".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn type_variables() {
        let tokens = token_kinds("'a 'key 'T").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::TyVar("a".to_string()),
                TokenKind::TyVar("key".to_string()),
                TokenKind::TyVar("T".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    // ========== Numbers ==========

    #[test]
    fn integers() {
        let tokens = token_kinds("0 42 1_000_000").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Int(0),
                TokenKind::Int(42),
                TokenKind::Int(1_000_000),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn hex_numbers() {
        let tokens = token_kinds("0xFF 0x1A2B 0xdead_beef").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Int(0xFF),
                TokenKind::Int(0x1A2B),
                TokenKind::Int(0xDEAD_BEEF),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn binary_numbers() {
        let tokens = token_kinds("0b1010 0B1111_0000").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Int(0b1010),
                TokenKind::Int(0b1111_0000),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn octal_numbers() {
        let tokens = token_kinds("0o755 0O644").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Int(0o755),
                TokenKind::Int(0o644),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn floats() {
        let tokens = token_kinds("3.14 0.5 1.0").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Float(3.14),
                TokenKind::Float(0.5),
                TokenKind::Float(1.0),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn float_with_exponent() {
        let tokens = token_kinds("1e10 2.5e-3 1E+5").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Float(1e10),
                TokenKind::Float(2.5e-3),
                TokenKind::Float(1e5),
                TokenKind::Eof,
            ]
        );
    }

    // ========== Strings ==========

    #[test]
    fn strings() {
        let tokens = token_kinds(r#""hello" "world""#).unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::String("hello".to_string()),
                TokenKind::String("world".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn string_escapes() {
        let tokens = token_kinds(r#""hello\nworld" "tab\there" "quote\"here""#).unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::String("hello\nworld".to_string()),
                TokenKind::String("tab\there".to_string()),
                TokenKind::String("quote\"here".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn empty_string() {
        let tokens = token_kinds(r#""""#).unwrap();
        assert_eq!(
            tokens,
            vec![TokenKind::String("".to_string()), TokenKind::Eof,]
        );
    }

    // ========== Characters ==========

    #[test]
    fn char_literals() {
        let tokens = token_kinds(r#"#"a" #"Z" #"0""#).unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Char('a'),
                TokenKind::Char('Z'),
                TokenKind::Char('0'),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn char_escapes() {
        let tokens = token_kinds(r#"#"\n" #"\t" #"\\""#).unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Char('\n'),
                TokenKind::Char('\t'),
                TokenKind::Char('\\'),
                TokenKind::Eof,
            ]
        );
    }

    // ========== Operators ==========

    #[test]
    fn arithmetic_operators() {
        let tokens = token_kinds("+ - * / %").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Plus,
                TokenKind::Minus,
                TokenKind::Star,
                TokenKind::Slash,
                TokenKind::Percent,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn comparison_operators() {
        let tokens = token_kinds("== != < > <= >=").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::EqEq,
                TokenKind::Neq,
                TokenKind::Lt,
                TokenKind::Gt,
                TokenKind::Le,
                TokenKind::Ge,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn assignment_vs_equality() {
        let tokens = token_kinds("= ==").unwrap();
        assert_eq!(
            tokens,
            vec![TokenKind::Eq, TokenKind::EqEq, TokenKind::Eof,]
        );
    }

    #[test]
    fn logical_operators() {
        let tokens = token_kinds("&& || !").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::AndAlso,
                TokenKind::OrElse,
                TokenKind::Bang,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn list_operators() {
        let tokens = token_kinds(":: @").unwrap();
        assert_eq!(
            tokens,
            vec![TokenKind::ColonColon, TokenKind::At, TokenKind::Eof,]
        );
    }

    #[test]
    fn concat_operator() {
        let tokens = token_kinds("++ \"a\" ++ \"b\"").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::PlusPlus,
                TokenKind::String("a".to_string()),
                TokenKind::PlusPlus,
                TokenKind::String("b".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn shell_operators() {
        let tokens = token_kinds("|> > >> $").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Pipe,
                TokenKind::Gt,
                TokenKind::GtGt,
                TokenKind::Dollar,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn arrows() {
        let tokens = token_kinds("-> => <-").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Arrow,
                TokenKind::FatArrow,
                TokenKind::LeftArrow,
                TokenKind::Eof,
            ]
        );
    }

    // ========== Delimiters ==========

    #[test]
    fn delimiters() {
        let tokens = token_kinds("( ) [ ] { } , ; : | _ . ...").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::LBracket,
                TokenKind::RBracket,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::Comma,
                TokenKind::Semi,
                TokenKind::Colon,
                TokenKind::Bar,
                TokenKind::Underscore,
                TokenKind::Dot,
                TokenKind::Ellipsis,
                TokenKind::Eof,
            ]
        );
    }

    // ========== Comments ==========

    #[test]
    fn single_comment() {
        let tokens = token_kinds("let (* comment *) x").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Let,
                TokenKind::Ident("x".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn nested_comments() {
        let tokens = token_kinds("let (* outer (* inner *) outer *) x").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Let,
                TokenKind::Ident("x".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    // ========== Complex Examples ==========

    #[test]
    fn let_binding() {
        let tokens = token_kinds("let x = 42 in x + 1").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Let,
                TokenKind::Ident("x".to_string()),
                TokenKind::Eq,
                TokenKind::Int(42),
                TokenKind::In,
                TokenKind::Ident("x".to_string()),
                TokenKind::Plus,
                TokenKind::Int(1),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn function_definition() {
        let tokens = token_kinds("fn x => x * 2").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Fn,
                TokenKind::Ident("x".to_string()),
                TokenKind::FatArrow,
                TokenKind::Ident("x".to_string()),
                TokenKind::Star,
                TokenKind::Int(2),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn pattern_match() {
        let tokens = token_kinds("match x with | Some y -> y | None -> 0").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Match,
                TokenKind::Ident("x".to_string()),
                TokenKind::With,
                TokenKind::Bar,
                TokenKind::UpperIdent("Some".to_string()),
                TokenKind::Ident("y".to_string()),
                TokenKind::Arrow,
                TokenKind::Ident("y".to_string()),
                TokenKind::Bar,
                TokenKind::UpperIdent("None".to_string()),
                TokenKind::Arrow,
                TokenKind::Int(0),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn shell_pipeline() {
        let tokens = token_kinds(r#"cat "file.txt" |> grep "pattern""#).unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("cat".to_string()),
                TokenKind::String("file.txt".to_string()),
                TokenKind::Pipe,
                TokenKind::Ident("grep".to_string()),
                TokenKind::String("pattern".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn type_annotation() {
        let tokens = token_kinds("let x: int -> int = fn y => y").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Let,
                TokenKind::Ident("x".to_string()),
                TokenKind::Colon,
                TokenKind::Ident("int".to_string()),
                TokenKind::Arrow,
                TokenKind::Ident("int".to_string()),
                TokenKind::Eq,
                TokenKind::Fn,
                TokenKind::Ident("y".to_string()),
                TokenKind::FatArrow,
                TokenKind::Ident("y".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn datatype_definition() {
        let tokens = token_kinds("datatype 'a option = None | Some of 'a").unwrap();
        assert_eq!(
            tokens,
            vec![
                TokenKind::Datatype,
                TokenKind::TyVar("a".to_string()),
                TokenKind::Ident("option".to_string()),
                TokenKind::Eq,
                TokenKind::UpperIdent("None".to_string()),
                TokenKind::Bar,
                TokenKind::UpperIdent("Some".to_string()),
                TokenKind::Of,
                TokenKind::TyVar("a".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    // ========== Error Cases ==========

    #[test]
    fn unterminated_string() {
        let result = token_kinds(r#""hello"#);
        assert!(matches!(
            result,
            Err(LexerError::UnterminatedString { .. })
        ));
    }

    #[test]
    fn unterminated_comment() {
        let result = token_kinds("(* comment");
        assert!(matches!(
            result,
            Err(LexerError::UnterminatedComment { .. })
        ));
    }

    #[test]
    fn invalid_escape() {
        let result = token_kinds(r#""\q""#);
        assert!(matches!(result, Err(LexerError::InvalidEscape { .. })));
    }

    #[test]
    fn empty_char_literal() {
        let result = token_kinds(r#"#"""#);
        assert!(matches!(result, Err(LexerError::EmptyCharLiteral { .. })));
    }

    #[test]
    fn multi_char_literal() {
        let result = token_kinds(r#"#"ab""#);
        assert!(matches!(result, Err(LexerError::MultiCharLiteral { .. })));
    }

    #[test]
    fn invalid_hex() {
        let result = token_kinds("0x");
        assert!(matches!(result, Err(LexerError::InvalidNumber { .. })));
    }

    #[test]
    fn unexpected_char() {
        let result = token_kinds("let x = ~5");
        assert!(matches!(result, Err(LexerError::UnexpectedChar { .. })));
    }

    // ========== Span Tests ==========

    #[test]
    fn token_spans() {
        let tokens = Lexer::tokenize("let x = 42").unwrap();

        assert_eq!(tokens[0].span, Span::new(0, 3)); // "let"
        assert_eq!(tokens[1].span, Span::new(4, 5)); // "x"
        assert_eq!(tokens[2].span, Span::new(6, 7)); // "="
        assert_eq!(tokens[3].span, Span::new(8, 10)); // "42"
    }

    #[test]
    fn multiline_spans() {
        let source = "let\n  x\n  =\n  42";
        let tokens = Lexer::tokenize(source).unwrap();

        assert_eq!(tokens[0].span, Span::new(0, 3)); // "let"
        assert_eq!(tokens[1].span, Span::new(6, 7)); // "x"
        assert_eq!(tokens[2].span, Span::new(10, 11)); // "="
        assert_eq!(tokens[3].span, Span::new(14, 16)); // "42"
    }
}
