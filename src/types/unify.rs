//! Type unification for the Neap type system
//!
//! Implements Robinson's unification algorithm for determining
//! if two types can be made equal by substitution.

use super::error::TypeError;
use super::subst::Substitution;
use super::core::{Type, TypeVar};

/// Unify two types, returning a substitution that makes them equal.
///
/// If unification fails, returns a type error explaining why.
pub fn unify(t1: &Type, t2: &Type) -> Result<Substitution, TypeError> {
    match (t1, t2) {
        // Same type variable: already unified
        (Type::Var(v1), Type::Var(v2)) if v1 == v2 => Ok(Substitution::new()),

        // Type variable on left: bind it to the right type
        (Type::Var(v), t) => bind_var(*v, t),

        // Type variable on right: bind it to the left type
        (t, Type::Var(v)) => bind_var(*v, t),

        // Same type constructor: unify arguments
        (Type::Con(n1, args1), Type::Con(n2, args2)) => {
            if n1 != n2 {
                return Err(TypeError::TypeMismatch {
                    expected: t1.clone(),
                    actual: t2.clone(),
                });
            }

            if args1.len() != args2.len() {
                return Err(TypeError::TypeMismatch {
                    expected: t1.clone(),
                    actual: t2.clone(),
                });
            }

            unify_many(args1, args2)
        }

        // Arrow types: unify both sides
        (Type::Arrow(a1, r1), Type::Arrow(a2, r2)) => {
            let s1 = unify(a1, a2)?;
            let r1_subst = s1.apply(r1);
            let r2_subst = s1.apply(r2);
            let s2 = unify(&r1_subst, &r2_subst)?;
            Ok(s2.compose(&s1))
        }

        // Tuple types: unify element-wise
        (Type::Tuple(elems1), Type::Tuple(elems2)) => {
            if elems1.len() != elems2.len() {
                return Err(TypeError::TypeMismatch {
                    expected: t1.clone(),
                    actual: t2.clone(),
                });
            }

            unify_many(elems1, elems2)
        }

        // Record types: unify field-wise
        (Type::Record(fields1), Type::Record(fields2)) => {
            if fields1.len() != fields2.len() {
                return Err(TypeError::TypeMismatch {
                    expected: t1.clone(),
                    actual: t2.clone(),
                });
            }

            // Check that all field names match
            let mut sorted1 = fields1.clone();
            let mut sorted2 = fields2.clone();
            sorted1.sort_by(|a, b| a.0.cmp(&b.0));
            sorted2.sort_by(|a, b| a.0.cmp(&b.0));

            if sorted1.iter().map(|(n, _)| n).collect::<Vec<_>>()
                != sorted2.iter().map(|(n, _)| n).collect::<Vec<_>>()
            {
                return Err(TypeError::TypeMismatch {
                    expected: t1.clone(),
                    actual: t2.clone(),
                });
            }

            // Unify types in sorted order
            let types1: Vec<_> = sorted1.into_iter().map(|(_, t)| t).collect();
            let types2: Vec<_> = sorted2.into_iter().map(|(_, t)| t).collect();
            unify_many(&types1, &types2)
        }

        // Everything else is a type mismatch
        _ => Err(TypeError::TypeMismatch {
            expected: t1.clone(),
            actual: t2.clone(),
        }),
    }
}

/// Bind a type variable to a type, with occurs check.
fn bind_var(var: TypeVar, ty: &Type) -> Result<Substitution, TypeError> {
    // Occurs check: prevent infinite types like 'a = 'a -> 'b
    if ty.contains_var(var) {
        return Err(TypeError::InfiniteType {
            var,
            ty: ty.clone(),
        });
    }

    Ok(Substitution::single(var, ty.clone()))
}

/// Unify multiple pairs of types, threading the substitution through.
fn unify_many(types1: &[Type], types2: &[Type]) -> Result<Substitution, TypeError> {
    let mut subst = Substitution::new();

    for (t1, t2) in types1.iter().zip(types2.iter()) {
        let t1_subst = subst.apply(t1);
        let t2_subst = subst.apply(t2);
        let s = unify(&t1_subst, &t2_subst)?;
        subst.extend(&s);
    }

    Ok(subst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unify_same_type() {
        let result = unify(&Type::int(), &Type::int());
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn unify_different_types() {
        let result = unify(&Type::int(), &Type::string());
        assert!(result.is_err());
    }

    #[test]
    fn unify_var_with_type() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();
        let result = unify(&Type::Var(v), &Type::int());

        assert!(result.is_ok());
        let subst = result.unwrap();
        assert_eq!(subst.apply(&Type::Var(v)), Type::int());
    }

    #[test]
    fn unify_type_with_var() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();
        let result = unify(&Type::int(), &Type::Var(v));

        assert!(result.is_ok());
        let subst = result.unwrap();
        assert_eq!(subst.apply(&Type::Var(v)), Type::int());
    }

    #[test]
    fn unify_same_var() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();
        let result = unify(&Type::Var(v), &Type::Var(v));

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn unify_different_vars() {
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();
        let result = unify(&Type::Var(v1), &Type::Var(v2));

        assert!(result.is_ok());
        let subst = result.unwrap();

        // One should map to the other
        let applied1 = subst.apply(&Type::Var(v1));
        let applied2 = subst.apply(&Type::Var(v2));
        assert_eq!(applied1, applied2);
    }

    #[test]
    fn unify_arrow_types() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();

        // 'a -> int  ≈  string -> 'a
        let t1 = Type::arrow(Type::Var(v), Type::int());
        let t2 = Type::arrow(Type::string(), Type::Var(v));

        let result = unify(&t1, &t2);
        // Should fail: 'a must be both string (from arg) and int (from ret)
        assert!(result.is_err());
    }

    #[test]
    fn unify_arrow_types_success() {
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();

        // 'a -> 'b  ≈  int -> string
        let t1 = Type::arrow(Type::Var(v1), Type::Var(v2));
        let t2 = Type::arrow(Type::int(), Type::string());

        let result = unify(&t1, &t2);
        assert!(result.is_ok());

        let subst = result.unwrap();
        assert_eq!(subst.apply(&Type::Var(v1)), Type::int());
        assert_eq!(subst.apply(&Type::Var(v2)), Type::string());
    }

    #[test]
    fn unify_occurs_check() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();

        // 'a ≈ 'a -> int  should fail (infinite type)
        let t1 = Type::Var(v);
        let t2 = Type::arrow(Type::Var(v), Type::int());

        let result = unify(&t1, &t2);
        assert!(matches!(result, Err(TypeError::InfiniteType { .. })));
    }

    #[test]
    fn unify_list_types() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();

        // 'a list ≈ int list
        let t1 = Type::list(Type::Var(v));
        let t2 = Type::list(Type::int());

        let result = unify(&t1, &t2);
        assert!(result.is_ok());

        let subst = result.unwrap();
        assert_eq!(subst.apply(&Type::Var(v)), Type::int());
    }

    #[test]
    fn unify_tuple_types() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();

        // ('a, string) ≈ (int, string)
        let t1 = Type::Tuple(vec![Type::Var(v), Type::string()]);
        let t2 = Type::Tuple(vec![Type::int(), Type::string()]);

        let result = unify(&t1, &t2);
        assert!(result.is_ok());

        let subst = result.unwrap();
        assert_eq!(subst.apply(&Type::Var(v)), Type::int());
    }

    #[test]
    fn unify_tuple_different_lengths() {
        // (int, string) ≈ (int, string, bool) should fail
        let t1 = Type::Tuple(vec![Type::int(), Type::string()]);
        let t2 = Type::Tuple(vec![Type::int(), Type::string(), Type::bool()]);

        let result = unify(&t1, &t2);
        assert!(result.is_err());
    }

    #[test]
    fn unify_record_types() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();

        // {x: 'a, y: string} ≈ {x: int, y: string}
        let t1 = Type::Record(vec![
            ("x".to_string(), Type::Var(v)),
            ("y".to_string(), Type::string()),
        ]);
        let t2 = Type::Record(vec![
            ("x".to_string(), Type::int()),
            ("y".to_string(), Type::string()),
        ]);

        let result = unify(&t1, &t2);
        assert!(result.is_ok());

        let subst = result.unwrap();
        assert_eq!(subst.apply(&Type::Var(v)), Type::int());
    }

    #[test]
    fn unify_record_different_order() {
        // {x: int, y: string} ≈ {y: string, x: int} should succeed
        let t1 = Type::Record(vec![
            ("x".to_string(), Type::int()),
            ("y".to_string(), Type::string()),
        ]);
        let t2 = Type::Record(vec![
            ("y".to_string(), Type::string()),
            ("x".to_string(), Type::int()),
        ]);

        let result = unify(&t1, &t2);
        assert!(result.is_ok());
    }

    #[test]
    fn unify_record_different_fields() {
        // {x: int} ≈ {y: int} should fail
        let t1 = Type::Record(vec![("x".to_string(), Type::int())]);
        let t2 = Type::Record(vec![("y".to_string(), Type::int())]);

        let result = unify(&t1, &t2);
        assert!(result.is_err());
    }

    #[test]
    fn unify_complex_type() {
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();

        // ('a -> 'b) list ≈ (int -> string) list
        let t1 = Type::list(Type::arrow(Type::Var(v1), Type::Var(v2)));
        let t2 = Type::list(Type::arrow(Type::int(), Type::string()));

        let result = unify(&t1, &t2);
        assert!(result.is_ok());

        let subst = result.unwrap();
        assert_eq!(subst.apply(&Type::Var(v1)), Type::int());
        assert_eq!(subst.apply(&Type::Var(v2)), Type::string());
    }
}
