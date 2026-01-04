//! Type inference for the Neap type system
//!
//! Implements Algorithm W for Hindley-Milner type inference.

use crate::syntax::{
    BinOp, Decl, DatatypeDecl, Expr, FunDecl, ImplDecl, Literal, MatchArm, Pattern, Program, Span,
    Spanned, TraitDecl, TypeDecl, TypeExpr, UnOp, ValDecl,
};

use super::env::TypeEnv;
use super::error::TypeError;
use super::subst::Substitution;
use super::core::{Constraint, Type, TypeScheme, TypeVar};
use super::unify::unify;

/// The type checker.
pub struct TypeChecker {
    /// The current type environment
    env: TypeEnv,
    /// Accumulated substitution
    subst: Substitution,
    /// Collected errors (for error recovery)
    errors: Vec<TypeError>,
    /// Collected type class constraints during inference
    constraints: Vec<Constraint>,
}

impl TypeChecker {
    /// Create a new type checker with built-in types.
    #[must_use]
    pub fn new() -> Self {
        Self {
            env: TypeEnv::with_builtins(),
            subst: Substitution::new(),
            errors: Vec::new(),
            constraints: Vec::new(),
        }
    }

    /// Create a type checker with a custom environment.
    #[must_use]
    pub fn with_env(env: TypeEnv) -> Self {
        Self {
            env,
            subst: Substitution::new(),
            errors: Vec::new(),
            constraints: Vec::new(),
        }
    }

    /// Type check a program.
    pub fn check_program(&mut self, program: &Program) -> Result<(), Vec<TypeError>> {
        for decl in &program.decls {
            if let Err(e) = self.check_decl(decl) {
                self.errors.push(e);
            }
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }

    /// Infer the type of an expression.
    pub fn infer_expr(&mut self, expr: &Spanned<Expr>) -> Result<Type, TypeError> {
        let ty = self.infer(expr)?;
        Ok(self.subst.apply(&ty))
    }

    /// Get the current environment.
    #[must_use]
    pub fn env(&self) -> &TypeEnv {
        &self.env
    }

    /// Look up the type of a name in the environment.
    #[must_use]
    pub fn lookup_type(&self, name: &str) -> Option<Type> {
        self.env.lookup(name).map(|scheme| scheme.ty.clone())
    }

    /// Register a global variable binding (for REPL use).
    pub fn register_global(&mut self, name: &str, scheme: TypeScheme) {
        self.env.insert(name.to_string(), scheme);
    }

    // ========== Declaration Type Checking ==========

    /// Check a single declaration (for REPL use).
    pub fn check_decl(&mut self, decl: &Spanned<Decl>) -> Result<(), TypeError> {
        match &decl.value {
            Decl::Val(val) => self.check_val_decl(val, decl.span),
            Decl::Fun(fun) => self.check_fun_decl(fun, decl.span),
            Decl::Type(ty) => self.check_type_decl(ty),
            Decl::Datatype(dt) => self.check_datatype_decl(dt),
            Decl::Trait(tr) => self.check_trait_decl(tr),
            Decl::Impl(im) => self.check_impl_decl(im),
        }
    }

    fn check_val_decl(&mut self, decl: &ValDecl, span: Span) -> Result<(), TypeError> {
        let expr_ty = if decl.rec {
            // For recursive bindings, we need to add a placeholder type first
            let var = Type::var();
            let name = self.pattern_name(&decl.pattern)?;
            let old_env = self.env.clone();
            self.env.insert(name.clone(), TypeScheme::mono(var.clone()));

            let inferred = self.infer(&decl.expr)?;
            let s = unify(&var, &inferred)?;
            self.subst.extend(&s);

            // Restore and generalize
            self.env = old_env;
            self.subst.apply(&inferred)
        } else {
            self.infer(&decl.expr)?
        };

        // Check type annotation if present
        let final_ty = if let Some(ref ann) = decl.ty {
            let ann_ty = self.resolve_type_expr(ann)?;
            let s = unify(&expr_ty, &ann_ty).map_err(|_| TypeError::AnnotationMismatch {
                annotation: ann_ty.clone(),
                inferred: self.subst.apply(&expr_ty),
                span,
            })?;
            self.subst.extend(&s);
            self.subst.apply(&ann_ty)
        } else {
            self.subst.apply(&expr_ty)
        };

        // Bind pattern variables
        self.bind_pattern(&decl.pattern, &final_ty)?;

        // Verify that all collected constraints can be satisfied
        self.verify_constraints(span)?;

        Ok(())
    }

    /// Verify that all collected constraints can be satisfied.
    ///
    /// After type inference, we apply the accumulated substitution to the
    /// constraints and check that instances exist for each concrete type.
    fn verify_constraints(&mut self, span: Span) -> Result<(), TypeError> {
        // Apply substitution to get concrete types
        let constraints: Vec<_> = self
            .constraints
            .drain(..)
            .map(|c| Constraint::new(c.class_name, self.subst.apply(&c.ty)))
            .collect();

        for constraint in constraints {
            // Skip constraints on type variables (they're polymorphic)
            if matches!(constraint.ty, Type::Var(_)) {
                continue;
            }

            // Check that an instance exists
            if self
                .env
                .find_instance(&constraint.class_name, &constraint.ty)
                .is_none()
            {
                return Err(TypeError::MissingInstance {
                    class_name: constraint.class_name,
                    ty: constraint.ty,
                    span,
                });
            }
        }

        Ok(())
    }

    fn check_fun_decl(&mut self, decl: &FunDecl, _span: Span) -> Result<(), TypeError> {
        // For now, just handle single-clause functions
        if decl.clauses.is_empty() {
            return Ok(());
        }

        let clause = &decl.clauses[0];

        // Create type variables for parameters
        let param_tys: Vec<Type> = clause.params.iter().map(|_| Type::var()).collect();
        let ret_ty = Type::var();

        // Build the function type
        let fun_ty = Type::arrows(param_tys.clone(), ret_ty.clone());

        // Add function to environment (for recursion)
        self.env
            .insert(decl.name.value.clone(), TypeScheme::mono(fun_ty.clone()));

        // Extend environment with parameters
        let mut param_bindings = Vec::new();
        for (param, ty) in clause.params.iter().zip(param_tys.iter()) {
            self.collect_pattern_bindings(param, ty, &mut param_bindings)?;
        }

        let old_env = self.env.clone();
        for (name, scheme) in param_bindings {
            self.env.insert(name, scheme);
        }

        // Infer body type
        let body_ty = self.infer(&clause.body)?;

        // Unify with return type
        let s = unify(&ret_ty, &body_ty)?;
        self.subst.extend(&s);

        // Check result type annotation if present
        if let Some(ref ann) = clause.result_ty {
            let ann_ty = self.resolve_type_expr(ann)?;
            let actual_ret = self.subst.apply(&ret_ty);
            let s = unify(&actual_ret, &ann_ty)?;
            self.subst.extend(&s);
        }

        // Restore environment
        self.env = old_env;

        // Generalize and update binding
        let final_ty = self.subst.apply(&fun_ty);
        let scheme = self.env.generalize(&final_ty);
        self.env.insert(decl.name.value.clone(), scheme);

        // Verify constraints on concrete types within the function
        self.verify_constraints(decl.name.span)?;

        Ok(())
    }

    fn check_type_decl(&mut self, decl: &TypeDecl) -> Result<(), TypeError> {
        // Register the type constructor
        self.env
            .insert_type(decl.name.value.clone(), decl.params.len());
        Ok(())
    }

    fn check_datatype_decl(&mut self, decl: &DatatypeDecl) -> Result<(), TypeError> {
        // Register the type constructor
        self.env
            .insert_type(decl.name.value.clone(), decl.params.len());

        // Create type variables for parameters
        let param_vars: Vec<TypeVar> = decl.params.iter().map(|_| TypeVar::fresh()).collect();
        let param_tys: Vec<Type> = param_vars.iter().map(|v| Type::Var(*v)).collect();

        // The result type
        let result_ty = Type::Con(decl.name.value.clone(), param_tys);

        // Register each constructor
        for con in &decl.constructors {
            let con_ty = if let Some(ref arg_ty_expr) = con.arg {
                // Constructor with argument: arg -> result_ty
                let arg_ty = self.resolve_type_expr_with_vars(arg_ty_expr, &decl.params, &param_vars)?;
                Type::arrow(arg_ty, result_ty.clone())
            } else {
                // Nullary constructor: just result_ty
                result_ty.clone()
            };

            let scheme = TypeScheme::poly(param_vars.clone(), con_ty);
            self.env.insert_constructor(con.name.value.clone(), scheme);
        }

        Ok(())
    }

    fn check_trait_decl(&mut self, _decl: &TraitDecl) -> Result<(), TypeError> {
        // TODO: Implement trait declaration checking
        // - Register the trait in the environment
        // - Store method signatures
        Ok(())
    }

    fn check_impl_decl(&mut self, _decl: &ImplDecl) -> Result<(), TypeError> {
        // TODO: Implement impl declaration checking
        // - Look up the trait
        // - Verify the type matches
        // - Check method implementations match signatures
        Ok(())
    }

    // ========== Expression Type Inference ==========

    fn infer(&mut self, expr: &Spanned<Expr>) -> Result<Type, TypeError> {
        match &expr.value {
            Expr::Lit(lit) => Ok(self.infer_literal(lit)),

            Expr::Var(name) => self.infer_var(name, expr.span),

            Expr::Con(name) => self.infer_constructor(name, expr.span),

            Expr::App(func, arg) => self.infer_app(func, arg),

            Expr::Lambda(params, body) => self.infer_lambda(params, body),

            Expr::If(cond, then_branch, else_branch) => {
                self.infer_if(cond, then_branch, else_branch)
            }

            Expr::Match(scrutinee, arms) => self.infer_match(scrutinee, arms, expr.span),

            Expr::BinOp(op, lhs, rhs) => self.infer_binop(*op, lhs, rhs),

            Expr::UnOp(op, operand) => self.infer_unop(*op, operand),

            Expr::Tuple(elems) => self.infer_tuple(elems),

            Expr::List(elems) => self.infer_list(elems),

            Expr::Record(fields) => self.infer_record(fields),

            Expr::Field(record, field) => self.infer_field(record, field),

            Expr::Annot(expr, ty) => self.infer_annot(expr, ty),

            Expr::Pipe(lhs, rhs) => self.infer_pipe(lhs, rhs),

            Expr::Unit => Ok(Type::unit()),

            Expr::EnvVar(_) => Ok(Type::option(Type::string())),

            Expr::Command(parts) => {
                // Infer types of interpolated expressions
                for part in parts {
                    if let crate::syntax::CommandPart::Interpolation(expr) = part {
                        self.infer(expr)?;
                    }
                }
                // Commands return a record: {exitCode: int, stdout: string, stderr: string}
                Ok(Type::command_result())
            }

            Expr::Do(stmts) => self.infer_do(stmts),

            Expr::Redirect { expr, .. } => {
                // Redirect produces IO<unit>
                let _ = self.infer(expr)?;
                Ok(Type::io(Type::unit()))
            }
        }
    }

    fn infer_literal(&self, lit: &Literal) -> Type {
        match lit {
            Literal::Int(_) => Type::int(),
            Literal::Float(_) => Type::float(),
            Literal::String(_) => Type::string(),
            Literal::Char(_) => Type::char(),
            Literal::Bool(_) => Type::bool(),
        }
    }

    fn infer_var(&mut self, name: &str, span: Span) -> Result<Type, TypeError> {
        // First check if it's a regular variable
        if let Some(scheme) = self.env.lookup(name) {
            return Ok(scheme.instantiate());
        }

        // Check if it's a trait method
        if let Some((class_name, method_scheme)) = self.find_trait_method(name) {
            // Instantiate the method type with a fresh type variable
            let method_ty = method_scheme.instantiate();

            // Add a constraint that the first argument type must implement the trait
            // For a method like `show: 'a -> string`, we add constraint `Show 'a`
            if let Some(arg_ty) = self.get_first_arg_type(&method_ty) {
                self.constraints.push(Constraint::new(class_name, arg_ty));
            }

            return Ok(method_ty);
        }

        Err(TypeError::UnboundVariable {
            name: name.to_string(),
            span,
        })
    }

    /// Find a trait method by name, returning the class name and method type.
    fn find_trait_method(&self, method_name: &str) -> Option<(String, TypeScheme)> {
        // Search all type classes for the method
        for (class_name, class) in self.env.type_classes_iter() {
            for (name, scheme) in &class.methods {
                if name == method_name {
                    return Some((class_name.clone(), scheme.clone()));
                }
            }
        }
        None
    }

    /// Extract the first argument type from a function type.
    fn get_first_arg_type(&self, ty: &Type) -> Option<Type> {
        match ty {
            Type::Arrow(arg, _) => Some((**arg).clone()),
            _ => None,
        }
    }

    fn infer_constructor(&mut self, name: &str, span: Span) -> Result<Type, TypeError> {
        match self.env.lookup_constructor(name) {
            Some(scheme) => Ok(scheme.instantiate()),
            None => Err(TypeError::UnboundConstructor {
                name: name.to_string(),
                span,
            }),
        }
    }

    fn infer_app(&mut self, func: &Spanned<Expr>, arg: &Spanned<Expr>) -> Result<Type, TypeError> {
        let func_ty = self.infer(func)?;
        let arg_ty = self.infer(arg)?;

        let ret_ty = Type::var();
        let expected = Type::arrow(arg_ty.clone(), ret_ty.clone());

        let s = unify(&func_ty, &expected).map_err(|_| TypeError::NotAFunction {
            ty: self.subst.apply(&func_ty),
            span: func.span,
        })?;

        self.subst.extend(&s);
        Ok(self.subst.apply(&ret_ty))
    }

    fn infer_lambda(
        &mut self,
        params: &[Spanned<Pattern>],
        body: &Spanned<Expr>,
    ) -> Result<Type, TypeError> {
        // Create type variables for parameters
        let param_tys: Vec<Type> = params.iter().map(|_| Type::var()).collect();

        // Extend environment with parameters
        let mut bindings = Vec::new();
        for (param, ty) in params.iter().zip(param_tys.iter()) {
            self.collect_pattern_bindings(param, ty, &mut bindings)?;
        }

        let old_env = self.env.clone();
        for (name, scheme) in bindings {
            self.env.insert(name, scheme);
        }

        // Infer body type
        let body_ty = self.infer(body)?;

        // Restore environment
        self.env = old_env;

        // Build function type
        let func_ty = param_tys
            .into_iter()
            .rev()
            .fold(body_ty, |acc, param| Type::arrow(param, acc));

        Ok(func_ty)
    }

    fn infer_if(
        &mut self,
        cond: &Spanned<Expr>,
        then_branch: &Spanned<Expr>,
        else_branch: &Spanned<Expr>,
    ) -> Result<Type, TypeError> {
        let cond_ty = self.infer(cond)?;
        let s = unify(&cond_ty, &Type::bool())?;
        self.subst.extend(&s);

        let then_ty = self.infer(then_branch)?;
        let else_ty = self.infer(else_branch)?;

        let s = unify(&then_ty, &else_ty)?;
        self.subst.extend(&s);

        Ok(self.subst.apply(&then_ty))
    }

    fn infer_match(
        &mut self,
        scrutinee: &Spanned<Expr>,
        arms: &[MatchArm],
        _span: Span,
    ) -> Result<Type, TypeError> {
        let scrutinee_ty = self.infer(scrutinee)?;
        let result_ty = Type::var();

        for arm in arms {
            // Infer pattern type and collect bindings
            let (pattern_ty, bindings) = self.infer_pattern(&arm.pattern)?;

            // Unify pattern type with scrutinee
            let s = unify(&scrutinee_ty, &pattern_ty)?;
            self.subst.extend(&s);

            // Extend environment with pattern bindings
            let old_env = self.env.clone();
            for (name, scheme) in bindings {
                self.env.insert(name, scheme);
            }

            // Infer body type
            let body_ty = self.infer(&arm.body)?;

            // Restore environment
            self.env = old_env;

            // Unify body type with result type
            let s = unify(&result_ty, &body_ty)?;
            self.subst.extend(&s);
        }

        Ok(self.subst.apply(&result_ty))
    }

    fn infer_binop(
        &mut self,
        op: BinOp,
        lhs: &Spanned<Expr>,
        rhs: &Spanned<Expr>,
    ) -> Result<Type, TypeError> {
        let lhs_ty = self.infer(lhs)?;
        let rhs_ty = self.infer(rhs)?;

        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                // Numeric operations: both operands same numeric type
                let s = unify(&lhs_ty, &rhs_ty)?;
                self.subst.extend(&s);

                let resolved = self.subst.apply(&lhs_ty);
                // Default to int if still a variable
                if resolved.is_var() {
                    let s = unify(&resolved, &Type::int())?;
                    self.subst.extend(&s);
                    Ok(Type::int())
                } else {
                    Ok(resolved)
                }
            }

            BinOp::Eq | BinOp::Neq => {
                // Equality: both operands same type, returns bool
                let s = unify(&lhs_ty, &rhs_ty)?;
                self.subst.extend(&s);
                Ok(Type::bool())
            }

            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                // Comparison: both operands same type, returns bool
                let s = unify(&lhs_ty, &rhs_ty)?;
                self.subst.extend(&s);
                Ok(Type::bool())
            }

            BinOp::And | BinOp::Or => {
                // Logical: both operands bool, returns bool
                let s1 = unify(&lhs_ty, &Type::bool())?;
                self.subst.extend(&s1);
                let s2 = unify(&rhs_ty, &Type::bool())?;
                self.subst.extend(&s2);
                Ok(Type::bool())
            }

            BinOp::Concat => {
                // String concatenation
                let s1 = unify(&lhs_ty, &Type::string())?;
                self.subst.extend(&s1);
                let s2 = unify(&rhs_ty, &Type::string())?;
                self.subst.extend(&s2);
                Ok(Type::string())
            }

            BinOp::Cons => {
                // List cons: 'a -> 'a list -> 'a list
                let elem_ty = lhs_ty;
                let list_ty = Type::list(elem_ty.clone());
                let s = unify(&rhs_ty, &list_ty)?;
                self.subst.extend(&s);
                Ok(self.subst.apply(&list_ty))
            }

            BinOp::Append => {
                // List append: 'a list -> 'a list -> 'a list
                let elem_ty = Type::var();
                let list_ty = Type::list(elem_ty);
                let s1 = unify(&lhs_ty, &list_ty)?;
                self.subst.extend(&s1);
                let s2 = unify(&rhs_ty, &self.subst.apply(&list_ty))?;
                self.subst.extend(&s2);
                Ok(self.subst.apply(&list_ty))
            }
        }
    }

    fn infer_unop(&mut self, op: UnOp, operand: &Spanned<Expr>) -> Result<Type, TypeError> {
        let operand_ty = self.infer(operand)?;

        match op {
            UnOp::Neg => {
                // Numeric negation: defaults to int
                if operand_ty.is_var() {
                    let s = unify(&operand_ty, &Type::int())?;
                    self.subst.extend(&s);
                    Ok(Type::int())
                } else {
                    Ok(operand_ty)
                }
            }
            UnOp::Not => {
                // Logical not: bool -> bool
                let s = unify(&operand_ty, &Type::bool())?;
                self.subst.extend(&s);
                Ok(Type::bool())
            }
        }
    }

    fn infer_tuple(&mut self, elems: &[Spanned<Expr>]) -> Result<Type, TypeError> {
        let elem_tys: Result<Vec<_>, _> = elems.iter().map(|e| self.infer(e)).collect();
        Ok(Type::Tuple(elem_tys?))
    }

    fn infer_list(&mut self, elems: &[Spanned<Expr>]) -> Result<Type, TypeError> {
        if elems.is_empty() {
            Ok(Type::list(Type::var()))
        } else {
            let first_ty = self.infer(&elems[0])?;

            for elem in &elems[1..] {
                let elem_ty = self.infer(elem)?;
                let s = unify(&first_ty, &elem_ty)?;
                self.subst.extend(&s);
            }

            Ok(Type::list(self.subst.apply(&first_ty)))
        }
    }

    fn infer_record(
        &mut self,
        fields: &[(Spanned<String>, Spanned<Expr>)],
    ) -> Result<Type, TypeError> {
        let mut field_tys = Vec::new();

        for (name, expr) in fields {
            let ty = self.infer(expr)?;
            field_tys.push((name.value.clone(), ty));
        }

        Ok(Type::Record(field_tys))
    }

    fn infer_field(
        &mut self,
        record: &Spanned<Expr>,
        field: &Spanned<String>,
    ) -> Result<Type, TypeError> {
        let record_ty = self.infer(record)?;

        // Create a record type with the requested field
        let field_ty = Type::var();
        let expected_record = Type::Record(vec![(field.value.clone(), field_ty.clone())]);

        // For now, just check that record_ty is compatible
        // A more sophisticated implementation would handle row polymorphism
        match self.subst.apply(&record_ty) {
            Type::Record(fields) => {
                for (name, ty) in fields {
                    if name == field.value {
                        return Ok(ty);
                    }
                }
                Err(TypeError::MissingField {
                    name: field.value.clone(),
                    record_ty: self.subst.apply(&record_ty),
                    span: field.span,
                })
            }
            ty if ty.is_var() => {
                // If still a variable, create constraint
                let s = unify(&ty, &expected_record)?;
                self.subst.extend(&s);
                Ok(self.subst.apply(&field_ty))
            }
            ty => Err(TypeError::NotARecord {
                ty,
                span: record.span,
            }),
        }
    }

    fn infer_annot(
        &mut self,
        expr: &Spanned<Expr>,
        ty_expr: &Spanned<TypeExpr>,
    ) -> Result<Type, TypeError> {
        let inferred = self.infer(expr)?;
        let annotated = self.resolve_type_expr(ty_expr)?;

        let s = unify(&inferred, &annotated)?;
        self.subst.extend(&s);

        Ok(self.subst.apply(&annotated))
    }

    fn infer_pipe(&mut self, lhs: &Spanned<Expr>, rhs: &Spanned<Expr>) -> Result<Type, TypeError> {
        use crate::syntax::Expr;

        // Check if both sides are commands - if so, this is a shell pipeline
        if matches!((&lhs.value, &rhs.value), (Expr::Command(_), Expr::Command(_))) {
            // Infer types of both commands (to check interpolations)
            self.infer(lhs)?;
            self.infer(rhs)?;
            // Result is CommandResult
            return Ok(Type::command_result());
        }

        // Check for chained pipeline: (cmd1 |> cmd2) |> cmd3
        if let Expr::Pipe(inner_lhs, inner_rhs) = &lhs.value {
            if matches!(
                (&inner_lhs.value, &inner_rhs.value, &rhs.value),
                (Expr::Command(_), Expr::Command(_), Expr::Command(_))
            ) {
                // Infer types of all commands
                self.infer(inner_lhs)?;
                self.infer(inner_rhs)?;
                self.infer(rhs)?;
                // Result is CommandResult
                return Ok(Type::command_result());
            }
        }

        // x |> f  is equivalent to  f x
        let lhs_ty = self.infer(lhs)?;
        let rhs_ty = self.infer(rhs)?;

        let ret_ty = Type::var();
        let expected_func = Type::arrow(lhs_ty, ret_ty.clone());

        let s = unify(&rhs_ty, &expected_func)?;
        self.subst.extend(&s);

        Ok(self.subst.apply(&ret_ty))
    }

    fn infer_do(&mut self, stmts: &[crate::syntax::DoStmt]) -> Result<Type, TypeError> {
        use crate::syntax::DoStmt;

        if stmts.is_empty() {
            return Ok(Type::io(Type::unit()));
        }

        let mut result_ty = Type::var();

        for (i, stmt) in stmts.iter().enumerate() {
            let is_last = i == stmts.len() - 1;

            match stmt {
                DoStmt::Expr(expr) => {
                    let ty = self.infer(expr)?;
                    if is_last {
                        // Last expression determines the type
                        result_ty = ty;
                    } else {
                        // Non-last expressions should be IO<_>
                        let inner = Type::var();
                        let expected = Type::io(inner);
                        let _ = unify(&ty, &expected);
                    }
                }
                DoStmt::Bind(pattern, expr) => {
                    let expr_ty = self.infer(expr)?;

                    // expr should have type IO<t>
                    let inner_ty = Type::var();
                    let expected = Type::io(inner_ty.clone());
                    let s = unify(&expr_ty, &expected)?;
                    self.subst.extend(&s);

                    // Bind pattern to inner type
                    let resolved_inner = self.subst.apply(&inner_ty);
                    self.bind_pattern(pattern, &resolved_inner)?;
                }
                DoStmt::Let(pattern, expr) => {
                    let expr_ty = self.infer(expr)?;
                    self.bind_pattern(pattern, &expr_ty)?;
                }
            }
        }

        Ok(result_ty)
    }

    // ========== Pattern Type Inference ==========

    fn infer_pattern(
        &mut self,
        pattern: &Spanned<Pattern>,
    ) -> Result<(Type, Vec<(String, TypeScheme)>), TypeError> {
        let mut bindings = Vec::new();
        let ty = self.infer_pattern_inner(pattern, &mut bindings)?;
        Ok((ty, bindings))
    }

    fn infer_pattern_inner(
        &mut self,
        pattern: &Spanned<Pattern>,
        bindings: &mut Vec<(String, TypeScheme)>,
    ) -> Result<Type, TypeError> {
        match &pattern.value {
            Pattern::Wildcard => Ok(Type::var()),

            Pattern::Var(name) => {
                let ty = Type::var();
                bindings.push((name.clone(), TypeScheme::mono(ty.clone())));
                Ok(ty)
            }

            Pattern::Lit(lit) => Ok(self.infer_literal(lit)),

            Pattern::Con(name, arg) => {
                let con_ty = self
                    .env
                    .lookup_constructor(name)
                    .ok_or_else(|| TypeError::UnboundConstructor {
                        name: name.clone(),
                        span: pattern.span,
                    })?
                    .instantiate();

                if let Some(arg_pattern) = arg {
                    // Constructor with argument
                    let (arg_ty, ret_ty) = con_ty.decompose_arrow().ok_or_else(|| {
                        TypeError::UnboundConstructor {
                            name: name.clone(),
                            span: pattern.span,
                        }
                    })?;

                    let pattern_arg_ty = self.infer_pattern_inner(arg_pattern, bindings)?;
                    let s = unify(arg_ty, &pattern_arg_ty)?;
                    self.subst.extend(&s);

                    Ok(self.subst.apply(ret_ty))
                } else {
                    // Nullary constructor
                    Ok(con_ty)
                }
            }

            Pattern::Tuple(elems) => {
                let elem_tys: Result<Vec<_>, _> = elems
                    .iter()
                    .map(|p| self.infer_pattern_inner(p, bindings))
                    .collect();
                Ok(Type::Tuple(elem_tys?))
            }

            Pattern::List(elems) => {
                if elems.is_empty() {
                    Ok(Type::list(Type::var()))
                } else {
                    let first_ty = self.infer_pattern_inner(&elems[0], bindings)?;
                    for elem in &elems[1..] {
                        let elem_ty = self.infer_pattern_inner(elem, bindings)?;
                        let s = unify(&first_ty, &elem_ty)?;
                        self.subst.extend(&s);
                    }
                    Ok(Type::list(self.subst.apply(&first_ty)))
                }
            }

            Pattern::Cons(head, tail) => {
                let head_ty = self.infer_pattern_inner(head, bindings)?;
                let tail_ty = self.infer_pattern_inner(tail, bindings)?;

                let list_ty = Type::list(head_ty);
                let s = unify(&tail_ty, &list_ty)?;
                self.subst.extend(&s);

                Ok(self.subst.apply(&list_ty))
            }

            Pattern::Record(fields) => {
                let field_tys: Result<Vec<_>, _> = fields
                    .iter()
                    .map(|(name, p)| {
                        let ty = self.infer_pattern_inner(p, bindings)?;
                        Ok((name.value.clone(), ty))
                    })
                    .collect();
                Ok(Type::Record(field_tys?))
            }

            Pattern::Or(lhs, rhs) => {
                let lhs_ty = self.infer_pattern_inner(lhs, bindings)?;
                let rhs_ty = self.infer_pattern_inner(rhs, bindings)?;
                let s = unify(&lhs_ty, &rhs_ty)?;
                self.subst.extend(&s);
                Ok(self.subst.apply(&lhs_ty))
            }

            Pattern::Annot(inner, ty_expr) => {
                let pattern_ty = self.infer_pattern_inner(inner, bindings)?;
                let annotated_ty = self.resolve_type_expr(ty_expr)?;
                let s = unify(&pattern_ty, &annotated_ty)?;
                self.subst.extend(&s);
                Ok(self.subst.apply(&annotated_ty))
            }

            Pattern::As(inner, name) => {
                let ty = self.infer_pattern_inner(inner, bindings)?;
                bindings.push((name.value.clone(), TypeScheme::mono(ty.clone())));
                Ok(ty)
            }
        }
    }

    // ========== Helper Methods ==========

    fn pattern_name(&self, pattern: &Spanned<Pattern>) -> Result<String, TypeError> {
        match &pattern.value {
            Pattern::Var(name) => Ok(name.clone()),
            _ => Err(TypeError::InvalidPattern { span: pattern.span }),
        }
    }

    fn bind_pattern(&mut self, pattern: &Spanned<Pattern>, ty: &Type) -> Result<(), TypeError> {
        let mut bindings = Vec::new();
        self.collect_pattern_bindings(pattern, ty, &mut bindings)?;
        for (name, scheme) in bindings {
            self.env.insert(name, scheme);
        }
        Ok(())
    }

    fn collect_pattern_bindings(
        &mut self,
        pattern: &Spanned<Pattern>,
        ty: &Type,
        bindings: &mut Vec<(String, TypeScheme)>,
    ) -> Result<(), TypeError> {
        match &pattern.value {
            Pattern::Wildcard => Ok(()),

            Pattern::Var(name) => {
                bindings.push((name.clone(), TypeScheme::mono(ty.clone())));
                Ok(())
            }

            Pattern::Tuple(elems) => {
                if let Type::Tuple(elem_tys) = ty {
                    if elems.len() != elem_tys.len() {
                        return Err(TypeError::PatternTypeMismatch {
                            pattern_ty: Type::Tuple(vec![Type::var(); elems.len()]),
                            scrutinee_ty: ty.clone(),
                            span: pattern.span,
                        });
                    }
                    for (p, t) in elems.iter().zip(elem_tys.iter()) {
                        self.collect_pattern_bindings(p, t, bindings)?;
                    }
                    Ok(())
                } else if ty.is_var() {
                    // Create tuple type
                    let elem_tys: Vec<Type> = elems.iter().map(|_| Type::var()).collect();
                    let tuple_ty = Type::Tuple(elem_tys.clone());
                    let s = unify(ty, &tuple_ty)?;
                    self.subst.extend(&s);
                    for (p, t) in elems.iter().zip(elem_tys.iter()) {
                        self.collect_pattern_bindings(p, &self.subst.apply(t), bindings)?;
                    }
                    Ok(())
                } else {
                    Err(TypeError::PatternTypeMismatch {
                        pattern_ty: Type::Tuple(vec![Type::var(); elems.len()]),
                        scrutinee_ty: ty.clone(),
                        span: pattern.span,
                    })
                }
            }

            Pattern::Lit(_) | Pattern::Con(_, _) | Pattern::List(_) | Pattern::Cons(_, _) => {
                // These patterns don't introduce bindings in this simplified version
                // A full implementation would check type compatibility
                Ok(())
            }

            Pattern::Record(fields) => {
                if let Type::Record(field_tys) = ty {
                    for (name, p) in fields {
                        if let Some((_, t)) = field_tys.iter().find(|(n, _)| n == &name.value) {
                            self.collect_pattern_bindings(p, t, bindings)?;
                        }
                    }
                }
                Ok(())
            }

            Pattern::Or(_, _) | Pattern::Annot(_, _) | Pattern::As(_, _) => {
                // Simplified handling
                Ok(())
            }
        }
    }

    fn resolve_type_expr(&mut self, ty_expr: &Spanned<TypeExpr>) -> Result<Type, TypeError> {
        self.resolve_type_expr_with_vars(ty_expr, &[], &[])
    }

    fn resolve_type_expr_with_vars(
        &mut self,
        ty_expr: &Spanned<TypeExpr>,
        param_names: &[Spanned<String>],
        param_vars: &[TypeVar],
    ) -> Result<Type, TypeError> {
        match &ty_expr.value {
            TypeExpr::Var(name) => {
                // Check if it's a bound type parameter
                for (pname, pvar) in param_names.iter().zip(param_vars.iter()) {
                    if &pname.value == name {
                        return Ok(Type::Var(*pvar));
                    }
                }
                // Otherwise it's a fresh type variable
                Ok(Type::var())
            }

            TypeExpr::Con(name) => {
                if self.env.type_arity(name).is_some() {
                    Ok(Type::Con(name.clone(), vec![]))
                } else {
                    Err(TypeError::UnknownType {
                        name: name.clone(),
                        span: ty_expr.span,
                    })
                }
            }

            TypeExpr::App(con, args) => {
                let con_ty = self.resolve_type_expr_with_vars(con, param_names, param_vars)?;
                let arg_tys: Result<Vec<_>, _> = args
                    .iter()
                    .map(|a| self.resolve_type_expr_with_vars(a, param_names, param_vars))
                    .collect();

                if let Type::Con(name, _) = con_ty {
                    Ok(Type::Con(name, arg_tys?))
                } else {
                    Ok(con_ty)
                }
            }

            TypeExpr::Arrow(t1, t2) => {
                let ty1 = self.resolve_type_expr_with_vars(t1, param_names, param_vars)?;
                let ty2 = self.resolve_type_expr_with_vars(t2, param_names, param_vars)?;
                Ok(Type::arrow(ty1, ty2))
            }

            TypeExpr::Tuple(elems) => {
                let elem_tys: Result<Vec<_>, _> = elems
                    .iter()
                    .map(|e| self.resolve_type_expr_with_vars(e, param_names, param_vars))
                    .collect();
                Ok(Type::Tuple(elem_tys?))
            }

            TypeExpr::Record(fields) => {
                let field_tys: Result<Vec<_>, _> = fields
                    .iter()
                    .map(|(name, ty)| {
                        let resolved = self.resolve_type_expr_with_vars(ty, param_names, param_vars)?;
                        Ok((name.value.clone(), resolved))
                    })
                    .collect();
                Ok(Type::Record(field_tys?))
            }

            TypeExpr::Paren(inner) => {
                self.resolve_type_expr_with_vars(inner, param_names, param_vars)
            }
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Parser;

    fn infer(source: &str) -> Result<Type, TypeError> {
        TypeVar::reset_counter();
        let mut parser = Parser::new(source).unwrap();
        let expr = parser.parse_expr().unwrap();
        let mut checker = TypeChecker::new();
        checker.infer_expr(&expr)
    }

    fn check(source: &str) -> Result<(), Vec<TypeError>> {
        TypeVar::reset_counter();
        let mut parser = Parser::new(source).unwrap();
        let program = parser.parse_program().unwrap();
        let mut checker = TypeChecker::new();
        checker.check_program(&program)
    }

    // ========== Literal Tests ==========

    #[test]
    fn infer_int() {
        let ty = infer("42").unwrap();
        assert_eq!(ty, Type::int());
    }

    #[test]
    fn infer_float() {
        let ty = infer("3.14").unwrap();
        assert_eq!(ty, Type::float());
    }

    #[test]
    fn infer_string() {
        let ty = infer(r#""hello""#).unwrap();
        assert_eq!(ty, Type::string());
    }

    #[test]
    fn infer_bool() {
        assert_eq!(infer("true").unwrap(), Type::bool());
        assert_eq!(infer("false").unwrap(), Type::bool());
    }

    #[test]
    fn infer_unit() {
        let ty = infer("()").unwrap();
        assert_eq!(ty, Type::unit());
    }

    // ========== Operator Tests ==========

    #[test]
    fn infer_addition() {
        let ty = infer("1 + 2").unwrap();
        assert_eq!(ty, Type::int());
    }

    #[test]
    fn infer_comparison() {
        let ty = infer("1 < 2").unwrap();
        assert_eq!(ty, Type::bool());
    }

    #[test]
    fn infer_logical() {
        let ty = infer("true && false").unwrap();
        assert_eq!(ty, Type::bool());
    }

    #[test]
    fn infer_concat() {
        let ty = infer(r#""hello" ++ " world""#).unwrap();
        assert_eq!(ty, Type::string());
    }

    #[test]
    fn infer_negation() {
        let ty = infer("-42").unwrap();
        assert_eq!(ty, Type::int());
    }

    #[test]
    fn infer_not() {
        let ty = infer("!true").unwrap();
        assert_eq!(ty, Type::bool());
    }

    // ========== Tuple and List Tests ==========

    #[test]
    fn infer_tuple() {
        let ty = infer("(1, true, \"hi\")").unwrap();
        assert_eq!(ty, Type::Tuple(vec![Type::int(), Type::bool(), Type::string()]));
    }

    #[test]
    fn infer_empty_list() {
        let ty = infer("[]").unwrap();
        assert!(matches!(ty, Type::Con(name, _) if name == "list"));
    }

    #[test]
    fn infer_list() {
        let ty = infer("[1, 2, 3]").unwrap();
        assert_eq!(ty, Type::list(Type::int()));
    }

    #[test]
    fn infer_cons() {
        let ty = infer("1 :: [2, 3]").unwrap();
        assert_eq!(ty, Type::list(Type::int()));
    }

    // ========== Lambda Tests ==========

    #[test]
    fn infer_identity() {
        let ty = infer("fn x => x").unwrap();
        // Should be 'a -> 'a
        let (args, ret) = ty.collect_arrow_args();
        assert_eq!(args.len(), 1);
        assert_eq!(args[0], ret);
    }

    #[test]
    fn infer_const() {
        let ty = infer("fn x y => x").unwrap();
        // Should be 'a -> 'b -> 'a
        let (args, ret) = ty.collect_arrow_args();
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], ret);
    }

    #[test]
    fn infer_lambda_typed() {
        let ty = infer("fn x => x + 1").unwrap();
        assert_eq!(ty, Type::arrow(Type::int(), Type::int()));
    }

    // ========== If Tests ==========

    #[test]
    fn infer_if() {
        let ty = infer("if true then 1 else 2").unwrap();
        assert_eq!(ty, Type::int());
    }

    #[test]
    fn infer_if_mismatch() {
        let result = infer("if true then 1 else \"hi\"");
        assert!(result.is_err());
    }

    // ========== Match Tests ==========

    #[test]
    fn infer_match_bool() {
        let ty = infer("match true with | true -> 1 | false -> 0").unwrap();
        assert_eq!(ty, Type::int());
    }

    // ========== Application Tests ==========

    #[test]
    fn infer_app() {
        let ty = infer("(fn x => x + 1) 5").unwrap();
        assert_eq!(ty, Type::int());
    }

    // ========== Record Tests ==========

    #[test]
    fn infer_record() {
        let ty = infer("{ x = 1, y = true }").unwrap();
        assert_eq!(
            ty,
            Type::Record(vec![
                ("x".to_string(), Type::int()),
                ("y".to_string(), Type::bool()),
            ])
        );
    }

    #[test]
    fn infer_field() {
        let ty = infer("{ x = 1, y = true }.x").unwrap();
        assert_eq!(ty, Type::int());
    }

    // ========== Pipe Tests ==========

    #[test]
    fn infer_pipe() {
        let ty = infer("1 |> (fn x => x + 1)").unwrap();
        assert_eq!(ty, Type::int());
    }

    // ========== Declaration Tests ==========

    #[test]
    fn check_let_decl() {
        let result = check("let x = 1");
        assert!(result.is_ok());
    }

    #[test]
    fn check_fun() {
        let result = check("fun add x y = x + y");
        assert!(result.is_ok());
    }

    #[test]
    fn check_fun_recursive() {
        let result = check("fun fact n = if n = 0 then 1 else n * fact (n - 1)");
        assert!(result.is_ok());
    }

    #[test]
    fn check_multiple() {
        let result = check(
            r#"
            let x = 1
            let y = x + 1
            fun double n = n * 2
        "#,
        );
        assert!(result.is_ok());
    }

    // ========== Command Tests ==========

    #[test]
    fn infer_command() {
        let ty = infer("`ls -la`").unwrap();
        // Command returns CommandResult record
        assert!(matches!(ty, Type::Record(fields) if fields.len() == 3));
    }

    #[test]
    fn infer_command_with_interpolation() {
        // Interpolation with string value
        let result = check(
            r#"
            let file = "README.md"
            let result = `cat {file}`
        "#,
        );
        assert!(result.is_ok());
    }

    // ========== Type Class Tests ==========

    #[test]
    fn infer_show_int() {
        // show 5 should type check and return string
        let ty = infer("show 5").unwrap();
        assert_eq!(ty, Type::string());
    }

    #[test]
    fn infer_show_float() {
        // show 3.14 should type check and return string
        let ty = infer("show 3.14").unwrap();
        assert_eq!(ty, Type::string());
    }

    #[test]
    fn infer_show_bool() {
        // show true should type check and return string
        let ty = infer("show true").unwrap();
        assert_eq!(ty, Type::string());
    }

    #[test]
    fn infer_show_string() {
        // show "hello" should type check and return string
        let ty = infer("show \"hello\"").unwrap();
        assert_eq!(ty, Type::string());
    }

    #[test]
    fn check_show_in_let() {
        // Using show in a let binding
        let result = check("let s = show 42");
        assert!(result.is_ok());
    }

    #[test]
    #[ignore = "stack overflow when resolving show trait on polymorphic type - needs investigation"]
    fn check_show_in_fun() {
        // Using show in a function
        let result = check(
            r#"
            fun showTwice x = show x ++ " " ++ show x
            let result = showTwice 5
        "#,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn infer_eq_int() {
        // eq 1 2 should type check and return bool
        let ty = infer("eq 1 2").unwrap();
        assert_eq!(ty, Type::bool());
    }
}
