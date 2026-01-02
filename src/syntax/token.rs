//! Token definitions for the Neap lexer
//!
//! This module defines all token types recognized by the Neap language,
//! including ML keywords, shell-specific operators, and literals.

use super::span::Span;

/// A token with its kind and source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// The kind of token
    pub kind: TokenKind,
    /// The source span where this token was found
    pub span: Span,
}

impl Token {
    /// Create a new token.
    #[must_use]
    pub const fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// The kind of a token.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ========== Keywords ==========
    /// `let` - value binding
    Let,
    /// `in` - let body
    In,
    /// `fn` - anonymous function
    Fn,
    /// `fun` - named function
    Fun,
    /// `rec` - recursive binding
    Rec,
    /// `and` - parallel bindings
    And,
    /// `match` - pattern matching
    Match,
    /// `with` - match arms
    With,
    /// `case` - case expression (alternative to match)
    Case,
    /// `of` - case/datatype arms
    Of,
    /// `datatype` - algebraic data type
    Datatype,
    /// `type` - type alias
    Type,
    /// `if` - conditional
    If,
    /// `then` - conditional true branch
    Then,
    /// `else` - conditional false branch
    Else,
    /// `do` - IO sequencing block
    Do,
    /// `end` - block terminator
    End,
    /// `true` - boolean literal
    True,
    /// `false` - boolean literal
    False,

    // ========== Type Classes ==========
    /// `trait` - trait definition
    Trait,
    /// `impl` - trait implementation
    Impl,
    /// `for` - used in `impl Trait for Type`
    For,

    // ========== Shell Sugar ==========
    /// `|>` - pipe operator
    Pipe,
    /// `>` - redirect stdout (also used for greater-than)
    Gt,
    /// `>>` - append redirect
    GtGt,
    /// `<` - redirect stdin (also used for less-than)
    Lt,
    /// `$` - environment variable prefix
    Dollar,
    /// `` ` `` - command interpolation (backtick)
    Backtick,
    /// Text inside a command (between backticks)
    CommandText(String),

    // ========== Operators ==========
    /// `+` - addition
    Plus,
    /// `++` - string/list concatenation (Haskell-style)
    PlusPlus,
    /// `-` - subtraction
    Minus,
    /// `*` - multiplication
    Star,
    /// `/` - division
    Slash,
    /// `%` - modulo
    Percent,
    /// `=` - equality / binding
    Eq,
    /// `<>` - not equal
    Neq,
    /// `<=` - less than or equal
    Le,
    /// `>=` - greater than or equal
    Ge,
    /// `^` - string concatenation
    Caret,
    /// `::` - list cons
    ColonColon,
    /// `@` - list append
    At,
    /// `&&` or `andalso` - logical and
    AndAlso,
    /// `||` or `orelse` - logical or
    OrElse,
    /// `!` or `not` - logical negation / reference dereference
    Bang,
    /// `:=` - reference assignment
    ColonEq,

    // ========== Arrows ==========
    /// `->` - function type / lambda body
    Arrow,
    /// `=>` - pattern clause / type constraint
    FatArrow,
    /// `<-` - monadic bind
    LeftArrow,

    // ========== Delimiters ==========
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `,`
    Comma,
    /// `;`
    Semi,
    /// `:`
    Colon,
    /// `|` - pattern alternative
    Bar,
    /// `_` - wildcard pattern
    Underscore,
    /// `.` - record access / qualified name
    Dot,
    /// `...` - ellipsis (for records)
    Ellipsis,

    // ========== Literals ==========
    /// Integer literal (e.g., `42`, `0xFF`, `0b1010`)
    Int(i64),
    /// Floating-point literal (e.g., `3.14`, `1e10`)
    Float(f64),
    /// String literal (e.g., `"hello"`)
    String(String),
    /// Character literal (e.g., `#"a"`)
    Char(char),

    // ========== Identifiers ==========
    /// Identifier (e.g., `foo`, `myFunc`)
    Ident(String),
    /// Type variable (e.g., `'a`, `'key`)
    TyVar(String),
    /// Uppercase identifier (for constructors, e.g., `Some`, `None`)
    UpperIdent(String),

    // ========== Special ==========
    /// End of file
    Eof,
}

impl TokenKind {
    /// Check if this token is a keyword.
    #[must_use]
    pub const fn is_keyword(&self) -> bool {
        matches!(
            self,
            Self::Let
                | Self::In
                | Self::Fn
                | Self::Fun
                | Self::Rec
                | Self::And
                | Self::Match
                | Self::With
                | Self::Case
                | Self::Of
                | Self::Datatype
                | Self::Type
                | Self::If
                | Self::Then
                | Self::Else
                | Self::Do
                | Self::End
                | Self::True
                | Self::False
                | Self::Trait
                | Self::Impl
                | Self::For
        )
    }

    /// Check if this token is a literal.
    #[must_use]
    pub const fn is_literal(&self) -> bool {
        matches!(
            self,
            Self::Int(_) | Self::Float(_) | Self::String(_) | Self::Char(_)
        )
    }

    /// Check if this token is an operator.
    #[must_use]
    pub const fn is_operator(&self) -> bool {
        matches!(
            self,
            Self::Plus
                | Self::Minus
                | Self::Star
                | Self::Slash
                | Self::Percent
                | Self::Eq
                | Self::Neq
                | Self::Lt
                | Self::Le
                | Self::Gt
                | Self::Ge
                | Self::Caret
                | Self::ColonColon
                | Self::At
                | Self::AndAlso
                | Self::OrElse
                | Self::Bang
                | Self::Pipe
        )
    }

    /// Check if this token can start an expression.
    #[must_use]
    pub const fn can_start_expr(&self) -> bool {
        matches!(
            self,
            Self::Let
                | Self::Fn
                | Self::If
                | Self::Match
                | Self::Case
                | Self::Do
                | Self::LParen
                | Self::LBracket
                | Self::LBrace
                | Self::Int(_)
                | Self::Float(_)
                | Self::String(_)
                | Self::Char(_)
                | Self::True
                | Self::False
                | Self::Ident(_)
                | Self::UpperIdent(_)
                | Self::Dollar
                | Self::Minus
                | Self::Bang
        )
    }

    /// Get the keyword for an identifier, if it is one.
    #[must_use]
    pub fn keyword_from_str(s: &str) -> Option<Self> {
        match s {
            "let" => Some(Self::Let),
            "in" => Some(Self::In),
            "fn" => Some(Self::Fn),
            "fun" => Some(Self::Fun),
            "rec" => Some(Self::Rec),
            "and" => Some(Self::And),
            "match" => Some(Self::Match),
            "with" => Some(Self::With),
            "case" => Some(Self::Case),
            "of" => Some(Self::Of),
            "datatype" => Some(Self::Datatype),
            "type" => Some(Self::Type),
            "if" => Some(Self::If),
            "then" => Some(Self::Then),
            "else" => Some(Self::Else),
            "do" => Some(Self::Do),
            "end" => Some(Self::End),
            "true" => Some(Self::True),
            "false" => Some(Self::False),
            "andalso" => Some(Self::AndAlso),
            "orelse" => Some(Self::OrElse),
            "not" => Some(Self::Bang),
            "trait" => Some(Self::Trait),
            "impl" => Some(Self::Impl),
            "for" => Some(Self::For),
            _ => None,
        }
    }

    /// Get a human-readable name for this token kind.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Let => "let",
            Self::In => "in",
            Self::Fn => "fn",
            Self::Fun => "fun",
            Self::Rec => "rec",
            Self::And => "and",
            Self::Match => "match",
            Self::With => "with",
            Self::Case => "case",
            Self::Of => "of",
            Self::Datatype => "datatype",
            Self::Type => "type",
            Self::If => "if",
            Self::Then => "then",
            Self::Else => "else",
            Self::Do => "do",
            Self::End => "end",
            Self::True => "true",
            Self::False => "false",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::For => "for",
            Self::Pipe => "|>",
            Self::Gt => ">",
            Self::GtGt => ">>",
            Self::Lt => "<",
            Self::Dollar => "$",
            Self::Backtick => "`",
            Self::CommandText(_) => "command text",
            Self::Plus => "+",
            Self::PlusPlus => "++",
            Self::Minus => "-",
            Self::Star => "*",
            Self::Slash => "/",
            Self::Percent => "%",
            Self::Eq => "=",
            Self::Neq => "<>",
            Self::Le => "<=",
            Self::Ge => ">=",
            Self::Caret => "^",
            Self::ColonColon => "::",
            Self::At => "@",
            Self::AndAlso => "andalso",
            Self::OrElse => "orelse",
            Self::Bang => "!",
            Self::ColonEq => ":=",
            Self::Arrow => "->",
            Self::FatArrow => "=>",
            Self::LeftArrow => "<-",
            Self::LParen => "(",
            Self::RParen => ")",
            Self::LBracket => "[",
            Self::RBracket => "]",
            Self::LBrace => "{",
            Self::RBrace => "}",
            Self::Comma => ",",
            Self::Semi => ";",
            Self::Colon => ":",
            Self::Bar => "|",
            Self::Underscore => "_",
            Self::Dot => ".",
            Self::Ellipsis => "...",
            Self::Int(_) => "integer",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::Char(_) => "char",
            Self::Ident(_) => "identifier",
            Self::TyVar(_) => "type variable",
            Self::UpperIdent(_) => "constructor",
            Self::Eof => "end of file",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_detection() {
        assert!(TokenKind::Let.is_keyword());
        assert!(TokenKind::Match.is_keyword());
        assert!(TokenKind::True.is_keyword());
        assert!(!TokenKind::Plus.is_keyword());
        assert!(!TokenKind::Ident("foo".to_string()).is_keyword());
    }

    #[test]
    fn literal_detection() {
        assert!(TokenKind::Int(42).is_literal());
        assert!(TokenKind::Float(3.14).is_literal());
        assert!(TokenKind::String("hello".to_string()).is_literal());
        assert!(TokenKind::Char('a').is_literal());
        assert!(!TokenKind::Let.is_literal());
    }

    #[test]
    fn operator_detection() {
        assert!(TokenKind::Plus.is_operator());
        assert!(TokenKind::Pipe.is_operator());
        assert!(TokenKind::AndAlso.is_operator());
        assert!(!TokenKind::Let.is_operator());
        assert!(!TokenKind::LParen.is_operator());
    }

    #[test]
    fn keyword_from_str() {
        assert_eq!(TokenKind::keyword_from_str("let"), Some(TokenKind::Let));
        assert_eq!(TokenKind::keyword_from_str("match"), Some(TokenKind::Match));
        assert_eq!(TokenKind::keyword_from_str("true"), Some(TokenKind::True));
        assert_eq!(
            TokenKind::keyword_from_str("andalso"),
            Some(TokenKind::AndAlso)
        );
        assert_eq!(TokenKind::keyword_from_str("foo"), None);
        assert_eq!(TokenKind::keyword_from_str("Let"), None); // Case sensitive
    }

    #[test]
    fn token_names() {
        assert_eq!(TokenKind::Let.name(), "let");
        assert_eq!(TokenKind::Pipe.name(), "|>");
        assert_eq!(TokenKind::Arrow.name(), "->");
        assert_eq!(TokenKind::Int(42).name(), "integer");
        assert_eq!(TokenKind::Ident("foo".to_string()).name(), "identifier");
    }

    #[test]
    fn can_start_expr() {
        assert!(TokenKind::Let.can_start_expr());
        assert!(TokenKind::If.can_start_expr());
        assert!(TokenKind::Int(1).can_start_expr());
        assert!(TokenKind::Ident("x".to_string()).can_start_expr());
        assert!(TokenKind::LParen.can_start_expr());
        assert!(!TokenKind::RParen.can_start_expr());
        assert!(!TokenKind::Comma.can_start_expr());
    }
}
