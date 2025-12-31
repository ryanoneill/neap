//! Runtime values for the Neap VM

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::ir::{IRExpr, VarId};

use super::env::Env;

/// A runtime value.
#[derive(Clone)]
pub enum Value {
    /// Unit value
    Unit,

    /// Boolean
    Bool(bool),

    /// Integer (64-bit signed)
    Int(i64),

    /// Float (64-bit)
    Float(f64),

    /// Character
    Char(char),

    /// String (reference counted for efficiency)
    String(Rc<String>),

    /// Function closure
    Closure {
        /// The parameter variable
        param: VarId,
        /// The function body
        body: Rc<IRExpr>,
        /// Captured environment
        env: Rc<Env>,
    },

    /// Data constructor (tagged union)
    Constructor {
        /// The constructor name (e.g., "None", "Some", "Nil", "Cons")
        tag: String,
        /// Optional payload
        payload: Option<Box<Value>>,
    },

    /// Tuple (heterogeneous, fixed-size)
    Tuple(Rc<Vec<Value>>),

    /// Record (named fields)
    Record(Rc<HashMap<String, Value>>),
}

impl Value {
    /// Create a string value.
    #[must_use]
    pub fn string(s: impl Into<String>) -> Self {
        Self::String(Rc::new(s.into()))
    }

    /// Create a tuple value.
    #[must_use]
    pub fn tuple(elems: Vec<Value>) -> Self {
        Self::Tuple(Rc::new(elems))
    }

    /// Create a record value.
    #[must_use]
    pub fn record(fields: HashMap<String, Value>) -> Self {
        Self::Record(Rc::new(fields))
    }

    /// Create a constructor with no payload.
    #[must_use]
    pub fn constructor(tag: impl Into<String>) -> Self {
        Self::Constructor {
            tag: tag.into(),
            payload: None,
        }
    }

    /// Create a constructor with a payload.
    #[must_use]
    pub fn constructor_with(tag: impl Into<String>, payload: Value) -> Self {
        Self::Constructor {
            tag: tag.into(),
            payload: Some(Box::new(payload)),
        }
    }

    /// Create None value.
    #[must_use]
    pub fn none() -> Self {
        Self::constructor("None")
    }

    /// Create Some value.
    #[must_use]
    pub fn some(value: Value) -> Self {
        Self::constructor_with("Some", value)
    }

    /// Create Nil (empty list).
    #[must_use]
    pub fn nil() -> Self {
        Self::constructor("Nil")
    }

    /// Create Cons cell.
    #[must_use]
    pub fn cons(head: Value, tail: Value) -> Self {
        Self::constructor_with("Cons", Value::tuple(vec![head, tail]))
    }

    /// Create Ok value.
    #[must_use]
    pub fn ok(value: Value) -> Self {
        Self::constructor_with("Ok", value)
    }

    /// Create Err value.
    #[must_use]
    pub fn err(value: Value) -> Self {
        Self::constructor_with("Err", value)
    }

    /// Check if this value is truthy.
    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Unit => false,
            Self::Int(n) => *n != 0,
            Self::Float(f) => *f != 0.0,
            Self::String(s) => !s.is_empty(),
            Self::Constructor { tag, .. } => tag != "None" && tag != "Nil",
            _ => true,
        }
    }

    /// Get the type name of this value (for error messages).
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::Char(_) => "char",
            Self::String(_) => "string",
            Self::Closure { .. } => "function",
            Self::Constructor { .. } => "constructor",
            Self::Tuple(_) => "tuple",
            Self::Record(_) => "record",
        }
    }

    /// Try to get as bool.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Try to get as int.
    #[must_use]
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// Try to get as float.
    #[must_use]
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Try to get as string.
    #[must_use]
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Try to get as char.
    #[must_use]
    pub fn as_char(&self) -> Option<char> {
        match self {
            Self::Char(c) => Some(*c),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unit => write!(f, "()"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Float(n) => write!(f, "{n}"),
            Self::Char(c) => write!(f, "'{c}'"),
            Self::String(s) => write!(f, "{s}"),
            Self::Closure { .. } => write!(f, "<function>"),
            Self::Constructor { tag, payload: None } => write!(f, "{tag}"),
            Self::Constructor {
                tag,
                payload: Some(p),
            } => {
                // Special case for Cons to print as list
                if tag == "Cons" {
                    write!(f, "[")?;
                    self.fmt_list(f)?;
                    write!(f, "]")
                } else {
                    write!(f, "{tag}({p})")
                }
            }
            Self::Tuple(elems) => {
                write!(f, "(")?;
                for (i, elem) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{elem}")?;
                }
                write!(f, ")")
            }
            Self::Record(fields) => {
                write!(f, "{{")?;
                let mut sorted: Vec<_> = fields.iter().collect();
                sorted.sort_by_key(|(k, _)| *k);
                for (i, (name, val)) in sorted.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{name} = {val}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

impl Value {
    /// Helper for printing lists.
    fn fmt_list(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Constructor { tag, payload: None } if tag == "Nil" => Ok(()),
            Self::Constructor {
                tag,
                payload: Some(p),
            } if tag == "Cons" => {
                if let Self::Tuple(elems) = p.as_ref()
                    && elems.len() == 2
                {
                    write!(f, "{}", elems[0])?;
                    if !matches!(&elems[1], Self::Constructor { tag, payload: None } if tag == "Nil")
                    {
                        write!(f, ", ")?;
                        elems[1].fmt_list(f)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unit => write!(f, "Unit"),
            Self::Bool(b) => write!(f, "Bool({b})"),
            Self::Int(n) => write!(f, "Int({n})"),
            Self::Float(n) => write!(f, "Float({n})"),
            Self::Char(c) => write!(f, "Char({c:?})"),
            Self::String(s) => write!(f, "String({s:?})"),
            Self::Closure { param, .. } => write!(f, "Closure(param={param})"),
            Self::Constructor { tag, payload } => {
                write!(f, "Constructor({tag}, {payload:?})")
            }
            Self::Tuple(elems) => write!(f, "Tuple({elems:?})"),
            Self::Record(fields) => write!(f, "Record({fields:?})"),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Unit, Self::Unit) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::Char(a), Self::Char(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (
                Self::Constructor {
                    tag: t1,
                    payload: p1,
                },
                Self::Constructor {
                    tag: t2,
                    payload: p2,
                },
            ) => t1 == t2 && p1 == p2,
            (Self::Tuple(a), Self::Tuple(b)) => a == b,
            (Self::Record(a), Self::Record(b)) => a == b,
            // Closures are never equal
            (Self::Closure { .. }, Self::Closure { .. }) => false,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_display() {
        assert_eq!(format!("{}", Value::Unit), "()");
        assert_eq!(format!("{}", Value::Bool(true)), "true");
        assert_eq!(format!("{}", Value::Int(42)), "42");
        assert_eq!(format!("{}", Value::Float(3.14)), "3.14");
        assert_eq!(format!("{}", Value::Char('x')), "'x'");
        assert_eq!(format!("{}", Value::string("hello")), "hello");
    }

    #[test]
    fn value_constructors() {
        let none = Value::none();
        assert!(matches!(none, Value::Constructor { tag, payload: None } if tag == "None"));

        let some = Value::some(Value::Int(42));
        assert!(
            matches!(&some, Value::Constructor { tag, payload: Some(_) } if tag == "Some")
        );
    }

    #[test]
    fn value_list_display() {
        let list = Value::cons(
            Value::Int(1),
            Value::cons(Value::Int(2), Value::cons(Value::Int(3), Value::nil())),
        );
        assert_eq!(format!("{list}"), "[1, 2, 3]");
    }

    #[test]
    fn value_empty_list() {
        let nil = Value::nil();
        assert!(matches!(nil, Value::Constructor { tag, payload: None } if tag == "Nil"));
    }

    #[test]
    fn value_tuple() {
        let t = Value::tuple(vec![Value::Int(1), Value::string("hello")]);
        assert_eq!(format!("{t}"), "(1, hello)");
    }

    #[test]
    fn value_record() {
        let mut fields = HashMap::new();
        fields.insert("x".to_string(), Value::Int(10));
        fields.insert("y".to_string(), Value::Int(20));
        let r = Value::record(fields);
        assert_eq!(format!("{r}"), "{x = 10, y = 20}");
    }

    #[test]
    fn value_equality() {
        assert_eq!(Value::Int(42), Value::Int(42));
        assert_ne!(Value::Int(42), Value::Int(43));
        assert_eq!(Value::string("a"), Value::string("a"));
        assert_eq!(Value::none(), Value::none());
    }

    #[test]
    fn value_type_name() {
        assert_eq!(Value::Unit.type_name(), "unit");
        assert_eq!(Value::Int(0).type_name(), "int");
        assert_eq!(Value::string("").type_name(), "string");
    }
}
