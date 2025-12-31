//! Environment for the Neap VM
//!
//! The environment maintains variable bindings using a linked chain of scopes
//! for lexical scoping. Rc is used for sharing environments in closures.

use std::collections::HashMap;
use std::rc::Rc;

use crate::ir::VarId;

use super::value::Value;

/// A lexically-scoped environment for variable bindings.
///
/// Environments form a linked chain where each scope has an optional parent.
/// This supports lexical scoping and efficient closure capture.
#[derive(Debug, Clone)]
pub struct Env {
    /// Bindings in this scope
    bindings: HashMap<VarId, Value>,
    /// Parent scope (if any)
    parent: Option<Rc<Env>>,
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

impl Env {
    /// Create a new empty environment.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            parent: None,
        }
    }

    /// Create a new environment with a parent scope.
    #[must_use]
    pub fn with_parent(parent: Rc<Env>) -> Self {
        Self {
            bindings: HashMap::new(),
            parent: Some(parent),
        }
    }

    /// Look up a variable in this environment.
    ///
    /// Searches the current scope first, then parent scopes.
    #[must_use]
    pub fn lookup(&self, var: VarId) -> Option<&Value> {
        if let Some(value) = self.bindings.get(&var) {
            Some(value)
        } else if let Some(parent) = &self.parent {
            parent.lookup(var)
        } else {
            None
        }
    }

    /// Bind a variable in this scope.
    pub fn bind(&mut self, var: VarId, value: Value) {
        self.bindings.insert(var, value);
    }

    /// Create a new environment extended with a single binding.
    ///
    /// This creates a new scope with the current environment as parent.
    #[must_use]
    pub fn extend(self: &Rc<Self>, var: VarId, value: Value) -> Self {
        let mut new_env = Self::with_parent(Rc::clone(self));
        new_env.bind(var, value);
        new_env
    }

    /// Create a new environment extended with multiple bindings.
    ///
    /// This creates a new scope with the current environment as parent.
    #[must_use]
    pub fn extend_many(self: &Rc<Self>, bindings: impl IntoIterator<Item = (VarId, Value)>) -> Self {
        let mut new_env = Self::with_parent(Rc::clone(self));
        for (var, value) in bindings {
            new_env.bind(var, value);
        }
        new_env
    }

    /// Check if a variable is bound in this environment (any scope).
    #[must_use]
    pub fn contains(&self, var: VarId) -> bool {
        self.lookup(var).is_some()
    }

    /// Get the number of bindings in this scope only (not including parents).
    #[must_use]
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Check if this scope is empty (not including parents).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Create a restricted environment containing only the specified variables.
    ///
    /// This is used for closure capture - we only capture the free variables
    /// that the closure actually needs.
    #[must_use]
    pub fn restrict(&self, vars: &[VarId]) -> Self {
        let mut bindings = HashMap::new();
        for &var in vars {
            if let Some(value) = self.lookup(var) {
                bindings.insert(var, value.clone());
            }
        }
        Self {
            bindings,
            parent: None,
        }
    }

    /// Get all variable IDs bound in this scope (not including parents).
    pub fn keys(&self) -> impl Iterator<Item = &VarId> {
        self.bindings.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_new() {
        let env = Env::new();
        assert!(env.is_empty());
        assert_eq!(env.len(), 0);
    }

    #[test]
    fn env_bind_and_lookup() {
        let v1 = VarId::fresh();
        let v2 = VarId::fresh();
        let mut env = Env::new();
        env.bind(v1, Value::Int(42));

        assert_eq!(env.lookup(v1), Some(&Value::Int(42)));
        assert_eq!(env.lookup(v2), None);
    }

    #[test]
    fn env_extend() {
        let v1 = VarId::fresh();
        let env = Rc::new(Env::new());
        let env2 = env.extend(v1, Value::Int(42));

        assert_eq!(env2.lookup(v1), Some(&Value::Int(42)));
        assert!(env.lookup(v1).is_none()); // Original unchanged
    }

    #[test]
    fn env_shadowing() {
        let v1 = VarId::fresh();
        let mut env = Env::new();
        env.bind(v1, Value::Int(42));
        let env = Rc::new(env);

        let env2 = env.extend(v1, Value::Int(100));

        // New binding shadows old one
        assert_eq!(env2.lookup(v1), Some(&Value::Int(100)));
        // Old env still has old value
        assert_eq!(env.lookup(v1), Some(&Value::Int(42)));
    }

    #[test]
    fn env_parent_lookup() {
        let v1 = VarId::fresh();
        let v2 = VarId::fresh();
        let mut env = Env::new();
        env.bind(v1, Value::Int(42));
        let env = Rc::new(env);

        let env2 = env.extend(v2, Value::Int(100));

        // Can look up from parent
        assert_eq!(env2.lookup(v1), Some(&Value::Int(42)));
        // And from current scope
        assert_eq!(env2.lookup(v2), Some(&Value::Int(100)));
    }

    #[test]
    fn env_extend_many() {
        let v1 = VarId::fresh();
        let v2 = VarId::fresh();
        let v3 = VarId::fresh();
        let env = Rc::new(Env::new());
        let env2 = env.extend_many(vec![(v1, Value::Int(1)), (v2, Value::Int(2)), (v3, Value::Int(3))]);

        assert_eq!(env2.lookup(v1), Some(&Value::Int(1)));
        assert_eq!(env2.lookup(v2), Some(&Value::Int(2)));
        assert_eq!(env2.lookup(v3), Some(&Value::Int(3)));
    }

    #[test]
    fn env_contains() {
        let v1 = VarId::fresh();
        let v2 = VarId::fresh();
        let v3 = VarId::fresh();
        let mut env = Env::new();
        env.bind(v1, Value::Int(42));
        let env = Rc::new(env);

        let env2 = env.extend(v2, Value::Int(100));

        assert!(env2.contains(v1));
        assert!(env2.contains(v2));
        assert!(!env2.contains(v3));
    }

    #[test]
    fn env_restrict() {
        let v1 = VarId::fresh();
        let v2 = VarId::fresh();
        let v3 = VarId::fresh();
        let mut env = Env::new();
        env.bind(v1, Value::Int(1));
        env.bind(v2, Value::Int(2));
        env.bind(v3, Value::Int(3));

        let restricted = env.restrict(&[v1, v3]);

        assert_eq!(restricted.lookup(v1), Some(&Value::Int(1)));
        assert_eq!(restricted.lookup(v2), None); // Not included
        assert_eq!(restricted.lookup(v3), Some(&Value::Int(3)));
    }

    #[test]
    fn env_restrict_with_parent() {
        let v1 = VarId::fresh();
        let v2 = VarId::fresh();
        let mut env = Env::new();
        env.bind(v1, Value::Int(1));
        let env = Rc::new(env);

        let env2 = env.extend(v2, Value::Int(2));

        // restrict should look up through parent chain
        let restricted = env2.restrict(&[v1, v2]);

        assert_eq!(restricted.lookup(v1), Some(&Value::Int(1)));
        assert_eq!(restricted.lookup(v2), Some(&Value::Int(2)));
        // Restricted env is flat (no parent)
        assert!(restricted.parent.is_none());
    }
}
