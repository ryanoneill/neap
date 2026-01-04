//! Parser for the Neap language
//!
//! Transforms a token stream into an Abstract Syntax Tree using
//! recursive descent parsing with Pratt parsing for operators.

use super::ast::*;
use super::lexer::{Lexer, LexerError};
use super::span::{Span, Spanned};
use super::token::{Token, TokenKind};
use thiserror::Error;

/// Errors that can occur during parsing.
#[derive(Debug, Clone, Error, PartialEq)]
pub enum ParseError {
    /// Lexer error during tokenization
    #[error("lexer error: {0}")]
    LexerError(#[from] LexerError),

    /// Unexpected token
    #[error("unexpected token: expected {expected}, found {found} at {span:?}")]
    UnexpectedToken {
        expected: String,
        found: String,
        span: Span,
    },

    /// Unexpected end of file
    #[error("unexpected end of file: expected {expected}")]
    UnexpectedEof { expected: String },

    /// Invalid pattern
    #[error("invalid pattern at {span:?}")]
    InvalidPattern { span: Span },

    /// Invalid expression
    #[error("invalid expression at {span:?}")]
    InvalidExpression { span: Span },
}

/// The Neap parser.
///
/// Parses a token stream into an AST.
pub struct Parser {
    /// The tokens to parse
    tokens: Vec<Token>,
    /// Current position in the token stream
    pos: usize,
}

impl Parser {
    /// Create a new parser from source code.
    pub fn new(source: &str) -> Result<Self, ParseError> {
        let tokens = Lexer::tokenize(source)?;
        Ok(Self { tokens, pos: 0 })
    }

    /// Create a parser from a pre-tokenized stream.
    #[must_use]
    pub fn from_tokens(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    /// Parse the entire source into a program.
    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut decls = Vec::new();

        while !self.is_at_end() {
            let decl = self.parse_decl()?;
            decls.push(decl);
        }

        Ok(Program::with_decls(decls))
    }

    /// Parse a single expression (for REPL/testing).
    pub fn parse_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        self.parse_expr_bp(0)
    }

    /// Parse a single pattern (for testing).
    pub fn parse_pattern(&mut self) -> Result<Spanned<Pattern>, ParseError> {
        self.parse_pattern_inner()
    }

    /// Parse a single type expression (for testing).
    pub fn parse_type(&mut self) -> Result<Spanned<TypeExpr>, ParseError> {
        self.parse_type_expr()
    }

    /// Check if the parser has consumed all input.
    pub fn is_at_end(&self) -> bool {
        matches!(self.peek_kind(), None | Some(TokenKind::Eof))
    }

    // ========== Declaration Parsing ==========

    /// Parse a single declaration.
    pub fn parse_decl(&mut self) -> Result<Spanned<Decl>, ParseError> {
        match self.peek_kind() {
            Some(TokenKind::Let) => self.parse_let_decl(),
            Some(TokenKind::Fun) => self.parse_fun_decl(),
            Some(TokenKind::Type) => self.parse_type_decl(),
            Some(TokenKind::Datatype) => self.parse_datatype_decl(),
            Some(TokenKind::Trait) => self.parse_trait_decl(),
            Some(TokenKind::Impl) => self.parse_impl_decl(),
            Some(_) => {
                let token = self.peek().unwrap();
                Err(ParseError::UnexpectedToken {
                    expected: "declaration (let, fun, type, datatype, trait, or impl)"
                        .to_string(),
                    found: token.kind.name().to_string(),
                    span: token.span,
                })
            }
            None => Err(ParseError::UnexpectedEof {
                expected: "declaration".to_string(),
            }),
        }
    }

    fn parse_let_decl(&mut self) -> Result<Spanned<Decl>, ParseError> {
        let start = self.expect(TokenKind::Let)?.span;

        let rec = self.eat(TokenKind::Rec);

        let pattern = self.parse_pattern()?;

        let ty = if self.eat(TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(TokenKind::Eq)?;
        let expr = self.parse_expr()?;

        let span = start.merge(expr.span);
        Ok(Spanned::new(
            Decl::Val(ValDecl {
                rec,
                pattern,
                ty,
                expr,
            }),
            span,
        ))
    }

    fn parse_fun_decl(&mut self) -> Result<Spanned<Decl>, ParseError> {
        let start = self.expect(TokenKind::Fun)?.span;
        let name = self.expect_ident()?;

        let mut clauses = Vec::new();
        clauses.push(self.parse_fun_clause()?);

        // Parse additional clauses with |
        while self.eat(TokenKind::Bar) {
            // Expect the function name again
            let clause_name = self.expect_ident()?;
            if clause_name.value != name.value {
                return Err(ParseError::UnexpectedToken {
                    expected: format!("function name '{}'", name.value),
                    found: clause_name.value,
                    span: clause_name.span,
                });
            }
            clauses.push(self.parse_fun_clause()?);
        }

        let end_span = clauses.last().map(|c| c.body.span).unwrap_or(name.span);
        let span = start.merge(end_span);

        Ok(Spanned::new(
            Decl::Fun(FunDecl { name, clauses }),
            span,
        ))
    }

    fn parse_fun_clause(&mut self) -> Result<FunClause, ParseError> {
        let mut params = Vec::new();

        // Parse parameters until we hit = or :
        while !matches!(self.peek_kind(), Some(TokenKind::Eq) | Some(TokenKind::Colon) | None) {
            params.push(self.parse_atomic_pattern()?);
        }

        let result_ty = if self.eat(TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(TokenKind::Eq)?;
        let body = self.parse_expr()?;

        Ok(FunClause {
            params,
            result_ty,
            body,
        })
    }

    fn parse_type_decl(&mut self) -> Result<Spanned<Decl>, ParseError> {
        let start = self.expect(TokenKind::Type)?.span;

        let mut params = Vec::new();

        // Parse type parameters
        while let Some(TokenKind::TyVar(name)) = self.peek_kind().cloned() {
            let span = self.advance().span;
            params.push(Spanned::new(name, span));
        }

        let name = self.expect_ident()?;
        self.expect(TokenKind::Eq)?;
        let ty = self.parse_type()?;

        let span = start.merge(ty.span);
        Ok(Spanned::new(
            Decl::Type(TypeDecl { params, name, ty }),
            span,
        ))
    }

    fn parse_datatype_decl(&mut self) -> Result<Spanned<Decl>, ParseError> {
        let start = self.expect(TokenKind::Datatype)?.span;

        let mut params = Vec::new();

        // Parse type parameters
        while let Some(TokenKind::TyVar(name)) = self.peek_kind().cloned() {
            let span = self.advance().span;
            params.push(Spanned::new(name, span));
        }

        let name = self.expect_ident()?;
        self.expect(TokenKind::Eq)?;

        // Optional leading |
        self.eat(TokenKind::Bar);

        let mut constructors = Vec::new();
        constructors.push(self.parse_constructor()?);

        while self.eat(TokenKind::Bar) {
            constructors.push(self.parse_constructor()?);
        }

        let end_span = constructors
            .last()
            .map(|c| c.arg.as_ref().map(|a| a.span).unwrap_or(c.name.span))
            .unwrap_or(name.span);
        let span = start.merge(end_span);

        Ok(Spanned::new(
            Decl::Datatype(DatatypeDecl {
                params,
                name,
                constructors,
            }),
            span,
        ))
    }

    fn parse_constructor(&mut self) -> Result<Constructor, ParseError> {
        let name = self.expect_upper_ident()?;

        let arg = if self.eat(TokenKind::Of) {
            Some(self.parse_type()?)
        } else {
            None
        };

        Ok(Constructor { name, arg })
    }

    /// Parse a trait declaration.
    ///
    /// Syntax: `trait Name { fn method(self) -> Type ... }`
    fn parse_trait_decl(&mut self) -> Result<Spanned<Decl>, ParseError> {
        let start = self.expect(TokenKind::Trait)?.span;
        let name = self.expect_upper_ident()?;

        self.expect(TokenKind::LBrace)?;

        let mut methods = Vec::new();

        // Parse method signatures until we hit }
        while !matches!(self.peek_kind(), Some(TokenKind::RBrace)) && !self.is_at_end() {
            methods.push(self.parse_method_sig()?);
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        let span = start.merge(end);

        // The type parameter is implicit "self" for now
        let type_param = Spanned::new("self".to_string(), name.span);

        Ok(Spanned::new(
            Decl::Trait(TraitDecl {
                name,
                type_param,
                methods,
            }),
            span,
        ))
    }

    /// Parse a method signature in a trait.
    ///
    /// Syntax: `fn name(param1, param2, ...) -> ReturnType`
    fn parse_method_sig(&mut self) -> Result<MethodSig, ParseError> {
        self.expect(TokenKind::Fn)?;
        let name = self.expect_ident()?;

        self.expect(TokenKind::LParen)?;

        let mut params = Vec::new();

        // Parse parameters (comma-separated identifiers)
        if !matches!(self.peek_kind(), Some(TokenKind::RParen)) {
            params.push(self.expect_ident()?);
            while self.eat(TokenKind::Comma) {
                params.push(self.expect_ident()?);
            }
        }

        self.expect(TokenKind::RParen)?;
        self.expect(TokenKind::Arrow)?;

        let return_ty = self.parse_type()?;

        Ok(MethodSig {
            name,
            params,
            return_ty,
        })
    }

    /// Parse an impl declaration.
    ///
    /// Syntax: `impl TraitName for Type { fn method(param) = expr ... }`
    fn parse_impl_decl(&mut self) -> Result<Spanned<Decl>, ParseError> {
        let start = self.expect(TokenKind::Impl)?.span;
        let trait_name = self.expect_upper_ident()?;

        self.expect(TokenKind::For)?;
        let for_type = self.parse_type()?;

        self.expect(TokenKind::LBrace)?;

        let mut methods = Vec::new();

        // Parse method implementations until we hit }
        while !matches!(self.peek_kind(), Some(TokenKind::RBrace)) && !self.is_at_end() {
            methods.push(self.parse_method_impl()?);
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        let span = start.merge(end);

        Ok(Spanned::new(
            Decl::Impl(ImplDecl {
                trait_name,
                for_type,
                methods,
            }),
            span,
        ))
    }

    /// Parse a method implementation in an impl block.
    ///
    /// Syntax: `fn name(param1, param2, ...) = expr`
    /// or: `fn name(param1, param2, ...) -> ReturnType = expr`
    fn parse_method_impl(&mut self) -> Result<MethodImpl, ParseError> {
        self.expect(TokenKind::Fn)?;
        let name = self.expect_ident()?;

        self.expect(TokenKind::LParen)?;

        let mut params = Vec::new();

        // Parse parameters (comma-separated identifiers)
        if !matches!(self.peek_kind(), Some(TokenKind::RParen)) {
            params.push(self.expect_ident()?);
            while self.eat(TokenKind::Comma) {
                params.push(self.expect_ident()?);
            }
        }

        self.expect(TokenKind::RParen)?;

        // Optional return type annotation
        let return_ty = if self.eat(TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(TokenKind::Eq)?;
        let body = self.parse_expr()?;

        Ok(MethodImpl {
            name,
            params,
            return_ty,
            body,
        })
    }

    // ========== Expression Parsing (Pratt Parser) ==========

    /// Parse an expression with the given minimum binding power.
    fn parse_expr_bp(&mut self, min_bp: u8) -> Result<Spanned<Expr>, ParseError> {
        let mut lhs = self.parse_prefix_expr()?;

        loop {
            let op = match self.peek_kind() {
                None | Some(TokenKind::Eof) => break,
                Some(kind) => kind.clone(),
            };

            // Check for postfix operators (function application)
            if let Some((l_bp, ())) = postfix_binding_power(&op) {
                if l_bp < min_bp {
                    break;
                }

                lhs = self.parse_postfix_expr(lhs)?;
                continue;
            }

            // Check for infix operators
            if let Some((l_bp, r_bp)) = infix_binding_power(&op) {
                if l_bp < min_bp {
                    break;
                }

                lhs = self.parse_infix_expr(lhs, r_bp)?;
                continue;
            }

            break;
        }

        Ok(lhs)
    }

    fn parse_prefix_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        match self.peek_kind().cloned() {
            // Unary minus
            Some(TokenKind::Minus) => {
                let op_token = self.advance();
                let ((), r_bp) = prefix_binding_power(&TokenKind::Minus);
                let operand = self.parse_expr_bp(r_bp)?;
                let span = op_token.span.merge(operand.span);
                Ok(Spanned::new(
                    Expr::UnOp(UnOp::Neg, Box::new(operand)),
                    span,
                ))
            }

            // Logical not
            Some(TokenKind::Bang) => {
                let op_token = self.advance();
                let ((), r_bp) = prefix_binding_power(&TokenKind::Bang);
                let operand = self.parse_expr_bp(r_bp)?;
                let span = op_token.span.merge(operand.span);
                Ok(Spanned::new(
                    Expr::UnOp(UnOp::Not, Box::new(operand)),
                    span,
                ))
            }

            // Lambda
            Some(TokenKind::Fn) => self.parse_lambda(),

            // Conditional
            Some(TokenKind::If) => self.parse_if_expr(),

            // Match expression
            Some(TokenKind::Match) => self.parse_match_expr(),

            // Case expression (alternative syntax)
            Some(TokenKind::Case) => self.parse_case_expr(),

            // Do block
            Some(TokenKind::Do) => self.parse_do_expr(),

            // Atomic expressions
            _ => self.parse_atomic_expr(),
        }
    }

    fn parse_atomic_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        match self.peek_kind().cloned() {
            // Literals
            Some(TokenKind::Int(n)) => {
                let token = self.advance();
                Ok(Spanned::new(Expr::Lit(Literal::Int(n)), token.span))
            }

            Some(TokenKind::Float(f)) => {
                let token = self.advance();
                Ok(Spanned::new(Expr::Lit(Literal::Float(f)), token.span))
            }

            Some(TokenKind::String(s)) => {
                let token = self.advance();
                Ok(Spanned::new(Expr::Lit(Literal::String(s)), token.span))
            }

            Some(TokenKind::Char(c)) => {
                let token = self.advance();
                Ok(Spanned::new(Expr::Lit(Literal::Char(c)), token.span))
            }

            Some(TokenKind::True) => {
                let token = self.advance();
                Ok(Spanned::new(Expr::Lit(Literal::Bool(true)), token.span))
            }

            Some(TokenKind::False) => {
                let token = self.advance();
                Ok(Spanned::new(Expr::Lit(Literal::Bool(false)), token.span))
            }

            // Variables
            Some(TokenKind::Ident(name)) => {
                let token = self.advance();
                Ok(Spanned::new(Expr::Var(name), token.span))
            }

            // Constructors
            Some(TokenKind::UpperIdent(name)) => {
                let token = self.advance();
                Ok(Spanned::new(Expr::Con(name), token.span))
            }

            // Environment variables (accept both ident and upper ident for $HOME, $PATH, etc.)
            Some(TokenKind::Dollar) => {
                let start = self.advance().span;
                let name = self.expect_any_ident()?;
                let span = start.merge(name.span);
                Ok(Spanned::new(Expr::EnvVar(name.value), span))
            }

            // Parenthesized expression, tuple, or unit
            Some(TokenKind::LParen) => self.parse_paren_expr(),

            // List
            Some(TokenKind::LBracket) => self.parse_list_expr(),

            // Record
            Some(TokenKind::LBrace) => self.parse_record_expr(),

            // Shell command
            Some(TokenKind::Backtick) => self.parse_command_expr(),

            Some(_) => {
                let token = self.peek().unwrap();
                Err(ParseError::InvalidExpression { span: token.span })
            }

            None => Err(ParseError::UnexpectedEof {
                expected: "expression".to_string(),
            }),
        }
    }

    fn parse_paren_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.expect(TokenKind::LParen)?.span;

        // Unit: ()
        if self.eat(TokenKind::RParen) {
            return Ok(Spanned::new(Expr::Unit, start.merge(Span::new(start.end, start.end + 1))));
        }

        let first = self.parse_expr()?;

        // Tuple: (e1, e2, ...)
        if self.eat(TokenKind::Comma) {
            let mut elements = vec![first];
            elements.push(self.parse_expr()?);

            while self.eat(TokenKind::Comma) {
                elements.push(self.parse_expr()?);
            }

            let end = self.expect(TokenKind::RParen)?.span;
            return Ok(Spanned::new(Expr::Tuple(elements), start.merge(end)));
        }

        // Parenthesized expression
        let end = self.expect(TokenKind::RParen)?.span;
        Ok(Spanned::new(first.value, start.merge(end)))
    }

    fn parse_list_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.expect(TokenKind::LBracket)?.span;

        // Empty list: []
        if self.eat(TokenKind::RBracket) {
            return Ok(Spanned::new(Expr::List(vec![]), start.merge(Span::new(start.end, start.end + 1))));
        }

        let mut elements = vec![self.parse_expr()?];

        while self.eat(TokenKind::Comma) {
            elements.push(self.parse_expr()?);
        }

        let end = self.expect(TokenKind::RBracket)?.span;
        Ok(Spanned::new(Expr::List(elements), start.merge(end)))
    }

    fn parse_record_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.expect(TokenKind::LBrace)?.span;

        // Empty record: {}
        if self.eat(TokenKind::RBrace) {
            return Ok(Spanned::new(Expr::Record(vec![]), start.merge(Span::new(start.end, start.end + 1))));
        }

        let mut fields = Vec::new();
        fields.push(self.parse_record_field()?);

        while self.eat(TokenKind::Comma) {
            fields.push(self.parse_record_field()?);
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        Ok(Spanned::new(Expr::Record(fields), start.merge(end)))
    }

    fn parse_record_field(&mut self) -> Result<(Spanned<String>, Spanned<Expr>), ParseError> {
        let name = self.expect_ident()?;
        self.expect(TokenKind::Eq)?;
        let value = self.parse_expr()?;
        Ok((name, value))
    }

    /// Parse a shell command expression: `` `command {interpolation} more` ``
    fn parse_command_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        use crate::syntax::ast::CommandPart;

        let start = self.expect(TokenKind::Backtick)?.span;
        let mut parts = Vec::new();

        loop {
            match self.peek_kind().cloned() {
                Some(TokenKind::Backtick) => {
                    // End of command
                    let end = self.advance().span;
                    return Ok(Spanned::new(Expr::Command(parts), start.merge(end)));
                }

                Some(TokenKind::CommandText(text)) => {
                    // Literal command text
                    self.advance();
                    if !text.is_empty() {
                        parts.push(CommandPart::Literal(text));
                    }
                }

                Some(TokenKind::LBrace) => {
                    // Start of interpolation
                    self.advance();
                    let expr = self.parse_expr()?;
                    self.expect(TokenKind::RBrace)?;
                    parts.push(CommandPart::Interpolation(Box::new(expr)));
                }

                Some(TokenKind::Eof) | None => {
                    return Err(ParseError::UnexpectedEof {
                        expected: "closing backtick".to_string(),
                    });
                }

                Some(_) => {
                    // This shouldn't happen if the lexer is working correctly
                    let token = self.peek().unwrap();
                    return Err(ParseError::UnexpectedToken {
                        expected: "command text, interpolation, or closing backtick".to_string(),
                        found: token.kind.name().to_string(),
                        span: token.span,
                    });
                }
            }
        }
    }

    fn parse_lambda(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.expect(TokenKind::Fn)?.span;

        let mut params = Vec::new();
        while !matches!(self.peek_kind(), Some(TokenKind::FatArrow) | None) {
            params.push(self.parse_atomic_pattern()?);
        }

        self.expect(TokenKind::FatArrow)?;
        let body = self.parse_expr()?;

        let span = start.merge(body.span);
        Ok(Spanned::new(
            Expr::Lambda(params, Box::new(body)),
            span,
        ))
    }

    fn parse_if_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.expect(TokenKind::If)?.span;
        let cond = self.parse_expr()?;

        self.expect(TokenKind::Then)?;
        let then_branch = self.parse_expr()?;

        self.expect(TokenKind::Else)?;
        let else_branch = self.parse_expr()?;

        let span = start.merge(else_branch.span);
        Ok(Spanned::new(
            Expr::If(
                Box::new(cond),
                Box::new(then_branch),
                Box::new(else_branch),
            ),
            span,
        ))
    }

    fn parse_match_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.expect(TokenKind::Match)?.span;
        let scrutinee = self.parse_expr()?;

        self.expect(TokenKind::With)?;

        // Optional leading |
        self.eat(TokenKind::Bar);

        let mut arms = Vec::new();
        arms.push(self.parse_match_arm()?);

        while self.eat(TokenKind::Bar) {
            arms.push(self.parse_match_arm()?);
        }

        let end_span = arms.last().map(|a| a.body.span).unwrap_or(start);
        let span = start.merge(end_span);

        Ok(Spanned::new(
            Expr::Match(Box::new(scrutinee), arms),
            span,
        ))
    }

    fn parse_case_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.expect(TokenKind::Case)?.span;
        let scrutinee = self.parse_expr()?;

        self.expect(TokenKind::Of)?;

        // Optional leading |
        self.eat(TokenKind::Bar);

        let mut arms = Vec::new();
        arms.push(self.parse_match_arm()?);

        while self.eat(TokenKind::Bar) {
            arms.push(self.parse_match_arm()?);
        }

        let end_span = arms.last().map(|a| a.body.span).unwrap_or(start);
        let span = start.merge(end_span);

        Ok(Spanned::new(
            Expr::Match(Box::new(scrutinee), arms),
            span,
        ))
    }

    fn parse_match_arm(&mut self) -> Result<MatchArm, ParseError> {
        let pattern = self.parse_pattern()?;

        // TODO: Parse guards with 'when'
        let guard = None;

        self.expect(TokenKind::Arrow)?;
        let body = self.parse_expr()?;

        Ok(MatchArm {
            pattern,
            guard,
            body,
        })
    }

    fn parse_do_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.expect(TokenKind::Do)?.span;

        let mut stmts = Vec::new();

        while !matches!(self.peek_kind(), Some(TokenKind::End) | None) {
            stmts.push(self.parse_do_stmt()?);
            // Optional semicolon
            self.eat(TokenKind::Semi);
        }

        let end = self.expect(TokenKind::End)?.span;
        Ok(Spanned::new(Expr::Do(stmts), start.merge(end)))
    }

    fn parse_do_stmt(&mut self) -> Result<DoStmt, ParseError> {
        // Check for let binding
        if self.eat(TokenKind::Let) {
            let pattern = self.parse_pattern()?;
            self.expect(TokenKind::Eq)?;
            let expr = self.parse_expr()?;
            return Ok(DoStmt::Let(pattern, expr));
        }

        // Parse expression, then check for <- (bind)
        let expr = self.parse_expr()?;

        if self.eat(TokenKind::LeftArrow) {
            // This was actually a pattern
            let pattern = self.expr_to_pattern(&expr)?;
            let rhs = self.parse_expr()?;
            return Ok(DoStmt::Bind(pattern, rhs));
        }

        Ok(DoStmt::Expr(expr))
    }

    fn parse_postfix_expr(&mut self, lhs: Spanned<Expr>) -> Result<Spanned<Expr>, ParseError> {
        // Function application
        if self.peek_kind().is_some_and(can_start_atom) {
            let arg = self.parse_atomic_expr()?;
            let span = lhs.span.merge(arg.span);
            return Ok(Spanned::new(
                Expr::App(Box::new(lhs), Box::new(arg)),
                span,
            ));
        }

        // Record field access
        if self.eat(TokenKind::Dot) {
            let field = self.expect_ident()?;
            let span = lhs.span.merge(field.span);
            return Ok(Spanned::new(
                Expr::Field(Box::new(lhs), field),
                span,
            ));
        }

        Ok(lhs)
    }

    fn parse_infix_expr(
        &mut self,
        lhs: Spanned<Expr>,
        r_bp: u8,
    ) -> Result<Spanned<Expr>, ParseError> {
        let op_token = self.advance();

        match &op_token.kind {
            // Binary operators
            TokenKind::Plus => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Add, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::Minus => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Sub, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::Star => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Mul, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::Slash => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Div, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::Percent => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Mod, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::Eq => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Eq, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::Neq => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Neq, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::Lt => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Lt, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::Le => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Le, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::Gt => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Gt, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::Ge => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Ge, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::AndAlso => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::And, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::OrElse => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Or, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::PlusPlus => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Concat, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::ColonColon => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Cons, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }
            TokenKind::At => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::BinOp(BinOp::Append, Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }

            // Pipe operator
            TokenKind::Pipe => {
                let rhs = self.parse_expr_bp(r_bp)?;
                let span = lhs.span.merge(rhs.span);
                Ok(Spanned::new(
                    Expr::Pipe(Box::new(lhs), Box::new(rhs)),
                    span,
                ))
            }

            // Type annotation
            TokenKind::Colon => {
                let ty = self.parse_type()?;
                let span = lhs.span.merge(ty.span);
                Ok(Spanned::new(
                    Expr::Annot(Box::new(lhs), Box::new(ty)),
                    span,
                ))
            }

            other => Err(ParseError::UnexpectedToken {
                expected: "operator".to_string(),
                found: other.name().to_string(),
                span: op_token.span,
            }),
        }
    }

    // ========== Pattern Parsing ==========

    fn parse_pattern_inner(&mut self) -> Result<Spanned<Pattern>, ParseError> {
        let lhs = self.parse_atomic_pattern()?;

        // Check for :: (cons)
        if self.eat(TokenKind::ColonColon) {
            let rhs = self.parse_pattern_inner()?;
            let span = lhs.span.merge(rhs.span);
            return Ok(Spanned::new(
                Pattern::Cons(Box::new(lhs), Box::new(rhs)),
                span,
            ));
        }

        // Check for | (or pattern)
        if self.eat(TokenKind::Bar) {
            let rhs = self.parse_pattern_inner()?;
            let span = lhs.span.merge(rhs.span);
            return Ok(Spanned::new(
                Pattern::Or(Box::new(lhs), Box::new(rhs)),
                span,
            ));
        }

        Ok(lhs)
    }

    fn parse_atomic_pattern(&mut self) -> Result<Spanned<Pattern>, ParseError> {
        match self.peek_kind().cloned() {
            // Wildcard
            Some(TokenKind::Underscore) => {
                let token = self.advance();
                Ok(Spanned::new(Pattern::Wildcard, token.span))
            }

            // Variable or constructor
            Some(TokenKind::Ident(name)) => {
                let token = self.advance();
                Ok(Spanned::new(Pattern::Var(name), token.span))
            }

            // Constructor (possibly with argument)
            Some(TokenKind::UpperIdent(name)) => {
                let token = self.advance();
                let name_span = token.span;

                // Check for constructor argument
                if self.peek_kind().is_some_and(can_start_atom) {
                    let arg = self.parse_atomic_pattern()?;
                    let span = name_span.merge(arg.span);
                    return Ok(Spanned::new(
                        Pattern::Con(name, Some(Box::new(arg))),
                        span,
                    ));
                }

                Ok(Spanned::new(Pattern::Con(name, None), name_span))
            }

            // Literal patterns
            Some(TokenKind::Int(n)) => {
                let token = self.advance();
                Ok(Spanned::new(Pattern::Lit(Literal::Int(n)), token.span))
            }

            Some(TokenKind::Float(f)) => {
                let token = self.advance();
                Ok(Spanned::new(Pattern::Lit(Literal::Float(f)), token.span))
            }

            Some(TokenKind::String(s)) => {
                let token = self.advance();
                Ok(Spanned::new(Pattern::Lit(Literal::String(s)), token.span))
            }

            Some(TokenKind::Char(c)) => {
                let token = self.advance();
                Ok(Spanned::new(Pattern::Lit(Literal::Char(c)), token.span))
            }

            Some(TokenKind::True) => {
                let token = self.advance();
                Ok(Spanned::new(Pattern::Lit(Literal::Bool(true)), token.span))
            }

            Some(TokenKind::False) => {
                let token = self.advance();
                Ok(Spanned::new(Pattern::Lit(Literal::Bool(false)), token.span))
            }

            // Parenthesized pattern or tuple
            Some(TokenKind::LParen) => self.parse_paren_pattern(),

            // List pattern
            Some(TokenKind::LBracket) => self.parse_list_pattern(),

            // Record pattern
            Some(TokenKind::LBrace) => self.parse_record_pattern(),

            Some(_) => {
                let token = self.peek().unwrap();
                Err(ParseError::InvalidPattern { span: token.span })
            }

            None => Err(ParseError::UnexpectedEof {
                expected: "pattern".to_string(),
            }),
        }
    }

    fn parse_paren_pattern(&mut self) -> Result<Spanned<Pattern>, ParseError> {
        let start = self.expect(TokenKind::LParen)?.span;

        // Unit pattern: ()
        if self.eat(TokenKind::RParen) {
            return Ok(Spanned::new(Pattern::Tuple(vec![]), start.merge(Span::new(start.end, start.end + 1))));
        }

        let first = self.parse_pattern()?;

        // Tuple pattern: (p1, p2, ...)
        if self.eat(TokenKind::Comma) {
            let mut elements = vec![first];
            elements.push(self.parse_pattern()?);

            while self.eat(TokenKind::Comma) {
                elements.push(self.parse_pattern()?);
            }

            let end = self.expect(TokenKind::RParen)?.span;
            return Ok(Spanned::new(Pattern::Tuple(elements), start.merge(end)));
        }

        // Parenthesized pattern
        let end = self.expect(TokenKind::RParen)?.span;
        Ok(Spanned::new(first.value, start.merge(end)))
    }

    fn parse_list_pattern(&mut self) -> Result<Spanned<Pattern>, ParseError> {
        let start = self.expect(TokenKind::LBracket)?.span;

        // Empty list: []
        if self.eat(TokenKind::RBracket) {
            return Ok(Spanned::new(Pattern::List(vec![]), start.merge(Span::new(start.end, start.end + 1))));
        }

        let mut elements = vec![self.parse_pattern()?];

        while self.eat(TokenKind::Comma) {
            elements.push(self.parse_pattern()?);
        }

        let end = self.expect(TokenKind::RBracket)?.span;
        Ok(Spanned::new(Pattern::List(elements), start.merge(end)))
    }

    fn parse_record_pattern(&mut self) -> Result<Spanned<Pattern>, ParseError> {
        let start = self.expect(TokenKind::LBrace)?.span;

        // Empty record: {}
        if self.eat(TokenKind::RBrace) {
            return Ok(Spanned::new(Pattern::Record(vec![]), start.merge(Span::new(start.end, start.end + 1))));
        }

        let mut fields = Vec::new();
        fields.push(self.parse_record_pattern_field()?);

        while self.eat(TokenKind::Comma) {
            fields.push(self.parse_record_pattern_field()?);
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        Ok(Spanned::new(Pattern::Record(fields), start.merge(end)))
    }

    fn parse_record_pattern_field(&mut self) -> Result<(Spanned<String>, Spanned<Pattern>), ParseError> {
        let name = self.expect_ident()?;

        // Punning: { x } means { x = x }
        if !self.eat(TokenKind::Eq) {
            let pattern = Spanned::new(Pattern::Var(name.value.clone()), name.span);
            return Ok((name, pattern));
        }

        let pattern = self.parse_pattern()?;
        Ok((name, pattern))
    }

    /// Convert an expression to a pattern (for do-notation bind)
    fn expr_to_pattern(&self, expr: &Spanned<Expr>) -> Result<Spanned<Pattern>, ParseError> {
        let pattern = match &expr.value {
            Expr::Var(name) => Pattern::Var(name.clone()),
            Expr::Con(name) => Pattern::Con(name.clone(), None),
            Expr::Lit(lit) => Pattern::Lit(lit.clone()),
            Expr::Unit => Pattern::Tuple(vec![]),
            Expr::Tuple(exprs) => {
                let mut patterns = Vec::new();
                for e in exprs {
                    patterns.push(self.expr_to_pattern(e)?);
                }
                Pattern::Tuple(patterns)
            }
            Expr::List(exprs) => {
                let mut patterns = Vec::new();
                for e in exprs {
                    patterns.push(self.expr_to_pattern(e)?);
                }
                Pattern::List(patterns)
            }
            _ => return Err(ParseError::InvalidPattern { span: expr.span }),
        };

        Ok(Spanned::new(pattern, expr.span))
    }

    // ========== Type Expression Parsing ==========

    fn parse_type_expr(&mut self) -> Result<Spanned<TypeExpr>, ParseError> {
        let lhs = self.parse_type_app()?;

        // Function type: ty1 -> ty2
        if self.eat(TokenKind::Arrow) {
            let rhs = self.parse_type_expr()?;
            let span = lhs.span.merge(rhs.span);
            return Ok(Spanned::new(
                TypeExpr::Arrow(Box::new(lhs), Box::new(rhs)),
                span,
            ));
        }

        // Tuple type: ty1 * ty2 * ...
        if self.eat(TokenKind::Star) {
            let mut elements = vec![lhs];
            elements.push(self.parse_type_app()?);

            while self.eat(TokenKind::Star) {
                elements.push(self.parse_type_app()?);
            }

            let span = elements.first().unwrap().span.merge(elements.last().unwrap().span);
            return Ok(Spanned::new(TypeExpr::Tuple(elements), span));
        }

        Ok(lhs)
    }

    fn parse_type_app(&mut self) -> Result<Spanned<TypeExpr>, ParseError> {
        let base = self.parse_atomic_type()?;

        // Type application: 'a list, int option, etc.
        if let Some(TokenKind::Ident(_)) = self.peek_kind() {
            let con = self.expect_ident()?;
            let span = base.span.merge(con.span);
            return Ok(Spanned::new(
                TypeExpr::App(
                    Box::new(Spanned::new(TypeExpr::Con(con.value), con.span)),
                    vec![base],
                ),
                span,
            ));
        }

        Ok(base)
    }

    fn parse_atomic_type(&mut self) -> Result<Spanned<TypeExpr>, ParseError> {
        match self.peek_kind().cloned() {
            // Type variable
            Some(TokenKind::TyVar(name)) => {
                let token = self.advance();
                Ok(Spanned::new(TypeExpr::Var(name), token.span))
            }

            // Type constructor
            Some(TokenKind::Ident(name)) => {
                let token = self.advance();
                Ok(Spanned::new(TypeExpr::Con(name), token.span))
            }

            // Parenthesized type or tuple type args
            Some(TokenKind::LParen) => self.parse_paren_type(),

            // Record type
            Some(TokenKind::LBrace) => self.parse_record_type(),

            Some(_) => {
                let token = self.peek().unwrap();
                Err(ParseError::UnexpectedToken {
                    expected: "type".to_string(),
                    found: token.kind.name().to_string(),
                    span: token.span,
                })
            }

            None => Err(ParseError::UnexpectedEof {
                expected: "type".to_string(),
            }),
        }
    }

    fn parse_paren_type(&mut self) -> Result<Spanned<TypeExpr>, ParseError> {
        let start = self.expect(TokenKind::LParen)?.span;

        // Unit type: ()
        if self.eat(TokenKind::RParen) {
            return Ok(Spanned::new(TypeExpr::Con("unit".to_string()), start.merge(Span::new(start.end, start.end + 1))));
        }

        let first = self.parse_type_expr()?;

        // Multiple type args: (ty1, ty2) con
        if self.eat(TokenKind::Comma) {
            let mut args = vec![first];
            args.push(self.parse_type_expr()?);

            while self.eat(TokenKind::Comma) {
                args.push(self.parse_type_expr()?);
            }

            self.expect(TokenKind::RParen)?;

            // Expect constructor name after multi-arg type
            let con = self.expect_ident()?;
            let span = start.merge(con.span);

            return Ok(Spanned::new(
                TypeExpr::App(
                    Box::new(Spanned::new(TypeExpr::Con(con.value), con.span)),
                    args,
                ),
                span,
            ));
        }

        // Parenthesized type
        let end = self.expect(TokenKind::RParen)?.span;
        Ok(Spanned::new(
            TypeExpr::Paren(Box::new(first)),
            start.merge(end),
        ))
    }

    fn parse_record_type(&mut self) -> Result<Spanned<TypeExpr>, ParseError> {
        let start = self.expect(TokenKind::LBrace)?.span;

        // Empty record type: {}
        if self.eat(TokenKind::RBrace) {
            return Ok(Spanned::new(TypeExpr::Record(vec![]), start.merge(Span::new(start.end, start.end + 1))));
        }

        let mut fields = Vec::new();
        fields.push(self.parse_record_type_field()?);

        while self.eat(TokenKind::Comma) {
            fields.push(self.parse_record_type_field()?);
        }

        let end = self.expect(TokenKind::RBrace)?.span;
        Ok(Spanned::new(TypeExpr::Record(fields), start.merge(end)))
    }

    fn parse_record_type_field(&mut self) -> Result<(Spanned<String>, Spanned<TypeExpr>), ParseError> {
        let name = self.expect_ident()?;
        self.expect(TokenKind::Colon)?;
        let ty = self.parse_type_expr()?;
        Ok((name, ty))
    }

    // ========== Helper Methods ==========

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        self.peek().map(|t| &t.kind)
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        self.pos += 1;
        token
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.peek_kind() == Some(&kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Result<Token, ParseError> {
        match self.peek() {
            Some(token) if token.kind == kind => Ok(self.advance()),
            Some(token) => Err(ParseError::UnexpectedToken {
                expected: kind.name().to_string(),
                found: token.kind.name().to_string(),
                span: token.span,
            }),
            None => Err(ParseError::UnexpectedEof {
                expected: kind.name().to_string(),
            }),
        }
    }

    fn expect_ident(&mut self) -> Result<Spanned<String>, ParseError> {
        match self.peek_kind().cloned() {
            Some(TokenKind::Ident(name)) => {
                let token = self.advance();
                Ok(Spanned::new(name, token.span))
            }
            Some(_) => {
                let token = self.peek().unwrap();
                Err(ParseError::UnexpectedToken {
                    expected: "identifier".to_string(),
                    found: token.kind.name().to_string(),
                    span: token.span,
                })
            }
            None => Err(ParseError::UnexpectedEof {
                expected: "identifier".to_string(),
            }),
        }
    }

    fn expect_upper_ident(&mut self) -> Result<Spanned<String>, ParseError> {
        match self.peek_kind().cloned() {
            Some(TokenKind::UpperIdent(name)) => {
                let token = self.advance();
                Ok(Spanned::new(name, token.span))
            }
            Some(_) => {
                let token = self.peek().unwrap();
                Err(ParseError::UnexpectedToken {
                    expected: "constructor".to_string(),
                    found: token.kind.name().to_string(),
                    span: token.span,
                })
            }
            None => Err(ParseError::UnexpectedEof {
                expected: "constructor".to_string(),
            }),
        }
    }

    fn expect_any_ident(&mut self) -> Result<Spanned<String>, ParseError> {
        match self.peek_kind().cloned() {
            Some(TokenKind::Ident(name)) | Some(TokenKind::UpperIdent(name)) => {
                let token = self.advance();
                Ok(Spanned::new(name, token.span))
            }
            Some(_) => {
                let token = self.peek().unwrap();
                Err(ParseError::UnexpectedToken {
                    expected: "identifier".to_string(),
                    found: token.kind.name().to_string(),
                    span: token.span,
                })
            }
            None => Err(ParseError::UnexpectedEof {
                expected: "identifier".to_string(),
            }),
        }
    }
}

// ========== Operator Binding Powers ==========

/// Prefix binding power (for unary operators)
fn prefix_binding_power(op: &TokenKind) -> ((), u8) {
    match op {
        TokenKind::Minus => ((), 23), // Unary minus
        TokenKind::Bang => ((), 23),  // Logical not
        _ => ((), 0),
    }
}

/// Infix binding power (for binary operators)
/// Returns (left_bp, right_bp) where higher binds tighter.
fn infix_binding_power(op: &TokenKind) -> Option<(u8, u8)> {
    let bp = match op {
        // Type annotation (lowest precedence)
        TokenKind::Colon => (2, 1),

        // Logical or
        TokenKind::OrElse => (3, 4),

        // Logical and
        TokenKind::AndAlso => (5, 6),

        // Comparison
        TokenKind::Eq | TokenKind::Neq | TokenKind::Lt | TokenKind::Le | TokenKind::Gt | TokenKind::Ge => (7, 8),

        // Cons (right-associative)
        TokenKind::ColonColon => (10, 9),

        // Append (right-associative)
        TokenKind::At => (10, 9),

        // String/list concatenation (Haskell-style ++)
        TokenKind::PlusPlus => (11, 12),

        // Addition and subtraction
        TokenKind::Plus | TokenKind::Minus => (13, 14),

        // Multiplication, division, modulo
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => (15, 16),

        // Pipe (left-associative, low precedence)
        TokenKind::Pipe => (1, 2),

        _ => return None,
    };

    Some(bp)
}

/// Postfix binding power (for function application)
fn postfix_binding_power(op: &TokenKind) -> Option<(u8, ())> {
    match op {
        // Function application (very high precedence)
        k if can_start_atom(k) => Some((21, ())),
        // Field access
        TokenKind::Dot => Some((25, ())),
        _ => None,
    }
}

/// Check if a token can start an atomic expression
fn can_start_atom(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Int(_)
            | TokenKind::Float(_)
            | TokenKind::String(_)
            | TokenKind::Char(_)
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Ident(_)
            | TokenKind::UpperIdent(_)
            | TokenKind::LParen
            | TokenKind::LBracket
            | TokenKind::LBrace
            | TokenKind::Dollar
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to parse an expression from source
    fn parse_expr(source: &str) -> Result<Expr, ParseError> {
        let mut parser = Parser::new(source)?;
        Ok(parser.parse_expr()?.value)
    }

    /// Helper to parse a pattern from source
    fn parse_pattern(source: &str) -> Result<Pattern, ParseError> {
        let mut parser = Parser::new(source)?;
        Ok(parser.parse_pattern()?.value)
    }

    /// Helper to parse a type from source
    fn parse_type(source: &str) -> Result<TypeExpr, ParseError> {
        let mut parser = Parser::new(source)?;
        Ok(parser.parse_type()?.value)
    }

    /// Helper to parse a program from source
    fn parse_program(source: &str) -> Result<Program, ParseError> {
        let mut parser = Parser::new(source)?;
        parser.parse_program()
    }

    // ========== Literal Tests ==========

    #[test]
    fn parse_int_literal() {
        assert!(matches!(parse_expr("42"), Ok(Expr::Lit(Literal::Int(42)))));
    }

    #[test]
    fn parse_float_literal() {
        let expr = parse_expr("3.14").unwrap();
        assert!(matches!(expr, Expr::Lit(Literal::Float(f)) if (f - 3.14).abs() < 0.001));
    }

    #[test]
    fn parse_string_literal() {
        assert!(matches!(
            parse_expr(r#""hello""#),
            Ok(Expr::Lit(Literal::String(s))) if s == "hello"
        ));
    }

    #[test]
    fn parse_char_literal() {
        assert!(matches!(
            parse_expr(r#"#"a""#),
            Ok(Expr::Lit(Literal::Char('a')))
        ));
    }

    #[test]
    fn parse_bool_literals() {
        assert!(matches!(parse_expr("true"), Ok(Expr::Lit(Literal::Bool(true)))));
        assert!(matches!(parse_expr("false"), Ok(Expr::Lit(Literal::Bool(false)))));
    }

    // ========== Variable Tests ==========

    #[test]
    fn parse_variable() {
        assert!(matches!(
            parse_expr("foo"),
            Ok(Expr::Var(name)) if name == "foo"
        ));
    }

    #[test]
    fn parse_constructor() {
        assert!(matches!(
            parse_expr("Some"),
            Ok(Expr::Con(name)) if name == "Some"
        ));
    }

    // ========== Operator Tests ==========

    #[test]
    fn parse_addition() {
        let expr = parse_expr("1 + 2").unwrap();
        assert!(matches!(
            expr,
            Expr::BinOp(BinOp::Add, _, _)
        ));
    }

    #[test]
    fn parse_subtraction() {
        let expr = parse_expr("5 - 3").unwrap();
        assert!(matches!(
            expr,
            Expr::BinOp(BinOp::Sub, _, _)
        ));
    }

    #[test]
    fn parse_multiplication() {
        let expr = parse_expr("2 * 3").unwrap();
        assert!(matches!(
            expr,
            Expr::BinOp(BinOp::Mul, _, _)
        ));
    }

    #[test]
    fn parse_division() {
        let expr = parse_expr("10 / 2").unwrap();
        assert!(matches!(
            expr,
            Expr::BinOp(BinOp::Div, _, _)
        ));
    }

    #[test]
    fn parse_precedence_mul_add() {
        // 1 + 2 * 3 should be 1 + (2 * 3)
        let expr = parse_expr("1 + 2 * 3").unwrap();
        if let Expr::BinOp(BinOp::Add, lhs, rhs) = expr {
            assert!(matches!(lhs.value, Expr::Lit(Literal::Int(1))));
            assert!(matches!(rhs.value, Expr::BinOp(BinOp::Mul, _, _)));
        } else {
            panic!("Expected Add at top level");
        }
    }

    #[test]
    fn parse_left_associativity() {
        // 1 - 2 - 3 should be (1 - 2) - 3
        let expr = parse_expr("1 - 2 - 3").unwrap();
        if let Expr::BinOp(BinOp::Sub, lhs, rhs) = expr {
            assert!(matches!(lhs.value, Expr::BinOp(BinOp::Sub, _, _)));
            assert!(matches!(rhs.value, Expr::Lit(Literal::Int(3))));
        } else {
            panic!("Expected Sub at top level");
        }
    }

    #[test]
    fn parse_cons_right_associativity() {
        // 1 :: 2 :: xs should be 1 :: (2 :: xs)
        let expr = parse_expr("1 :: 2 :: xs").unwrap();
        if let Expr::BinOp(BinOp::Cons, lhs, rhs) = expr {
            assert!(matches!(lhs.value, Expr::Lit(Literal::Int(1))));
            assert!(matches!(rhs.value, Expr::BinOp(BinOp::Cons, _, _)));
        } else {
            panic!("Expected Cons at top level");
        }
    }

    #[test]
    fn parse_comparison() {
        let expr = parse_expr("x < y").unwrap();
        assert!(matches!(expr, Expr::BinOp(BinOp::Lt, _, _)));

        let expr = parse_expr("a = b").unwrap();
        assert!(matches!(expr, Expr::BinOp(BinOp::Eq, _, _)));
    }

    #[test]
    fn parse_logical() {
        let expr = parse_expr("a && b").unwrap();
        assert!(matches!(expr, Expr::BinOp(BinOp::And, _, _)));

        let expr = parse_expr("a || b").unwrap();
        assert!(matches!(expr, Expr::BinOp(BinOp::Or, _, _)));
    }

    #[test]
    fn parse_unary_minus() {
        let expr = parse_expr("-42").unwrap();
        assert!(matches!(expr, Expr::UnOp(UnOp::Neg, _)));
    }

    #[test]
    fn parse_unary_not() {
        let expr = parse_expr("!flag").unwrap();
        assert!(matches!(expr, Expr::UnOp(UnOp::Not, _)));
    }

    #[test]
    fn parse_parentheses() {
        // (1 + 2) * 3 should have Add inside
        let expr = parse_expr("(1 + 2) * 3").unwrap();
        if let Expr::BinOp(BinOp::Mul, lhs, _) = expr {
            assert!(matches!(lhs.value, Expr::BinOp(BinOp::Add, _, _)));
        } else {
            panic!("Expected Mul at top level");
        }
    }

    // ========== Tuple and List Tests ==========

    #[test]
    fn parse_unit() {
        let expr = parse_expr("()").unwrap();
        assert!(matches!(expr, Expr::Unit));
    }

    #[test]
    fn parse_tuple() {
        let expr = parse_expr("(1, 2, 3)").unwrap();
        if let Expr::Tuple(elements) = expr {
            assert_eq!(elements.len(), 3);
        } else {
            panic!("Expected Tuple");
        }
    }

    #[test]
    fn parse_empty_list() {
        let expr = parse_expr("[]").unwrap();
        if let Expr::List(elements) = expr {
            assert!(elements.is_empty());
        } else {
            panic!("Expected List");
        }
    }

    #[test]
    fn parse_list() {
        let expr = parse_expr("[1, 2, 3]").unwrap();
        if let Expr::List(elements) = expr {
            assert_eq!(elements.len(), 3);
        } else {
            panic!("Expected List");
        }
    }

    // ========== Lambda Tests ==========

    #[test]
    fn parse_lambda() {
        let expr = parse_expr("fn x => x + 1").unwrap();
        if let Expr::Lambda(params, _) = expr {
            assert_eq!(params.len(), 1);
        } else {
            panic!("Expected Lambda");
        }
    }

    #[test]
    fn parse_multi_param_lambda() {
        let expr = parse_expr("fn x y => x + y").unwrap();
        if let Expr::Lambda(params, _) = expr {
            assert_eq!(params.len(), 2);
        } else {
            panic!("Expected Lambda");
        }
    }

    // ========== Conditional Tests ==========

    #[test]
    fn parse_if_expr() {
        let expr = parse_expr("if true then 1 else 0").unwrap();
        assert!(matches!(expr, Expr::If(_, _, _)));
    }

    #[test]
    fn parse_nested_if() {
        let expr = parse_expr("if a then if b then 1 else 2 else 3").unwrap();
        if let Expr::If(_, then_branch, _) = expr {
            assert!(matches!(then_branch.value, Expr::If(_, _, _)));
        } else {
            panic!("Expected If");
        }
    }

    // ========== Match Tests ==========

    #[test]
    fn parse_match_expr() {
        let expr = parse_expr("match x with | Some y -> y | None -> 0").unwrap();
        if let Expr::Match(_, arms) = expr {
            assert_eq!(arms.len(), 2);
        } else {
            panic!("Expected Match");
        }
    }

    #[test]
    fn parse_case_expr() {
        let expr = parse_expr("case x of Some y -> y | None -> 0").unwrap();
        if let Expr::Match(_, arms) = expr {
            assert_eq!(arms.len(), 2);
        } else {
            panic!("Expected Match (from case)");
        }
    }

    // ========== Function Application Tests ==========

    #[test]
    fn parse_function_application() {
        let expr = parse_expr("f x").unwrap();
        assert!(matches!(expr, Expr::App(_, _)));
    }

    #[test]
    fn parse_curried_application() {
        let expr = parse_expr("f x y").unwrap();
        // Should be (f x) y
        if let Expr::App(lhs, _) = expr {
            assert!(matches!(lhs.value, Expr::App(_, _)));
        } else {
            panic!("Expected App");
        }
    }

    #[test]
    fn parse_constructor_application() {
        let expr = parse_expr("Some 42").unwrap();
        if let Expr::App(func, arg) = expr {
            assert!(matches!(func.value, Expr::Con(_)));
            assert!(matches!(arg.value, Expr::Lit(Literal::Int(42))));
        } else {
            panic!("Expected App");
        }
    }

    // ========== Pipe Tests ==========

    #[test]
    fn parse_pipe() {
        let expr = parse_expr("x |> f").unwrap();
        assert!(matches!(expr, Expr::Pipe(_, _)));
    }

    #[test]
    fn parse_pipe_chain() {
        let expr = parse_expr("x |> f |> g").unwrap();
        // Should be (x |> f) |> g
        if let Expr::Pipe(lhs, _) = expr {
            assert!(matches!(lhs.value, Expr::Pipe(_, _)));
        } else {
            panic!("Expected Pipe");
        }
    }

    // ========== Record Tests ==========

    #[test]
    fn parse_empty_record() {
        let expr = parse_expr("{}").unwrap();
        if let Expr::Record(fields) = expr {
            assert!(fields.is_empty());
        } else {
            panic!("Expected Record");
        }
    }

    #[test]
    fn parse_record() {
        let expr = parse_expr("{ x = 1, y = 2 }").unwrap();
        if let Expr::Record(fields) = expr {
            assert_eq!(fields.len(), 2);
        } else {
            panic!("Expected Record");
        }
    }

    #[test]
    fn parse_field_access() {
        let expr = parse_expr("r.x").unwrap();
        assert!(matches!(expr, Expr::Field(_, _)));
    }

    // ========== Environment Variable Tests ==========

    #[test]
    fn parse_env_var() {
        let expr = parse_expr("$HOME").unwrap();
        assert!(matches!(expr, Expr::EnvVar(name) if name == "HOME"));
    }

    // ========== Pattern Tests ==========

    #[test]
    fn parse_wildcard_pattern() {
        let pattern = parse_pattern("_").unwrap();
        assert!(matches!(pattern, Pattern::Wildcard));
    }

    #[test]
    fn parse_var_pattern() {
        let pattern = parse_pattern("x").unwrap();
        assert!(matches!(pattern, Pattern::Var(name) if name == "x"));
    }

    #[test]
    fn parse_literal_pattern() {
        let pattern = parse_pattern("42").unwrap();
        assert!(matches!(pattern, Pattern::Lit(Literal::Int(42))));
    }

    #[test]
    fn parse_constructor_pattern() {
        let pattern = parse_pattern("Some x").unwrap();
        if let Pattern::Con(name, Some(_)) = pattern {
            assert_eq!(name, "Some");
        } else {
            panic!("Expected Con pattern");
        }
    }

    #[test]
    fn parse_tuple_pattern() {
        let pattern = parse_pattern("(x, y, z)").unwrap();
        if let Pattern::Tuple(elements) = pattern {
            assert_eq!(elements.len(), 3);
        } else {
            panic!("Expected Tuple pattern");
        }
    }

    #[test]
    fn parse_list_pattern() {
        let pattern = parse_pattern("[x, y]").unwrap();
        if let Pattern::List(elements) = pattern {
            assert_eq!(elements.len(), 2);
        } else {
            panic!("Expected List pattern");
        }
    }

    #[test]
    fn parse_cons_pattern() {
        let pattern = parse_pattern("x :: xs").unwrap();
        assert!(matches!(pattern, Pattern::Cons(_, _)));
    }

    #[test]
    fn parse_or_pattern() {
        let pattern = parse_pattern("None | Some _").unwrap();
        assert!(matches!(pattern, Pattern::Or(_, _)));
    }

    // ========== Type Tests ==========

    #[test]
    fn parse_type_var() {
        let ty = parse_type("'a").unwrap();
        assert!(matches!(ty, TypeExpr::Var(name) if name == "a"));
    }

    #[test]
    fn parse_type_con() {
        let ty = parse_type("int").unwrap();
        assert!(matches!(ty, TypeExpr::Con(name) if name == "int"));
    }

    #[test]
    fn parse_function_type() {
        let ty = parse_type("int -> string").unwrap();
        assert!(matches!(ty, TypeExpr::Arrow(_, _)));
    }

    #[test]
    fn parse_function_type_right_assoc() {
        // int -> int -> int should be int -> (int -> int)
        let ty = parse_type("int -> int -> int").unwrap();
        if let TypeExpr::Arrow(_, rhs) = ty {
            assert!(matches!(rhs.value, TypeExpr::Arrow(_, _)));
        } else {
            panic!("Expected Arrow");
        }
    }

    #[test]
    fn parse_type_application() {
        let ty = parse_type("'a list").unwrap();
        assert!(matches!(ty, TypeExpr::App(_, _)));
    }

    #[test]
    fn parse_tuple_type() {
        let ty = parse_type("int * string * bool").unwrap();
        if let TypeExpr::Tuple(elements) = ty {
            assert_eq!(elements.len(), 3);
        } else {
            panic!("Expected Tuple type");
        }
    }

    #[test]
    fn parse_record_type() {
        let ty = parse_type("{ x: int, y: string }").unwrap();
        if let TypeExpr::Record(fields) = ty {
            assert_eq!(fields.len(), 2);
        } else {
            panic!("Expected Record type");
        }
    }

    // ========== Declaration Tests ==========

    #[test]
    fn parse_let_decl() {
        let program = parse_program("let x = 42").unwrap();
        assert_eq!(program.decls.len(), 1);
        assert!(matches!(program.decls[0].value, Decl::Val(_)));
    }

    #[test]
    fn parse_fun_decl() {
        let program = parse_program("fun add x y = x + y").unwrap();
        assert_eq!(program.decls.len(), 1);
        if let Decl::Fun(fun) = &program.decls[0].value {
            assert_eq!(fun.name.value, "add");
            assert_eq!(fun.clauses.len(), 1);
            assert_eq!(fun.clauses[0].params.len(), 2);
        } else {
            panic!("Expected Fun");
        }
    }

    #[test]
    fn parse_type_decl() {
        let program = parse_program("type point = { x: int, y: int }").unwrap();
        assert_eq!(program.decls.len(), 1);
        assert!(matches!(program.decls[0].value, Decl::Type(_)));
    }

    #[test]
    fn parse_datatype_decl() {
        let program = parse_program("datatype 'a option = None | Some of 'a").unwrap();
        assert_eq!(program.decls.len(), 1);
        if let Decl::Datatype(dt) = &program.decls[0].value {
            assert_eq!(dt.name.value, "option");
            assert_eq!(dt.params.len(), 1);
            assert_eq!(dt.constructors.len(), 2);
        } else {
            panic!("Expected Datatype");
        }
    }

    #[test]
    fn parse_trait_decl() {
        let source = r#"
            trait Show {
                fn show(self) -> string
            }
        "#;
        let program = parse_program(source).unwrap();
        assert_eq!(program.decls.len(), 1);
        if let Decl::Trait(tr) = &program.decls[0].value {
            assert_eq!(tr.name.value, "Show");
            assert_eq!(tr.methods.len(), 1);
            assert_eq!(tr.methods[0].name.value, "show");
            assert_eq!(tr.methods[0].params.len(), 1);
            assert_eq!(tr.methods[0].params[0].value, "self");
        } else {
            panic!("Expected Trait");
        }
    }

    #[test]
    fn parse_trait_multiple_methods() {
        let source = r#"
            trait Eq {
                fn eq(self, other) -> bool
                fn neq(self, other) -> bool
            }
        "#;
        let program = parse_program(source).unwrap();
        assert_eq!(program.decls.len(), 1);
        if let Decl::Trait(tr) = &program.decls[0].value {
            assert_eq!(tr.name.value, "Eq");
            assert_eq!(tr.methods.len(), 2);
            assert_eq!(tr.methods[0].name.value, "eq");
            assert_eq!(tr.methods[1].name.value, "neq");
        } else {
            panic!("Expected Trait");
        }
    }

    #[test]
    fn parse_impl_decl() {
        let source = r#"
            impl Show for int {
                fn show(self) = intToString self
            }
        "#;
        let program = parse_program(source).unwrap();
        assert_eq!(program.decls.len(), 1);
        if let Decl::Impl(im) = &program.decls[0].value {
            assert_eq!(im.trait_name.value, "Show");
            if let TypeExpr::Con(ty_name) = &im.for_type.value {
                assert_eq!(ty_name, "int");
            } else {
                panic!("Expected type constructor");
            }
            assert_eq!(im.methods.len(), 1);
            assert_eq!(im.methods[0].name.value, "show");
        } else {
            panic!("Expected Impl");
        }
    }

    #[test]
    fn parse_impl_with_return_type() {
        let source = r#"
            impl Show for float {
                fn show(self) -> string = floatToString self
            }
        "#;
        let program = parse_program(source).unwrap();
        assert_eq!(program.decls.len(), 1);
        if let Decl::Impl(im) = &program.decls[0].value {
            assert_eq!(im.trait_name.value, "Show");
            assert!(im.methods[0].return_ty.is_some());
        } else {
            panic!("Expected Impl");
        }
    }

    #[test]
    fn parse_trait_and_impl() {
        let source = r#"
            trait Show {
                fn show(self) -> string
            }
            impl Show for int {
                fn show(self) = intToString self
            }
            impl Show for bool {
                fn show(self) = if self then "true" else "false"
            }
        "#;
        let program = parse_program(source).unwrap();
        assert_eq!(program.decls.len(), 3);
        assert!(matches!(program.decls[0].value, Decl::Trait(_)));
        assert!(matches!(program.decls[1].value, Decl::Impl(_)));
        assert!(matches!(program.decls[2].value, Decl::Impl(_)));
    }

    #[test]
    fn parse_multiple_decls() {
        let source = r#"
            let x = 1
            let y = 2
            fun add a b = a + b
        "#;
        let program = parse_program(source).unwrap();
        assert_eq!(program.decls.len(), 3);
    }

    // ========== Complex Expression Tests ==========

    #[test]
    fn parse_nested_match() {
        let source = r#"
            match x with
            | Some (Some y) -> y
            | Some None -> 0
            | None -> 0
        "#;
        let expr = parse_expr(source).unwrap();
        if let Expr::Match(_, arms) = expr {
            assert_eq!(arms.len(), 3);
        } else {
            panic!("Expected Match");
        }
    }

    #[test]
    fn parse_shell_pipeline() {
        let expr = parse_expr(r#"input |> filter pred |> map f |> output"#).unwrap();
        // Should have 3 pipes
        let mut count = 0;
        let mut current = &expr;
        while let Expr::Pipe(lhs, _) = current {
            count += 1;
            current = &lhs.value;
        }
        assert_eq!(count, 3);
    }

    // ========== Command Expression Tests ==========

    #[test]
    fn parse_command_simple() {
        let expr = parse_expr("`ls -la`").unwrap();
        if let Expr::Command(parts) = expr {
            assert_eq!(parts.len(), 1);
            assert!(matches!(&parts[0], CommandPart::Literal(s) if s == "ls -la"));
        } else {
            panic!("Expected Command, got {:?}", expr);
        }
    }

    #[test]
    fn parse_command_with_interpolation() {
        let expr = parse_expr("`echo {x}`").unwrap();
        if let Expr::Command(parts) = expr {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[0], CommandPart::Literal(s) if s == "echo "));
            assert!(matches!(&parts[1], CommandPart::Interpolation(e) if matches!(e.value, Expr::Var(ref n) if n == "x")));
        } else {
            panic!("Expected Command, got {:?}", expr);
        }
    }

    #[test]
    fn parse_command_multiple_interpolations() {
        let expr = parse_expr("`{cmd} {arg1} {arg2}`").unwrap();
        if let Expr::Command(parts) = expr {
            assert_eq!(parts.len(), 5);
            assert!(matches!(&parts[0], CommandPart::Interpolation(_)));
            assert!(matches!(&parts[1], CommandPart::Literal(s) if s == " "));
            assert!(matches!(&parts[2], CommandPart::Interpolation(_)));
            assert!(matches!(&parts[3], CommandPart::Literal(s) if s == " "));
            assert!(matches!(&parts[4], CommandPart::Interpolation(_)));
        } else {
            panic!("Expected Command, got {:?}", expr);
        }
    }

    #[test]
    fn parse_command_empty() {
        let expr = parse_expr("``").unwrap();
        if let Expr::Command(parts) = expr {
            assert!(parts.is_empty());
        } else {
            panic!("Expected Command, got {:?}", expr);
        }
    }

    #[test]
    fn parse_command_pipeline() {
        // Commands piped together
        let expr = parse_expr("`ls` |> `grep foo`").unwrap();
        if let Expr::Pipe(lhs, rhs) = expr {
            assert!(matches!(lhs.value, Expr::Command(_)));
            assert!(matches!(rhs.value, Expr::Command(_)));
        } else {
            panic!("Expected Pipe, got {:?}", expr);
        }
    }
}
