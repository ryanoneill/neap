//! Abstract Syntax Tree definitions for Neap
//!
//! This module defines the AST nodes that represent parsed Neap programs.
//! The AST is produced by the parser and consumed by the type checker.

use super::span::{Span, Spanned};

/// A complete Neap program (a sequence of declarations).
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The declarations in this program
    pub decls: Vec<Spanned<Decl>>,
}

/// A top-level declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum Decl {
    /// Value binding: `let x = e`
    Val(ValDecl),

    /// Function binding: `fun f x = e`
    Fun(FunDecl),

    /// Type alias: `type t = ty`
    Type(TypeDecl),

    /// Datatype definition: `datatype 'a t = C1 | C2 of ty`
    Datatype(DatatypeDecl),

    /// Trait definition: `trait Show { fn show(self) -> string }`
    Trait(TraitDecl),

    /// Trait implementation: `impl Show for int { fn show(self) = ... }`
    Impl(ImplDecl),
}

/// A value declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ValDecl {
    /// Is this a recursive binding?
    pub rec: bool,
    /// The pattern being bound
    pub pattern: Spanned<Pattern>,
    /// Optional type annotation
    pub ty: Option<Spanned<TypeExpr>>,
    /// The expression being bound
    pub expr: Spanned<Expr>,
}

/// A function declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct FunDecl {
    /// The function name
    pub name: Spanned<String>,
    /// The function clauses (for pattern matching on arguments)
    pub clauses: Vec<FunClause>,
}

/// A single clause in a function definition.
#[derive(Debug, Clone, PartialEq)]
pub struct FunClause {
    /// The parameter patterns
    pub params: Vec<Spanned<Pattern>>,
    /// Optional result type annotation
    pub result_ty: Option<Spanned<TypeExpr>>,
    /// The function body
    pub body: Spanned<Expr>,
}

/// A type alias declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    /// Type parameters (e.g., 'a, 'b)
    pub params: Vec<Spanned<String>>,
    /// The type name
    pub name: Spanned<String>,
    /// The type being aliased
    pub ty: Spanned<TypeExpr>,
}

/// A datatype declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct DatatypeDecl {
    /// Type parameters
    pub params: Vec<Spanned<String>>,
    /// The datatype name
    pub name: Spanned<String>,
    /// The constructors
    pub constructors: Vec<Constructor>,
}

/// A datatype constructor.
#[derive(Debug, Clone, PartialEq)]
pub struct Constructor {
    /// The constructor name
    pub name: Spanned<String>,
    /// Optional argument type
    pub arg: Option<Spanned<TypeExpr>>,
}

/// A trait declaration.
///
/// Example: `trait Show { fn show(self) -> string }`
#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl {
    /// The trait name (e.g., "Show")
    pub name: Spanned<String>,
    /// The type parameter name (typically "self" but could be any name)
    pub type_param: Spanned<String>,
    /// The method signatures
    pub methods: Vec<MethodSig>,
}

/// A method signature in a trait declaration.
///
/// Example: `fn show(self) -> string`
#[derive(Debug, Clone, PartialEq)]
pub struct MethodSig {
    /// The method name
    pub name: Spanned<String>,
    /// Parameter names (first is typically "self")
    pub params: Vec<Spanned<String>>,
    /// The return type
    pub return_ty: Spanned<TypeExpr>,
}

/// A trait implementation declaration.
///
/// Example: `impl Show for int { fn show(self) = intToString self }`
#[derive(Debug, Clone, PartialEq)]
pub struct ImplDecl {
    /// The trait being implemented
    pub trait_name: Spanned<String>,
    /// The type the trait is being implemented for
    pub for_type: Spanned<TypeExpr>,
    /// The method implementations
    pub methods: Vec<MethodImpl>,
}

/// A method implementation in an impl block.
///
/// Example: `fn show(self) = intToString self`
#[derive(Debug, Clone, PartialEq)]
pub struct MethodImpl {
    /// The method name
    pub name: Spanned<String>,
    /// Parameter names
    pub params: Vec<Spanned<String>>,
    /// Optional return type annotation
    pub return_ty: Option<Spanned<TypeExpr>>,
    /// The method body
    pub body: Spanned<Expr>,
}

/// A part of a shell command (for interpolation support).
#[derive(Debug, Clone, PartialEq)]
pub enum CommandPart {
    /// Literal text in the command
    Literal(String),
    /// Interpolated expression: `{expr}`
    Interpolation(Box<Spanned<Expr>>),
}

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Literal value
    Lit(Literal),

    /// Variable reference
    Var(String),

    /// Constructor reference
    Con(String),

    /// Function application
    App(Box<Spanned<Expr>>, Box<Spanned<Expr>>),

    /// Lambda expression: `fn p => e`
    Lambda(Vec<Spanned<Pattern>>, Box<Spanned<Expr>>),

    /// Let binding: `let p = e1 in e2`
    Let {
        rec: bool,
        pattern: Box<Spanned<Pattern>>,
        ty: Option<Box<Spanned<TypeExpr>>>,
        value: Box<Spanned<Expr>>,
        body: Box<Spanned<Expr>>,
    },

    /// Conditional: `if e1 then e2 else e3`
    If(Box<Spanned<Expr>>, Box<Spanned<Expr>>, Box<Spanned<Expr>>),

    /// Pattern matching: `match e with | p1 -> e1 | p2 -> e2`
    Match(Box<Spanned<Expr>>, Vec<MatchArm>),

    /// Binary operation
    BinOp(BinOp, Box<Spanned<Expr>>, Box<Spanned<Expr>>),

    /// Unary operation
    UnOp(UnOp, Box<Spanned<Expr>>),

    /// Tuple: `(e1, e2, ..., en)`
    Tuple(Vec<Spanned<Expr>>),

    /// List: `[e1, e2, ..., en]`
    List(Vec<Spanned<Expr>>),

    /// Record: `{l1 = e1, l2 = e2, ...}`
    Record(Vec<(Spanned<String>, Spanned<Expr>)>),

    /// Record field access: `e.field`
    Field(Box<Spanned<Expr>>, Spanned<String>),

    /// Type annotation: `e : ty`
    Annot(Box<Spanned<Expr>>, Box<Spanned<TypeExpr>>),

    /// Do block (IO sequencing)
    Do(Vec<DoStmt>),

    /// Shell pipe: `e1 |> e2`
    Pipe(Box<Spanned<Expr>>, Box<Spanned<Expr>>),

    /// Shell redirect: `e > path` or `e >> path`
    Redirect {
        expr: Box<Spanned<Expr>>,
        target: Box<Spanned<Expr>>,
        append: bool,
    },

    /// Environment variable: `$VAR`
    EnvVar(String),

    /// Shell command: `` `ls -la` `` or `` `echo {x}` ``
    Command(Vec<CommandPart>),

    /// Unit value: `()`
    Unit,
}

/// A literal value.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// Integer literal
    Int(i64),
    /// Float literal
    Float(f64),
    /// String literal
    String(String),
    /// Character literal
    Char(char),
    /// Boolean literal
    Bool(bool),
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,

    // Comparison
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,

    // Logical
    And,
    Or,

    // String
    Concat,

    // List
    Cons,
    Append,
}

/// A unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// Logical negation
    Not,
    /// Arithmetic negation
    Neg,
}

/// A match arm.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    /// The pattern to match
    pub pattern: Spanned<Pattern>,
    /// Optional guard: `when e`
    pub guard: Option<Spanned<Expr>>,
    /// The body expression
    pub body: Spanned<Expr>,
}

/// A statement in a do block.
#[derive(Debug, Clone, PartialEq)]
pub enum DoStmt {
    /// Bind: `p <- e`
    Bind(Spanned<Pattern>, Spanned<Expr>),
    /// Let in do: `let p = e`
    Let(Spanned<Pattern>, Spanned<Expr>),
    /// Expression (last statement or ignored result)
    Expr(Spanned<Expr>),
}

/// A pattern for matching.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Wildcard: `_`
    Wildcard,

    /// Variable binding: `x`
    Var(String),

    /// Literal pattern
    Lit(Literal),

    /// Constructor pattern: `C` or `C p`
    Con(String, Option<Box<Spanned<Pattern>>>),

    /// Tuple pattern: `(p1, p2, ..., pn)`
    Tuple(Vec<Spanned<Pattern>>),

    /// List pattern: `[p1, p2, ..., pn]`
    List(Vec<Spanned<Pattern>>),

    /// Cons pattern: `p1 :: p2`
    Cons(Box<Spanned<Pattern>>, Box<Spanned<Pattern>>),

    /// Record pattern: `{l1 = p1, l2 = p2, ...}`
    Record(Vec<(Spanned<String>, Spanned<Pattern>)>),

    /// Type-annotated pattern: `p : ty`
    Annot(Box<Spanned<Pattern>>, Box<Spanned<TypeExpr>>),

    /// Or pattern: `p1 | p2`
    Or(Box<Spanned<Pattern>>, Box<Spanned<Pattern>>),

    /// As pattern: `p as x`
    As(Box<Spanned<Pattern>>, Spanned<String>),
}

/// A type expression.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// Type variable: `'a`
    Var(String),

    /// Type constructor: `int`, `string`, `list`
    Con(String),

    /// Type application: `'a list`, `(int, string) map`
    App(Box<Spanned<TypeExpr>>, Vec<Spanned<TypeExpr>>),

    /// Function type: `ty1 -> ty2`
    Arrow(Box<Spanned<TypeExpr>>, Box<Spanned<TypeExpr>>),

    /// Tuple type: `ty1 * ty2 * ... * tyn`
    Tuple(Vec<Spanned<TypeExpr>>),

    /// Record type: `{l1: ty1, l2: ty2, ...}`
    Record(Vec<(Spanned<String>, Spanned<TypeExpr>)>),

    /// Parenthesized type (for grouping)
    Paren(Box<Spanned<TypeExpr>>),
}

impl Program {
    /// Create a new empty program.
    #[must_use]
    pub fn new() -> Self {
        Self { decls: Vec::new() }
    }

    /// Create a program from a list of declarations.
    #[must_use]
    pub fn with_decls(decls: Vec<Spanned<Decl>>) -> Self {
        Self { decls }
    }
}

impl Default for Program {
    fn default() -> Self {
        Self::new()
    }
}

impl Expr {
    /// Create a variable expression.
    #[must_use]
    pub fn var(name: impl Into<String>) -> Self {
        Self::Var(name.into())
    }

    /// Create an integer literal expression.
    #[must_use]
    pub const fn int(n: i64) -> Self {
        Self::Lit(Literal::Int(n))
    }

    /// Create a string literal expression.
    #[must_use]
    pub fn string(s: impl Into<String>) -> Self {
        Self::Lit(Literal::String(s.into()))
    }

    /// Create a boolean literal expression.
    #[must_use]
    pub const fn bool(b: bool) -> Self {
        Self::Lit(Literal::Bool(b))
    }
}

impl Pattern {
    /// Create a variable pattern.
    #[must_use]
    pub fn var(name: impl Into<String>) -> Self {
        Self::Var(name.into())
    }
}

impl Span {
    /// Create a dummy span for testing.
    #[must_use]
    pub const fn dummy() -> Self {
        Self::new(0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_program() {
        let program = Program::new();
        assert!(program.decls.is_empty());
    }

    #[test]
    fn expr_constructors() {
        let var = Expr::var("x");
        assert!(matches!(var, Expr::Var(name) if name == "x"));

        let int = Expr::int(42);
        assert!(matches!(int, Expr::Lit(Literal::Int(42))));

        let s = Expr::string("hello");
        assert!(matches!(s, Expr::Lit(Literal::String(ref val)) if val == "hello"));

        let b = Expr::bool(true);
        assert!(matches!(b, Expr::Lit(Literal::Bool(true))));
    }

    #[test]
    fn pattern_constructors() {
        let var = Pattern::var("x");
        assert!(matches!(var, Pattern::Var(name) if name == "x"));
    }
}
