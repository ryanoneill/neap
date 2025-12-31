//! Type representations for the Neap type system
//!
//! Types in Neap follow the Hindley-Milner tradition with:
//! - Type variables (for polymorphism)
//! - Type constructors (int, string, bool, etc.)
//! - Function types (arrow types)
//! - Compound types (tuples, records, lists)

use std::collections::HashSet;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Global counter for generating unique type variable IDs
static TYPE_VAR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A type variable, used for polymorphism.
///
/// Type variables are created during type inference and may be
/// unified with concrete types or other type variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeVar(pub u64);

impl TypeVar {
    /// Create a new unique type variable.
    #[must_use]
    pub fn fresh() -> Self {
        Self(TYPE_VAR_COUNTER.fetch_add(1, Ordering::SeqCst))
    }

    /// Reset the type variable counter (for testing).
    #[cfg(test)]
    pub fn reset_counter() {
        TYPE_VAR_COUNTER.store(0, Ordering::SeqCst);
    }
}

impl fmt::Display for TypeVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Use letters for display: 'a, 'b, 'c, etc.
        let n = self.0 as usize;
        if n < 26 {
            write!(f, "'{}", (b'a' + n as u8) as char)
        } else {
            write!(f, "'t{}", n)
        }
    }
}

/// A monomorphic type (no quantifiers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// Type variable (may be unified)
    Var(TypeVar),

    /// Type constructor with arguments
    /// e.g., `int`, `string`, `list<'a>`, `option<'a>`
    Con(String, Vec<Type>),

    /// Function type: `t1 -> t2`
    Arrow(Box<Type>, Box<Type>),

    /// Tuple type: `t1 * t2 * ... * tn`
    Tuple(Vec<Type>),

    /// Record type: `{l1: t1, l2: t2, ...}`
    Record(Vec<(String, Type)>),
}

impl Type {
    // ========== Constructors for built-in types ==========

    /// The unit type `()`
    #[must_use]
    pub fn unit() -> Self {
        Self::Con("unit".to_string(), vec![])
    }

    /// The boolean type
    #[must_use]
    pub fn bool() -> Self {
        Self::Con("bool".to_string(), vec![])
    }

    /// The integer type
    #[must_use]
    pub fn int() -> Self {
        Self::Con("int".to_string(), vec![])
    }

    /// The float type
    #[must_use]
    pub fn float() -> Self {
        Self::Con("float".to_string(), vec![])
    }

    /// The string type
    #[must_use]
    pub fn string() -> Self {
        Self::Con("string".to_string(), vec![])
    }

    /// The char type
    #[must_use]
    pub fn char() -> Self {
        Self::Con("char".to_string(), vec![])
    }

    /// A list type `t list`
    #[must_use]
    pub fn list(elem: Type) -> Self {
        Self::Con("list".to_string(), vec![elem])
    }

    /// An option type `t option`
    #[must_use]
    pub fn option(elem: Type) -> Self {
        Self::Con("option".to_string(), vec![elem])
    }

    /// A result type `(t, e) result`
    #[must_use]
    pub fn result(ok: Type, err: Type) -> Self {
        Self::Con("result".to_string(), vec![ok, err])
    }

    /// The IO type `t io`
    #[must_use]
    pub fn io(inner: Type) -> Self {
        Self::Con("io".to_string(), vec![inner])
    }

    /// A stream type `t stream`
    #[must_use]
    pub fn stream(elem: Type) -> Self {
        Self::Con("stream".to_string(), vec![elem])
    }

    /// A function type `t1 -> t2`
    #[must_use]
    pub fn arrow(from: Type, to: Type) -> Self {
        Self::Arrow(Box::new(from), Box::new(to))
    }

    /// A fresh type variable
    #[must_use]
    pub fn var() -> Self {
        Self::Var(TypeVar::fresh())
    }

    // ========== Type operations ==========

    /// Check if this type is a type variable.
    #[must_use]
    pub const fn is_var(&self) -> bool {
        matches!(self, Self::Var(_))
    }

    /// Check if this type is a function type.
    #[must_use]
    pub const fn is_arrow(&self) -> bool {
        matches!(self, Self::Arrow(_, _))
    }

    /// Get all free type variables in this type.
    #[must_use]
    pub fn free_vars(&self) -> HashSet<TypeVar> {
        let mut vars = HashSet::new();
        self.collect_free_vars(&mut vars);
        vars
    }

    fn collect_free_vars(&self, vars: &mut HashSet<TypeVar>) {
        match self {
            Self::Var(v) => {
                vars.insert(*v);
            }
            Self::Con(_, args) => {
                for arg in args {
                    arg.collect_free_vars(vars);
                }
            }
            Self::Arrow(t1, t2) => {
                t1.collect_free_vars(vars);
                t2.collect_free_vars(vars);
            }
            Self::Tuple(elems) => {
                for elem in elems {
                    elem.collect_free_vars(vars);
                }
            }
            Self::Record(fields) => {
                for (_, ty) in fields {
                    ty.collect_free_vars(vars);
                }
            }
        }
    }

    /// Check if this type contains a specific type variable.
    #[must_use]
    pub fn contains_var(&self, var: TypeVar) -> bool {
        match self {
            Self::Var(v) => *v == var,
            Self::Con(_, args) => args.iter().any(|t| t.contains_var(var)),
            Self::Arrow(t1, t2) => t1.contains_var(var) || t2.contains_var(var),
            Self::Tuple(elems) => elems.iter().any(|t| t.contains_var(var)),
            Self::Record(fields) => fields.iter().any(|(_, t)| t.contains_var(var)),
        }
    }

    /// Create a multi-argument function type: `t1 -> t2 -> ... -> tn -> ret`
    #[must_use]
    pub fn arrows(params: Vec<Type>, ret: Type) -> Self {
        params.into_iter().rev().fold(ret, |acc, param| Self::arrow(param, acc))
    }

    /// Decompose a function type into its argument and return types.
    /// Returns `None` if this is not a function type.
    #[must_use]
    pub fn decompose_arrow(&self) -> Option<(&Type, &Type)> {
        match self {
            Self::Arrow(t1, t2) => Some((t1, t2)),
            _ => None,
        }
    }

    /// Collect all argument types of a curried function type.
    /// e.g., `int -> string -> bool` returns `([int, string], bool)`
    #[must_use]
    pub fn collect_arrow_args(&self) -> (Vec<&Type>, &Type) {
        let mut args = Vec::new();
        let mut current = self;

        while let Self::Arrow(param, ret) = current {
            args.push(param.as_ref());
            current = ret.as_ref();
        }

        (args, current)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Var(v) => write!(f, "{v}"),
            Self::Con(name, args) => {
                if args.is_empty() {
                    write!(f, "{name}")
                } else if args.len() == 1 {
                    write!(f, "{} {name}", args[0])
                } else {
                    write!(f, "(")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{arg}")?;
                    }
                    write!(f, ") {name}")
                }
            }
            Self::Arrow(t1, t2) => {
                // Parenthesize left side if it's also an arrow
                if t1.is_arrow() {
                    write!(f, "({t1}) -> {t2}")
                } else {
                    write!(f, "{t1} -> {t2}")
                }
            }
            Self::Tuple(elems) => {
                if elems.is_empty() {
                    write!(f, "unit")
                } else {
                    for (i, elem) in elems.iter().enumerate() {
                        if i > 0 {
                            write!(f, " * ")?;
                        }
                        // Parenthesize if element is an arrow or tuple
                        if elem.is_arrow() || matches!(elem, Self::Tuple(_)) {
                            write!(f, "({elem})")?;
                        } else {
                            write!(f, "{elem}")?;
                        }
                    }
                    Ok(())
                }
            }
            Self::Record(fields) => {
                write!(f, "{{")?;
                for (i, (name, ty)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{name}: {ty}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

/// A type scheme (polymorphic type).
///
/// A type scheme is a type with universally quantified type variables.
/// e.g., `forall 'a. 'a -> 'a` (the identity function)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeScheme {
    /// The quantified type variables
    pub vars: Vec<TypeVar>,
    /// The body type
    pub ty: Type,
}

impl TypeScheme {
    /// Create a monomorphic type scheme (no quantification).
    #[must_use]
    pub fn mono(ty: Type) -> Self {
        Self { vars: vec![], ty }
    }

    /// Create a polymorphic type scheme.
    #[must_use]
    pub fn poly(vars: Vec<TypeVar>, ty: Type) -> Self {
        Self { vars, ty }
    }

    /// Instantiate this type scheme with fresh type variables.
    ///
    /// Replaces each quantified variable with a fresh type variable.
    #[must_use]
    pub fn instantiate(&self) -> Type {
        use super::subst::Substitution;

        if self.vars.is_empty() {
            return self.ty.clone();
        }

        let mut subst = Substitution::new();
        for &var in &self.vars {
            subst.insert(var, Type::var());
        }
        subst.apply(&self.ty)
    }

    /// Get the free type variables (those not quantified).
    #[must_use]
    pub fn free_vars(&self) -> HashSet<TypeVar> {
        let mut vars = self.ty.free_vars();
        for v in &self.vars {
            vars.remove(v);
        }
        vars
    }
}

impl fmt::Display for TypeScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.vars.is_empty() {
            write!(f, "{}", self.ty)
        } else {
            write!(f, "forall")?;
            for var in &self.vars {
                write!(f, " {var}")?;
            }
            write!(f, ". {}", self.ty)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_var_fresh() {
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();
        assert_ne!(v1, v2);
        assert_eq!(v1.0 + 1, v2.0);
    }

    #[test]
    fn type_var_display() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();
        assert_eq!(format!("{v}"), "'a");
    }

    #[test]
    fn type_constructors() {
        assert_eq!(format!("{}", Type::int()), "int");
        assert_eq!(format!("{}", Type::bool()), "bool");
        assert_eq!(format!("{}", Type::string()), "string");
        assert_eq!(format!("{}", Type::unit()), "unit");
    }

    #[test]
    fn type_list() {
        let list_int = Type::list(Type::int());
        assert_eq!(format!("{list_int}"), "int list");
    }

    #[test]
    fn type_arrow() {
        let arrow = Type::arrow(Type::int(), Type::bool());
        assert_eq!(format!("{arrow}"), "int -> bool");
    }

    #[test]
    fn type_arrow_nested() {
        // int -> int -> int
        let arrow = Type::arrow(Type::int(), Type::arrow(Type::int(), Type::int()));
        assert_eq!(format!("{arrow}"), "int -> int -> int");
    }

    #[test]
    fn type_arrow_left_nested() {
        // (int -> int) -> int
        let arrow = Type::arrow(Type::arrow(Type::int(), Type::int()), Type::int());
        assert_eq!(format!("{arrow}"), "(int -> int) -> int");
    }

    #[test]
    fn type_tuple() {
        let tuple = Type::Tuple(vec![Type::int(), Type::string(), Type::bool()]);
        assert_eq!(format!("{tuple}"), "int * string * bool");
    }

    #[test]
    fn type_record() {
        let record = Type::Record(vec![
            ("x".to_string(), Type::int()),
            ("y".to_string(), Type::int()),
        ]);
        assert_eq!(format!("{record}"), "{x: int, y: int}");
    }

    #[test]
    fn type_free_vars() {
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();

        let ty = Type::arrow(Type::Var(v1), Type::Var(v2));
        let free = ty.free_vars();
        assert!(free.contains(&v1));
        assert!(free.contains(&v2));
        assert_eq!(free.len(), 2);
    }

    #[test]
    fn type_contains_var() {
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();

        let ty = Type::arrow(Type::Var(v1), Type::int());
        assert!(ty.contains_var(v1));
        assert!(!ty.contains_var(v2));
    }

    #[test]
    fn type_arrows() {
        let ty = Type::arrows(vec![Type::int(), Type::string()], Type::bool());
        assert_eq!(format!("{ty}"), "int -> string -> bool");
    }

    #[test]
    fn type_decompose_arrow() {
        let ty = Type::arrow(Type::int(), Type::bool());
        let (from, to) = ty.decompose_arrow().unwrap();
        assert_eq!(*from, Type::int());
        assert_eq!(*to, Type::bool());

        assert!(Type::int().decompose_arrow().is_none());
    }

    #[test]
    fn type_collect_arrow_args() {
        let ty = Type::arrows(vec![Type::int(), Type::string()], Type::bool());
        let (args, ret) = ty.collect_arrow_args();
        assert_eq!(args.len(), 2);
        assert_eq!(*args[0], Type::int());
        assert_eq!(*args[1], Type::string());
        assert_eq!(*ret, Type::bool());
    }

    #[test]
    fn type_scheme_mono() {
        let scheme = TypeScheme::mono(Type::int());
        assert!(scheme.vars.is_empty());
        assert_eq!(format!("{scheme}"), "int");
    }

    #[test]
    fn type_scheme_poly() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();
        let scheme = TypeScheme::poly(vec![v], Type::arrow(Type::Var(v), Type::Var(v)));
        assert_eq!(format!("{scheme}"), "forall 'a. 'a -> 'a");
    }

    #[test]
    fn type_scheme_instantiate() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();
        let scheme = TypeScheme::poly(vec![v], Type::arrow(Type::Var(v), Type::Var(v)));

        let instantiated = scheme.instantiate();
        // Should have a fresh variable
        if let Type::Arrow(t1, t2) = instantiated {
            assert!(t1.is_var());
            assert!(t2.is_var());
            assert_eq!(t1, t2); // Same variable in both positions
        } else {
            panic!("Expected arrow type");
        }
    }

    #[test]
    fn type_scheme_free_vars() {
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();

        // forall 'a. 'a -> 'b
        let scheme = TypeScheme::poly(
            vec![v1],
            Type::arrow(Type::Var(v1), Type::Var(v2)),
        );

        let free = scheme.free_vars();
        assert!(!free.contains(&v1)); // v1 is bound
        assert!(free.contains(&v2)); // v2 is free
    }
}
