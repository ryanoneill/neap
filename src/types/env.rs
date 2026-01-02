//! Type environment for the Neap type system
//!
//! The type environment maps identifiers to their type schemes and
//! manages scoping during type inference.

use std::collections::{HashMap, HashSet};

use super::subst::Substitution;
use super::core::{ClassInstance, Type, TypeClass, TypeScheme, TypeVar};

/// A type environment mapping identifiers to type schemes.
#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    /// Variable bindings (name -> type scheme)
    bindings: HashMap<String, TypeScheme>,

    /// Type constructor arities (name -> number of type parameters)
    type_arities: HashMap<String, usize>,

    /// Data constructor types (constructor name -> type scheme)
    constructors: HashMap<String, TypeScheme>,

    /// Type class definitions (class name -> definition)
    type_classes: HashMap<String, TypeClass>,

    /// Type class instances
    instances: Vec<ClassInstance>,
}

impl TypeEnv {
    /// Create a new empty type environment.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            type_arities: HashMap::new(),
            constructors: HashMap::new(),
            type_classes: HashMap::new(),
            instances: Vec::new(),
        }
    }

    /// Create a type environment with built-in types and functions.
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut env = Self::new();
        env.add_builtins();
        env
    }

    /// Add built-in types and functions.
    fn add_builtins(&mut self) {
        // Built-in type arities
        self.type_arities.insert("unit".to_string(), 0);
        self.type_arities.insert("bool".to_string(), 0);
        self.type_arities.insert("int".to_string(), 0);
        self.type_arities.insert("float".to_string(), 0);
        self.type_arities.insert("string".to_string(), 0);
        self.type_arities.insert("char".to_string(), 0);
        self.type_arities.insert("list".to_string(), 1);
        self.type_arities.insert("option".to_string(), 1);
        self.type_arities.insert("result".to_string(), 2);
        self.type_arities.insert("io".to_string(), 1);
        self.type_arities.insert("stream".to_string(), 1);

        // Boolean constructors
        self.constructors
            .insert("true".to_string(), TypeScheme::mono(Type::bool()));
        self.constructors
            .insert("false".to_string(), TypeScheme::mono(Type::bool()));

        // Option constructors
        let a = TypeVar::fresh();
        self.constructors.insert(
            "None".to_string(),
            TypeScheme::poly(vec![a], Type::option(Type::Var(a))),
        );

        let a = TypeVar::fresh();
        self.constructors.insert(
            "Some".to_string(),
            TypeScheme::poly(
                vec![a],
                Type::arrow(Type::Var(a), Type::option(Type::Var(a))),
            ),
        );

        // Result constructors
        let a = TypeVar::fresh();
        let e = TypeVar::fresh();
        self.constructors.insert(
            "Ok".to_string(),
            TypeScheme::poly(
                vec![a, e],
                Type::arrow(Type::Var(a), Type::result(Type::Var(a), Type::Var(e))),
            ),
        );

        let a = TypeVar::fresh();
        let e = TypeVar::fresh();
        self.constructors.insert(
            "Err".to_string(),
            TypeScheme::poly(
                vec![a, e],
                Type::arrow(Type::Var(e), Type::result(Type::Var(a), Type::Var(e))),
            ),
        );

        // List constructors
        let a = TypeVar::fresh();
        self.constructors.insert(
            "Nil".to_string(),
            TypeScheme::poly(vec![a], Type::list(Type::Var(a))),
        );

        let a = TypeVar::fresh();
        self.constructors.insert(
            "Cons".to_string(),
            TypeScheme::poly(
                vec![a],
                Type::arrow(
                    Type::Var(a),
                    Type::arrow(Type::list(Type::Var(a)), Type::list(Type::Var(a))),
                ),
            ),
        );

        // Built-in functions

        // Arithmetic: int -> int -> int
        for name in &["__add_int", "__sub_int", "__mul_int", "__div_int", "__mod_int"] {
            self.bindings.insert(
                (*name).to_string(),
                TypeScheme::mono(Type::arrows(vec![Type::int(), Type::int()], Type::int())),
            );
        }

        // Float arithmetic: float -> float -> float
        for name in &["__add_float", "__sub_float", "__mul_float", "__div_float"] {
            self.bindings.insert(
                (*name).to_string(),
                TypeScheme::mono(Type::arrows(vec![Type::float(), Type::float()], Type::float())),
            );
        }

        // Comparison: 'a -> 'a -> bool
        let a = TypeVar::fresh();
        for name in &["__eq", "__neq", "__lt", "__le", "__gt", "__ge"] {
            self.bindings.insert(
                (*name).to_string(),
                TypeScheme::poly(
                    vec![a],
                    Type::arrows(vec![Type::Var(a), Type::Var(a)], Type::bool()),
                ),
            );
        }

        // String concatenation: string -> string -> string
        self.bindings.insert(
            "__concat".to_string(),
            TypeScheme::mono(Type::arrows(vec![Type::string(), Type::string()], Type::string())),
        );

        // List operations
        let a = TypeVar::fresh();
        self.bindings.insert(
            "__cons".to_string(),
            TypeScheme::poly(
                vec![a],
                Type::arrows(
                    vec![Type::Var(a), Type::list(Type::Var(a))],
                    Type::list(Type::Var(a)),
                ),
            ),
        );

        let a = TypeVar::fresh();
        self.bindings.insert(
            "__append".to_string(),
            TypeScheme::poly(
                vec![a],
                Type::arrows(
                    vec![Type::list(Type::Var(a)), Type::list(Type::Var(a))],
                    Type::list(Type::Var(a)),
                ),
            ),
        );

        // Negation: int -> int
        self.bindings.insert(
            "__neg_int".to_string(),
            TypeScheme::mono(Type::arrow(Type::int(), Type::int())),
        );

        // Float negation: float -> float
        self.bindings.insert(
            "__neg_float".to_string(),
            TypeScheme::mono(Type::arrow(Type::float(), Type::float())),
        );

        // Logical not: bool -> bool
        self.bindings.insert(
            "__not".to_string(),
            TypeScheme::mono(Type::arrow(Type::bool(), Type::bool())),
        );

        // Print functions
        let a = TypeVar::fresh();
        self.bindings.insert(
            "print".to_string(),
            TypeScheme::poly(vec![a], Type::arrow(Type::Var(a), Type::io(Type::unit()))),
        );

        self.bindings.insert(
            "println".to_string(),
            TypeScheme::mono(Type::arrow(Type::string(), Type::io(Type::unit()))),
        );

        // IO functions
        self.bindings.insert(
            "readLine".to_string(),
            TypeScheme::mono(Type::io(Type::string())),
        );

        self.bindings.insert(
            "readFile".to_string(),
            TypeScheme::mono(Type::arrow(Type::string(), Type::io(Type::string()))),
        );

        self.bindings.insert(
            "writeFile".to_string(),
            TypeScheme::mono(Type::arrows(
                vec![Type::string(), Type::string()],
                Type::io(Type::unit()),
            )),
        );

        // Environment variable access
        self.bindings.insert(
            "getEnv".to_string(),
            TypeScheme::mono(Type::arrow(Type::string(), Type::io(Type::option(Type::string())))),
        );

        // Conversion functions
        self.bindings.insert(
            "intToFloat".to_string(),
            TypeScheme::mono(Type::arrow(Type::int(), Type::float())),
        );

        self.bindings.insert(
            "floatToInt".to_string(),
            TypeScheme::mono(Type::arrow(Type::float(), Type::int())),
        );

        self.bindings.insert(
            "intToString".to_string(),
            TypeScheme::mono(Type::arrow(Type::int(), Type::string())),
        );

        self.bindings.insert(
            "floatToString".to_string(),
            TypeScheme::mono(Type::arrow(Type::float(), Type::string())),
        );

        self.bindings.insert(
            "charToString".to_string(),
            TypeScheme::mono(Type::arrow(Type::char(), Type::string())),
        );

        self.bindings.insert(
            "charToInt".to_string(),
            TypeScheme::mono(Type::arrow(Type::char(), Type::int())),
        );

        self.bindings.insert(
            "intToChar".to_string(),
            TypeScheme::mono(Type::arrow(Type::int(), Type::char())),
        );

        // String operations
        self.bindings.insert(
            "stringLength".to_string(),
            TypeScheme::mono(Type::arrow(Type::string(), Type::int())),
        );

        self.bindings.insert(
            "charAt".to_string(),
            TypeScheme::mono(Type::arrow(
                Type::Tuple(vec![Type::string(), Type::int()]),
                Type::char(),
            )),
        );

        self.bindings.insert(
            "substring".to_string(),
            TypeScheme::mono(Type::arrow(
                Type::Tuple(vec![Type::string(), Type::int(), Type::int()]),
                Type::string(),
            )),
        );

        // List operations
        let a = TypeVar::fresh();
        self.bindings.insert(
            "listLength".to_string(),
            TypeScheme::poly(vec![a], Type::arrow(Type::list(Type::Var(a)), Type::int())),
        );

        // Print without newline
        let a = TypeVar::fresh();
        self.bindings.insert(
            "printNoNewline".to_string(),
            TypeScheme::poly(vec![a], Type::arrow(Type::Var(a), Type::io(Type::unit()))),
        );

        // Assertions
        self.bindings.insert(
            "assert".to_string(),
            TypeScheme::mono(Type::arrow(Type::bool(), Type::io(Type::unit()))),
        );

        let a = TypeVar::fresh();
        self.bindings.insert(
            "panic".to_string(),
            TypeScheme::poly(vec![a], Type::arrow(Type::string(), Type::Var(a))),
        );
    }

    // ========== Variable Bindings ==========

    /// Look up a variable's type scheme.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&TypeScheme> {
        self.bindings.get(name)
    }

    /// Insert a variable binding.
    pub fn insert(&mut self, name: String, scheme: TypeScheme) {
        self.bindings.insert(name, scheme);
    }

    /// Remove a variable binding.
    pub fn remove(&mut self, name: &str) -> Option<TypeScheme> {
        self.bindings.remove(name)
    }

    /// Check if a variable is bound.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.bindings.contains_key(name)
    }

    /// Create a new environment that extends this one.
    ///
    /// The new environment shadows bindings from this one.
    #[must_use]
    pub fn extend(&self, bindings: Vec<(String, TypeScheme)>) -> Self {
        let mut new_env = self.clone();
        for (name, scheme) in bindings {
            new_env.insert(name, scheme);
        }
        new_env
    }

    // ========== Type Constructors ==========

    /// Look up the arity of a type constructor.
    #[must_use]
    pub fn type_arity(&self, name: &str) -> Option<usize> {
        self.type_arities.get(name).copied()
    }

    /// Register a new type constructor.
    pub fn insert_type(&mut self, name: String, arity: usize) {
        self.type_arities.insert(name, arity);
    }

    // ========== Data Constructors ==========

    /// Look up a data constructor's type scheme.
    #[must_use]
    pub fn lookup_constructor(&self, name: &str) -> Option<&TypeScheme> {
        self.constructors.get(name)
    }

    /// Insert a data constructor.
    pub fn insert_constructor(&mut self, name: String, scheme: TypeScheme) {
        self.constructors.insert(name, scheme);
    }

    // ========== Type Class Operations ==========

    /// Register a type class.
    pub fn insert_class(&mut self, class: TypeClass) {
        self.type_classes.insert(class.name.clone(), class);
    }

    /// Look up a type class by name.
    #[must_use]
    pub fn lookup_class(&self, name: &str) -> Option<&TypeClass> {
        self.type_classes.get(name)
    }

    /// Check if a type class exists.
    #[must_use]
    pub fn has_class(&self, name: &str) -> bool {
        self.type_classes.contains_key(name)
    }

    /// Register a class instance.
    pub fn insert_instance(&mut self, instance: ClassInstance) {
        self.instances.push(instance);
    }

    /// Find an instance of a type class for a specific type.
    ///
    /// Returns the instance if found, or None if no matching instance exists.
    #[must_use]
    pub fn find_instance(&self, class_name: &str, ty: &Type) -> Option<&ClassInstance> {
        self.instances
            .iter()
            .find(|i| i.class_name == class_name && self.types_match(&i.for_type, ty))
    }

    /// Check if two types match (simple equality for now).
    ///
    /// In a more complete implementation, this would handle type variable
    /// matching and more sophisticated unification.
    fn types_match(&self, instance_ty: &Type, query_ty: &Type) -> bool {
        // For simple types, use equality
        // This is a simplified implementation; a full implementation would
        // need to handle polymorphic instances (e.g., Show for list 'a)
        match (instance_ty, query_ty) {
            (Type::Con(name1, args1), Type::Con(name2, args2)) => {
                name1 == name2
                    && args1.len() == args2.len()
                    && args1
                        .iter()
                        .zip(args2.iter())
                        .all(|(a, b)| self.types_match(a, b))
            }
            (Type::Var(_), _) => {
                // Instance type variables match anything
                true
            }
            _ => instance_ty == query_ty,
        }
    }

    /// Get all instances for a given type class.
    #[must_use]
    pub fn instances_for_class(&self, class_name: &str) -> Vec<&ClassInstance> {
        self.instances
            .iter()
            .filter(|i| i.class_name == class_name)
            .collect()
    }

    /// Look up a method in a type class.
    #[must_use]
    pub fn lookup_method(&self, class_name: &str, method_name: &str) -> Option<&TypeScheme> {
        self.type_classes.get(class_name).and_then(|class| {
            class
                .methods
                .iter()
                .find(|(name, _)| name == method_name)
                .map(|(_, scheme)| scheme)
        })
    }

    // ========== Type Scheme Operations ==========

    /// Get all free type variables in the environment.
    #[must_use]
    pub fn free_vars(&self) -> HashSet<TypeVar> {
        let mut vars = HashSet::new();
        for scheme in self.bindings.values() {
            vars.extend(scheme.free_vars());
        }
        vars
    }

    /// Generalize a type into a type scheme.
    ///
    /// Quantifies over all type variables that are free in the type
    /// but not free in the environment.
    #[must_use]
    pub fn generalize(&self, ty: &Type) -> TypeScheme {
        let env_vars = self.free_vars();
        let ty_vars = ty.free_vars();

        let quantified: Vec<TypeVar> = ty_vars.difference(&env_vars).copied().collect();

        if quantified.is_empty() {
            TypeScheme::mono(ty.clone())
        } else {
            TypeScheme::poly(quantified, ty.clone())
        }
    }

    /// Apply a substitution to the entire environment.
    #[must_use]
    pub fn apply_subst(&self, subst: &Substitution) -> Self {
        let mut new_env = self.clone();
        for scheme in new_env.bindings.values_mut() {
            *scheme = subst.apply_scheme(scheme);
        }
        new_env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_empty() {
        let env = TypeEnv::new();
        assert!(env.lookup("x").is_none());
    }

    #[test]
    fn env_insert_lookup() {
        let mut env = TypeEnv::new();
        env.insert("x".to_string(), TypeScheme::mono(Type::int()));

        let scheme = env.lookup("x").unwrap();
        assert_eq!(scheme.ty, Type::int());
    }

    #[test]
    fn env_extend() {
        let mut env = TypeEnv::new();
        env.insert("x".to_string(), TypeScheme::mono(Type::int()));

        let extended = env.extend(vec![
            ("y".to_string(), TypeScheme::mono(Type::string())),
            ("x".to_string(), TypeScheme::mono(Type::bool())), // shadows
        ]);

        // Original env unchanged
        assert_eq!(env.lookup("x").unwrap().ty, Type::int());
        assert!(env.lookup("y").is_none());

        // Extended env has new bindings
        assert_eq!(extended.lookup("x").unwrap().ty, Type::bool());
        assert_eq!(extended.lookup("y").unwrap().ty, Type::string());
    }

    #[test]
    fn env_generalize_no_free() {
        let env = TypeEnv::new();
        TypeVar::reset_counter();
        let v = TypeVar::fresh();

        // 'a -> 'a with empty env: should generalize 'a
        let ty = Type::arrow(Type::Var(v), Type::Var(v));
        let scheme = env.generalize(&ty);

        assert_eq!(scheme.vars.len(), 1);
        assert!(scheme.vars.contains(&v));
    }

    #[test]
    fn env_generalize_with_env_var() {
        let mut env = TypeEnv::new();
        TypeVar::reset_counter();
        let v1 = TypeVar::fresh();
        let v2 = TypeVar::fresh();

        // Environment has 'a
        env.insert("x".to_string(), TypeScheme::mono(Type::Var(v1)));

        // 'a -> 'b with 'a in env: should only generalize 'b
        let ty = Type::arrow(Type::Var(v1), Type::Var(v2));
        let scheme = env.generalize(&ty);

        assert_eq!(scheme.vars.len(), 1);
        assert!(scheme.vars.contains(&v2));
        assert!(!scheme.vars.contains(&v1));
    }

    #[test]
    fn env_apply_subst() {
        let mut env = TypeEnv::new();
        TypeVar::reset_counter();
        let v = TypeVar::fresh();

        env.insert("x".to_string(), TypeScheme::mono(Type::Var(v)));

        let subst = Substitution::single(v, Type::int());
        let new_env = env.apply_subst(&subst);

        assert_eq!(new_env.lookup("x").unwrap().ty, Type::int());
    }

    #[test]
    fn env_builtins() {
        let env = TypeEnv::with_builtins();

        // Check that built-in types are registered
        assert_eq!(env.type_arity("int"), Some(0));
        assert_eq!(env.type_arity("list"), Some(1));
        assert_eq!(env.type_arity("result"), Some(2));

        // Check that constructors are registered
        assert!(env.lookup_constructor("None").is_some());
        assert!(env.lookup_constructor("Some").is_some());
        assert!(env.lookup_constructor("Ok").is_some());
        assert!(env.lookup_constructor("Err").is_some());
    }
}
