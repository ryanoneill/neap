//! Pattern matching for the Neap VM
//!
//! Implements pattern matching against runtime values, returning
//! variable bindings on success.

use crate::ir::{IRLiteral, IRPattern, VarId};

use super::value::Value;

/// Match a pattern against a value.
///
/// Returns `Some(bindings)` if the pattern matches, where bindings is a
/// vector of (variable, value) pairs that should be added to the environment.
/// Returns `None` if the pattern does not match.
pub fn match_pattern(pattern: &IRPattern, value: &Value) -> Option<Vec<(VarId, Value)>> {
    match pattern {
        IRPattern::Wildcard => Some(vec![]),

        IRPattern::Var(var, _) => Some(vec![(*var, value.clone())]),

        IRPattern::Lit(lit) => {
            if literal_matches(lit, value) {
                Some(vec![])
            } else {
                None
            }
        }

        IRPattern::Tuple(patterns) => {
            if let Value::Tuple(values) = value {
                if patterns.len() != values.len() {
                    return None;
                }

                let mut bindings = Vec::new();
                for (pat, val) in patterns.iter().zip(values.iter()) {
                    let sub_bindings = match_pattern(pat, val)?;
                    bindings.extend(sub_bindings);
                }
                Some(bindings)
            } else {
                None
            }
        }

        IRPattern::Record(field_patterns) => {
            if let Value::Record(fields) = value {
                let mut bindings = Vec::new();
                for (field_name, pat) in field_patterns {
                    let field_val = fields.get(field_name)?;
                    let sub_bindings = match_pattern(pat, field_val)?;
                    bindings.extend(sub_bindings);
                }
                Some(bindings)
            } else {
                None
            }
        }

        IRPattern::Con { ctor, arg } => {
            if let Value::Constructor { tag, payload } = value {
                if tag != ctor {
                    return None;
                }

                match (arg, payload) {
                    (None, None) => Some(vec![]),
                    (Some(pat), Some(val)) => match_pattern(pat, val),
                    _ => None,
                }
            } else {
                None
            }
        }
    }
}

/// Check if a literal pattern matches a value.
fn literal_matches(lit: &IRLiteral, value: &Value) -> bool {
    match (lit, value) {
        (IRLiteral::Bool(a), Value::Bool(b)) => a == b,
        (IRLiteral::Int(a), Value::Int(b)) => a == b,
        (IRLiteral::Float(a), Value::Float(b)) => a == b,
        (IRLiteral::Char(a), Value::Char(b)) => a == b,
        (IRLiteral::String(a), Value::String(b)) => a == b.as_ref(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Type;

    #[test]
    fn match_wildcard() {
        let pattern = IRPattern::Wildcard;

        assert!(match_pattern(&pattern, &Value::Int(42)).is_some());
        assert!(match_pattern(&pattern, &Value::Bool(true)).is_some());
        assert!(match_pattern(&pattern, &Value::Unit).is_some());
    }

    #[test]
    fn match_var() {
        let var = VarId::fresh();
        let pattern = IRPattern::Var(var, Type::int());

        let bindings = match_pattern(&pattern, &Value::Int(42)).unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0], (var, Value::Int(42)));
    }

    #[test]
    fn match_literal_int() {
        let pattern = IRPattern::Lit(IRLiteral::Int(42));

        assert!(match_pattern(&pattern, &Value::Int(42)).is_some());
        assert!(match_pattern(&pattern, &Value::Int(43)).is_none());
        assert!(match_pattern(&pattern, &Value::Bool(true)).is_none());
    }

    #[test]
    fn match_literal_bool() {
        let pattern = IRPattern::Lit(IRLiteral::Bool(true));

        assert!(match_pattern(&pattern, &Value::Bool(true)).is_some());
        assert!(match_pattern(&pattern, &Value::Bool(false)).is_none());
    }

    #[test]
    fn match_literal_string() {
        let pattern = IRPattern::Lit(IRLiteral::String("hello".to_string()));

        assert!(match_pattern(&pattern, &Value::string("hello")).is_some());
        assert!(match_pattern(&pattern, &Value::string("world")).is_none());
    }

    #[test]
    fn match_tuple() {
        let v1 = VarId::fresh();
        let v2 = VarId::fresh();
        let pattern = IRPattern::Tuple(vec![
            IRPattern::Var(v1, Type::int()),
            IRPattern::Var(v2, Type::int()),
        ]);

        let value = Value::tuple(vec![Value::Int(1), Value::Int(2)]);
        let bindings = match_pattern(&pattern, &value).unwrap();

        assert_eq!(bindings.len(), 2);
        assert!(bindings.contains(&(v1, Value::Int(1))));
        assert!(bindings.contains(&(v2, Value::Int(2))));
    }

    #[test]
    fn match_tuple_wrong_length() {
        let v1 = VarId::fresh();
        let v2 = VarId::fresh();
        let pattern = IRPattern::Tuple(vec![
            IRPattern::Var(v1, Type::int()),
            IRPattern::Var(v2, Type::int()),
        ]);

        let value = Value::tuple(vec![Value::Int(1)]);
        assert!(match_pattern(&pattern, &value).is_none());
    }

    #[test]
    fn match_tuple_not_tuple() {
        let v1 = VarId::fresh();
        let pattern = IRPattern::Tuple(vec![IRPattern::Var(v1, Type::int())]);

        assert!(match_pattern(&pattern, &Value::Int(42)).is_none());
    }

    #[test]
    fn match_nested_tuple() {
        let v1 = VarId::fresh();
        let v2 = VarId::fresh();
        let v3 = VarId::fresh();

        // (x, (y, z))
        let pattern = IRPattern::Tuple(vec![
            IRPattern::Var(v1, Type::int()),
            IRPattern::Tuple(vec![
                IRPattern::Var(v2, Type::int()),
                IRPattern::Var(v3, Type::int()),
            ]),
        ]);

        let value = Value::tuple(vec![
            Value::Int(1),
            Value::tuple(vec![Value::Int(2), Value::Int(3)]),
        ]);

        let bindings = match_pattern(&pattern, &value).unwrap();
        assert_eq!(bindings.len(), 3);
        assert!(bindings.contains(&(v1, Value::Int(1))));
        assert!(bindings.contains(&(v2, Value::Int(2))));
        assert!(bindings.contains(&(v3, Value::Int(3))));
    }

    #[test]
    fn match_record() {
        let v1 = VarId::fresh();
        let v2 = VarId::fresh();
        let pattern = IRPattern::Record(vec![
            ("x".to_string(), IRPattern::Var(v1, Type::int())),
            ("y".to_string(), IRPattern::Var(v2, Type::int())),
        ]);

        let mut fields = std::collections::HashMap::new();
        fields.insert("x".to_string(), Value::Int(10));
        fields.insert("y".to_string(), Value::Int(20));
        let value = Value::record(fields);

        let bindings = match_pattern(&pattern, &value).unwrap();
        assert_eq!(bindings.len(), 2);
        assert!(bindings.contains(&(v1, Value::Int(10))));
        assert!(bindings.contains(&(v2, Value::Int(20))));
    }

    #[test]
    fn match_record_missing_field() {
        let v1 = VarId::fresh();
        let pattern = IRPattern::Record(vec![("z".to_string(), IRPattern::Var(v1, Type::int()))]);

        let mut fields = std::collections::HashMap::new();
        fields.insert("x".to_string(), Value::Int(10));
        let value = Value::record(fields);

        assert!(match_pattern(&pattern, &value).is_none());
    }

    #[test]
    fn match_record_partial() {
        // Pattern only matches subset of fields
        let v1 = VarId::fresh();
        let pattern = IRPattern::Record(vec![("x".to_string(), IRPattern::Var(v1, Type::int()))]);

        let mut fields = std::collections::HashMap::new();
        fields.insert("x".to_string(), Value::Int(10));
        fields.insert("y".to_string(), Value::Int(20));
        let value = Value::record(fields);

        let bindings = match_pattern(&pattern, &value).unwrap();
        assert_eq!(bindings.len(), 1);
        assert!(bindings.contains(&(v1, Value::Int(10))));
    }

    #[test]
    fn match_constructor_no_payload() {
        let pattern = IRPattern::Con {
            ctor: "None".to_string(),
            arg: None,
        };

        let value = Value::none();
        assert!(match_pattern(&pattern, &value).is_some());

        let value = Value::some(Value::Int(42));
        assert!(match_pattern(&pattern, &value).is_none());
    }

    #[test]
    fn match_constructor_with_payload() {
        let v1 = VarId::fresh();
        let pattern = IRPattern::Con {
            ctor: "Some".to_string(),
            arg: Some(Box::new(IRPattern::Var(v1, Type::int()))),
        };

        let value = Value::some(Value::Int(42));
        let bindings = match_pattern(&pattern, &value).unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0], (v1, Value::Int(42)));

        let value = Value::none();
        assert!(match_pattern(&pattern, &value).is_none());
    }

    #[test]
    fn match_constructor_wrong_tag() {
        let v1 = VarId::fresh();
        let pattern = IRPattern::Con {
            ctor: "Ok".to_string(),
            arg: Some(Box::new(IRPattern::Var(v1, Type::string()))),
        };

        let value = Value::err(Value::string("error"));
        assert!(match_pattern(&pattern, &value).is_none());
    }

    #[test]
    fn match_list_cons() {
        let v1 = VarId::fresh();
        let v2 = VarId::fresh();

        // Cons(h, t) pattern
        let _tuple_ty = Type::Tuple(vec![Type::int(), Type::list(Type::int())]);
        let pattern = IRPattern::Con {
            ctor: "Cons".to_string(),
            arg: Some(Box::new(IRPattern::Tuple(vec![
                IRPattern::Var(v1, Type::int()), // head
                IRPattern::Var(v2, Type::list(Type::int())), // tail
            ]))),
        };

        let list = Value::cons(Value::Int(1), Value::cons(Value::Int(2), Value::nil()));
        let bindings = match_pattern(&pattern, &list).unwrap();

        assert_eq!(bindings.len(), 2);
        assert!(bindings.iter().any(|(v, val)| *v == v1 && *val == Value::Int(1)));
    }

    #[test]
    fn match_list_nil() {
        let pattern = IRPattern::Con {
            ctor: "Nil".to_string(),
            arg: None,
        };

        let value = Value::nil();
        assert!(match_pattern(&pattern, &value).is_some());

        let value = Value::cons(Value::Int(1), Value::nil());
        assert!(match_pattern(&pattern, &value).is_none());
    }
}
