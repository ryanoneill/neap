//! Core IR types for Neap
//!
//! The IR uses A-Normal Form (ANF) where all intermediate values are
//! explicitly bound to variables. This makes code generation straightforward.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::types::Type;

/// A unique variable identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VarId(u64);

static VAR_COUNTER: AtomicU64 = AtomicU64::new(0);

impl VarId {
    /// Create a fresh variable ID.
    #[must_use]
    pub fn fresh() -> Self {
        Self(VAR_COUNTER.fetch_add(1, Ordering::SeqCst))
    }

    /// Create a variable ID from a raw value (for testing).
    #[must_use]
    pub const fn from_raw(id: u64) -> Self {
        Self(id)
    }

    /// Get the raw ID value.
    #[must_use]
    pub const fn raw(&self) -> u64 {
        self.0
    }

    /// Reset the counter (for testing only).
    #[cfg(test)]
    pub fn reset_counter() {
        VAR_COUNTER.store(0, Ordering::SeqCst);
    }
}

impl fmt::Display for VarId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "_{}", self.0)
    }
}

/// A complete IR program.
#[derive(Debug, Clone)]
pub struct IRProgram {
    /// Top-level declarations
    pub decls: Vec<IRDecl>,
}

impl IRProgram {
    /// Create a new IR program.
    #[must_use]
    pub fn new(decls: Vec<IRDecl>) -> Self {
        Self { decls }
    }
}

/// A top-level declaration in the IR.
#[derive(Debug, Clone)]
pub enum IRDecl {
    /// Value binding: name, type, expression
    Val {
        name: String,
        ty: Type,
        value: IRExpr,
    },

    /// Function binding
    Fun {
        name: String,
        ty: Type,
        params: Vec<(VarId, Type)>,
        body: IRExpr,
    },

    /// Data type definition (for runtime representation)
    Data {
        name: String,
        params: Vec<String>,
        constructors: Vec<(String, Option<Type>)>,
    },
}

/// An IR expression in A-Normal Form.
///
/// In ANF, all intermediate computations are bound to variables.
/// Complex expressions are broken down into sequences of let bindings.
#[derive(Debug, Clone)]
pub enum IRExpr {
    /// Literal value
    Lit(IRLiteral, Type),

    /// Variable reference
    Var(VarId, Type),

    /// Named variable reference (for top-level bindings)
    Global(String, Type),

    /// Let binding: let var = value in body
    Let {
        var: VarId,
        ty: Type,
        value: Box<IRExpr>,
        body: Box<IRExpr>,
    },

    /// Recursive let binding
    LetRec {
        bindings: Vec<Binding>,
        body: Box<IRExpr>,
    },

    /// Lambda: fn (param: ty) -> body
    Lambda {
        param: VarId,
        param_ty: Type,
        body: Box<IRExpr>,
        result_ty: Type,
    },

    /// Function application (must be a value applied to a value in ANF)
    App {
        func: Box<IRExpr>,
        arg: Box<IRExpr>,
        result_ty: Type,
    },

    /// Conditional
    If {
        cond: Box<IRExpr>,
        then_branch: Box<IRExpr>,
        else_branch: Box<IRExpr>,
        ty: Type,
    },

    /// Pattern match
    Match {
        scrutinee: Box<IRExpr>,
        arms: Vec<(IRPattern, IRExpr)>,
        ty: Type,
    },

    /// Primitive operation
    Prim {
        op: Primitive,
        args: Vec<IRExpr>,
        ty: Type,
    },

    /// Data constructor application
    Construct {
        ctor: String,
        arg: Option<Box<IRExpr>>,
        ty: Type,
    },

    /// Tuple construction
    Tuple {
        elems: Vec<IRExpr>,
        ty: Type,
    },

    /// Tuple projection
    TupleProj {
        tuple: Box<IRExpr>,
        index: usize,
        ty: Type,
    },

    /// Record construction
    Record {
        fields: Vec<(String, IRExpr)>,
        ty: Type,
    },

    /// Record field access
    Field {
        record: Box<IRExpr>,
        field: String,
        ty: Type,
    },

    /// Unit value
    Unit,
}

impl IRExpr {
    /// Get the type of this expression.
    #[must_use]
    pub fn ty(&self) -> Type {
        match self {
            Self::Lit(_, ty) | Self::Var(_, ty) | Self::Global(_, ty) => ty.clone(),
            Self::Let { body, .. } | Self::LetRec { body, .. } => body.ty(),
            Self::Lambda {
                param_ty,
                result_ty,
                ..
            } => Type::arrow(param_ty.clone(), result_ty.clone()),
            Self::App { result_ty, .. }
            | Self::If { ty: result_ty, .. }
            | Self::Match { ty: result_ty, .. }
            | Self::Prim { ty: result_ty, .. }
            | Self::Construct { ty: result_ty, .. }
            | Self::Tuple { ty: result_ty, .. }
            | Self::TupleProj { ty: result_ty, .. }
            | Self::Record { ty: result_ty, .. }
            | Self::Field { ty: result_ty, .. } => result_ty.clone(),
            Self::Unit => Type::unit(),
        }
    }

    /// Check if this expression is a value (no computation needed).
    #[must_use]
    pub fn is_value(&self) -> bool {
        matches!(
            self,
            Self::Lit(..) | Self::Var(..) | Self::Global(..) | Self::Lambda { .. } | Self::Unit
        )
    }
}

/// A binding in a letrec.
#[derive(Debug, Clone)]
pub struct Binding {
    pub var: VarId,
    pub ty: Type,
    pub value: IRExpr,
}

/// A literal value in the IR.
#[derive(Debug, Clone, PartialEq)]
pub enum IRLiteral {
    Int(i64),
    Float(f64),
    String(String),
    Char(char),
    Bool(bool),
}

impl fmt::Display for IRLiteral {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{n}"),
            Self::Float(n) => write!(f, "{n}"),
            Self::String(s) => write!(f, "{s:?}"),
            Self::Char(c) => write!(f, "'{c}'"),
            Self::Bool(b) => write!(f, "{b}"),
        }
    }
}

/// An IR pattern for match expressions.
#[derive(Debug, Clone)]
pub enum IRPattern {
    /// Wildcard: matches anything
    Wildcard,

    /// Variable binding
    Var(VarId, Type),

    /// Literal pattern
    Lit(IRLiteral),

    /// Constructor pattern
    Con {
        ctor: String,
        arg: Option<Box<IRPattern>>,
    },

    /// Tuple pattern
    Tuple(Vec<IRPattern>),

    /// Record pattern
    Record(Vec<(String, IRPattern)>),
}

/// Primitive operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primitive {
    // Integer arithmetic
    AddInt,
    SubInt,
    MulInt,
    DivInt,
    ModInt,
    NegInt,

    // Float arithmetic
    AddFloat,
    SubFloat,
    MulFloat,
    DivFloat,
    NegFloat,

    // Comparison (polymorphic in AST, specialized in IR)
    EqInt,
    NeqInt,
    LtInt,
    LeInt,
    GtInt,
    GeInt,

    EqFloat,
    NeqFloat,
    LtFloat,
    LeFloat,
    GtFloat,
    GeFloat,

    EqString,
    NeqString,
    LtString,
    LeString,
    GtString,
    GeString,

    EqBool,
    NeqBool,

    EqChar,
    NeqChar,
    LtChar,
    LeChar,
    GtChar,
    GeChar,

    // Logical
    Not,
    And,
    Or,

    // String
    Concat,

    // List operations
    Cons,
    Append,

    // String operations
    StringLength,
    Substring,
    CharAt,

    // Conversions
    IntToFloat,
    FloatToInt,
    IntToString,
    FloatToString,
    CharToString,
    CharToInt,
    IntToChar,

    // List operations
    ListLength,

    // IO
    Print,
    PrintNoNewline,
    ReadLine,
    ReadFile,
    WriteFile,
    GetEnv,

    // Assertions
    Assert,
    Panic,
}

impl Primitive {
    /// Get the name of this primitive.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::AddInt => "__add_int",
            Self::SubInt => "__sub_int",
            Self::MulInt => "__mul_int",
            Self::DivInt => "__div_int",
            Self::ModInt => "__mod_int",
            Self::NegInt => "__neg_int",
            Self::AddFloat => "__add_float",
            Self::SubFloat => "__sub_float",
            Self::MulFloat => "__mul_float",
            Self::DivFloat => "__div_float",
            Self::NegFloat => "__neg_float",
            Self::EqInt => "__eq_int",
            Self::NeqInt => "__neq_int",
            Self::LtInt => "__lt_int",
            Self::LeInt => "__le_int",
            Self::GtInt => "__gt_int",
            Self::GeInt => "__ge_int",
            Self::EqFloat => "__eq_float",
            Self::NeqFloat => "__neq_float",
            Self::LtFloat => "__lt_float",
            Self::LeFloat => "__le_float",
            Self::GtFloat => "__gt_float",
            Self::GeFloat => "__ge_float",
            Self::EqString => "__eq_string",
            Self::NeqString => "__neq_string",
            Self::LtString => "__lt_string",
            Self::LeString => "__le_string",
            Self::GtString => "__gt_string",
            Self::GeString => "__ge_string",
            Self::EqBool => "__eq_bool",
            Self::NeqBool => "__neq_bool",
            Self::EqChar => "__eq_char",
            Self::NeqChar => "__neq_char",
            Self::LtChar => "__lt_char",
            Self::LeChar => "__le_char",
            Self::GtChar => "__gt_char",
            Self::GeChar => "__ge_char",
            Self::Not => "__not",
            Self::And => "__and",
            Self::Or => "__or",
            Self::Concat => "__concat",
            Self::Cons => "__cons",
            Self::Append => "__append",
            Self::StringLength => "__string_length",
            Self::Substring => "__substring",
            Self::CharAt => "__char_at",
            Self::IntToFloat => "__int_to_float",
            Self::FloatToInt => "__float_to_int",
            Self::IntToString => "__int_to_string",
            Self::FloatToString => "__float_to_string",
            Self::CharToString => "__char_to_string",
            Self::CharToInt => "__char_to_int",
            Self::IntToChar => "__int_to_char",
            Self::ListLength => "__list_length",
            Self::Print => "print",
            Self::PrintNoNewline => "__print_no_newline",
            Self::ReadLine => "readLine",
            Self::ReadFile => "readFile",
            Self::WriteFile => "writeFile",
            Self::GetEnv => "getEnv",
            Self::Assert => "__assert",
            Self::Panic => "panic",
        }
    }

    /// Get the arity of this primitive.
    #[must_use]
    pub const fn arity(&self) -> usize {
        match self {
            // Unary
            Self::NegInt
            | Self::NegFloat
            | Self::Not
            | Self::StringLength
            | Self::IntToFloat
            | Self::FloatToInt
            | Self::IntToString
            | Self::FloatToString
            | Self::CharToString
            | Self::CharToInt
            | Self::IntToChar
            | Self::ListLength
            | Self::Print
            | Self::PrintNoNewline
            | Self::ReadLine
            | Self::GetEnv
            | Self::ReadFile
            | Self::Panic => 1,

            // Ternary
            Self::Substring => 3,

            // Binary (assert, write, and comparisons)
            Self::Assert | Self::WriteFile | Self::CharAt => 2,

            // All others are binary
            _ => 2,
        }
    }
}

impl fmt::Display for Primitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_id_fresh() {
        VarId::reset_counter();
        let v1 = VarId::fresh();
        let v2 = VarId::fresh();
        assert_ne!(v1, v2);
        assert_eq!(v1.raw(), 0);
        assert_eq!(v2.raw(), 1);
    }

    #[test]
    fn var_id_display() {
        let v = VarId::from_raw(42);
        assert_eq!(format!("{v}"), "_42");
    }

    #[test]
    fn literal_display() {
        assert_eq!(format!("{}", IRLiteral::Int(42)), "42");
        assert_eq!(format!("{}", IRLiteral::Float(3.14)), "3.14");
        assert_eq!(format!("{}", IRLiteral::String("hello".into())), "\"hello\"");
        assert_eq!(format!("{}", IRLiteral::Char('x')), "'x'");
        assert_eq!(format!("{}", IRLiteral::Bool(true)), "true");
    }

    #[test]
    fn expr_is_value() {
        let lit = IRExpr::Lit(IRLiteral::Int(42), Type::int());
        assert!(lit.is_value());

        VarId::reset_counter();
        let var = IRExpr::Var(VarId::fresh(), Type::int());
        assert!(var.is_value());

        let unit = IRExpr::Unit;
        assert!(unit.is_value());
    }

    #[test]
    fn expr_type() {
        let lit = IRExpr::Lit(IRLiteral::Int(42), Type::int());
        assert_eq!(lit.ty(), Type::int());

        let unit = IRExpr::Unit;
        assert_eq!(unit.ty(), Type::unit());
    }

    #[test]
    fn primitive_names() {
        assert_eq!(Primitive::AddInt.name(), "__add_int");
        assert_eq!(Primitive::Concat.name(), "__concat");
        assert_eq!(Primitive::Print.name(), "print");
    }

    #[test]
    fn primitive_arity() {
        assert_eq!(Primitive::AddInt.arity(), 2);
        assert_eq!(Primitive::NegInt.arity(), 1);
        assert_eq!(Primitive::Not.arity(), 1);
        assert_eq!(Primitive::WriteFile.arity(), 2);
    }
}
