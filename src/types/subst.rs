//! Type substitution for the Neap type system
//!
//! A substitution maps type variables to types. Substitutions are
//! the result of unification and are applied to types during inference.

use std::collections::HashMap;

use super::core::{Type, TypeScheme, TypeVar};

/// A substitution mapping type variables to types.
#[derive(Debug, Clone, Default)]
pub struct Substitution {
    /// The mapping from type variables to types
    map: HashMap<TypeVar, Type>,
}

impl Substitution {
    /// Create an empty substitution.
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Create a substitution with a single mapping.
    #[must_use]
    pub fn single(var: TypeVar, ty: Type) -> Self {
        let mut subst = Self::new();
        subst.insert(var, ty);
        subst
    }

    /// Insert a mapping into the substitution.
    pub fn insert(&mut self, var: TypeVar, ty: Type) {
        self.map.insert(var, ty);
    }

    /// Look up a type variable in the substitution.
    #[must_use]
    pub fn get(&self, var: TypeVar) -> Option<&Type> {
        self.map.get(&var)
    }

    /// Check if the substitution is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Get the number of mappings in the substitution.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Apply this substitution to a type.
    ///
    /// Replaces type variables with their mapped types, recursively.
    #[must_use]
    pub fn apply(&self, ty: &Type) -> Type {
        match ty {
            Type::Var(v) => {
                if let Some(replacement) = self.map.get(v) {
                    // Recursively apply in case the replacement contains more variables
                    self.apply(replacement)
                } else {
                    ty.clone()
                }
            }
            Type::Con(name, args) => {
                Type::Con(name.clone(), args.iter().map(|t| self.apply(t)).collect())
            }
            Type::Arrow(t1, t2) => {
                Type::Arrow(Box::new(self.apply(t1)), Box::new(self.apply(t2)))
            }
            Type::Tuple(elems) => {
                Type::Tuple(elems.iter().map(|t| self.apply(t)).collect())
            }
            Type::Record(fields) => {
                Type::Record(
                    fields
                        .iter()
                        .map(|(name, ty)| (name.clone(), self.apply(ty)))
                        .collect(),
                )
            }
        }
    }

    /// Apply this substitution to a type scheme.
    ///
    /// Only applies to free variables (not the quantified ones).
    #[must_use]
    pub fn apply_scheme(&self, scheme: &TypeScheme) -> TypeScheme {
        // Remove bound variables from the substitution temporarily
        let mut filtered = self.clone();
        for var in &scheme.vars {
            filtered.map.remove(var);
        }

        TypeScheme {
            vars: scheme.vars.clone(),
            ty: filtered.apply(&scheme.ty),
        }
    }

    /// Compose two substitutions: `self ∘ other`
    ///
    /// Applying the result is equivalent to applying `other` first,
    /// then applying `self`.
    #[must_use]
    pub fn compose(&self, other: &Substitution) -> Substitution {
        let mut result = Substitution::new();

        // First, apply self to all mappings in other
        for (var, ty) in &other.map {
            result.insert(*var, self.apply(ty));
        }

        // Then add mappings from self that aren't in other
        for (var, ty) in &self.map {
            if !other.map.contains_key(var) {
                result.insert(*var, ty.clone());
            }
        }

        result
    }

    /// Extend this substitution with another.
    ///
    /// The other substitution's mappings are added, and existing
    /// mappings in self are updated by applying the other substitution.
    pub fn extend(&mut self, other: &Substitution) {
        // Apply other to all existing mappings
        for ty in self.map.values_mut() {
            *ty = other.apply(ty);
        }

        // Add new mappings from other
        for (var, ty) in &other.map {
            if !self.map.contains_key(var) {
                self.insert(*var, ty.clone());
            }
        }
    }

    /// Iterate over all mappings.
    pub fn iter(&self) -> impl Iterator<Item = (&TypeVar, &Type)> {
        self.map.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subst_empty() {
        let subst = Substitution::new();
        assert!(subst.is_empty());
        assert_eq!(subst.len(), 0);
    }

    #[test]
    fn subst_single() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();
        let subst = Substitution::single(v, Type::int());

        assert!(!subst.is_empty());
        assert_eq!(subst.len(), 1);
        assert_eq!(subst.get(v), Some(&Type::int()));
    }

    #[test]
    fn subst_apply_var() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();
        let subst = Substitution::single(v, Type::int());

        let ty = Type::Var(v);
        assert_eq!(subst.apply(&ty), Type::int());
    }

    #[test]
    fn subst_apply_unbound_var() {
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();
        let subst = Substitution::single(v1, Type::int());

        let ty = Type::Var(v2);
        assert_eq!(subst.apply(&ty), Type::Var(v2));
    }

    #[test]
    fn subst_apply_arrow() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();
        let subst = Substitution::single(v, Type::int());

        let ty = Type::arrow(Type::Var(v), Type::Var(v));
        assert_eq!(subst.apply(&ty), Type::arrow(Type::int(), Type::int()));
    }

    #[test]
    fn subst_apply_nested() {
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();

        let mut subst = Substitution::new();
        subst.insert(v1, Type::Var(v2));
        subst.insert(v2, Type::int());

        // Applying to v1 should resolve through v2 to int
        let ty = Type::Var(v1);
        assert_eq!(subst.apply(&ty), Type::int());
    }

    #[test]
    fn subst_apply_con() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();
        let subst = Substitution::single(v, Type::int());

        let ty = Type::list(Type::Var(v));
        assert_eq!(subst.apply(&ty), Type::list(Type::int()));
    }

    #[test]
    fn subst_apply_tuple() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();
        let subst = Substitution::single(v, Type::int());

        let ty = Type::Tuple(vec![Type::Var(v), Type::string()]);
        assert_eq!(
            subst.apply(&ty),
            Type::Tuple(vec![Type::int(), Type::string()])
        );
    }

    #[test]
    fn subst_apply_record() {
        TypeVar::reset_counter();
        let v = TypeVar::fresh();
        let subst = Substitution::single(v, Type::int());

        let ty = Type::Record(vec![("x".to_string(), Type::Var(v))]);
        assert_eq!(
            subst.apply(&ty),
            Type::Record(vec![("x".to_string(), Type::int())])
        );
    }

    #[test]
    fn subst_apply_scheme_respects_bound() {
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();

        // subst: v1 -> int
        let subst = Substitution::single(v1, Type::int());

        // scheme: forall v1. v1 -> v2
        // v1 is bound, v2 is free
        let scheme = TypeScheme::poly(
            vec![v1],
            Type::arrow(Type::Var(v1), Type::Var(v2)),
        );

        let result = subst.apply_scheme(&scheme);

        // v1 should NOT be replaced (it's bound)
        // v2 should remain as-is (not in subst)
        assert_eq!(result.vars, vec![v1]);
        assert_eq!(result.ty, Type::arrow(Type::Var(v1), Type::Var(v2)));
    }

    #[test]
    fn subst_compose() {
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();

        // s1: v2 -> int
        let s1 = Substitution::single(v2, Type::int());

        // s2: v1 -> v2
        let s2 = Substitution::single(v1, Type::Var(v2));

        // compose: applying s2 then s1
        let composed = s1.compose(&s2);

        // v1 should map to int (v2 resolved to int)
        assert_eq!(composed.apply(&Type::Var(v1)), Type::int());
        assert_eq!(composed.apply(&Type::Var(v2)), Type::int());
    }

    #[test]
    fn subst_extend() {
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();

        let mut s1 = Substitution::single(v1, Type::Var(v2));
        let s2 = Substitution::single(v2, Type::int());

        s1.extend(&s2);

        // v1 should now map to int (through v2)
        assert_eq!(s1.apply(&Type::Var(v1)), Type::int());
        assert_eq!(s1.apply(&Type::Var(v2)), Type::int());
    }
}
