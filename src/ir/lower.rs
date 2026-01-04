//! AST to IR lowering
//!
//! This module converts typed AST to IR in A-Normal Form.
//! The lowering process:
//! - Converts all expressions to ANF (intermediate values named)
//! - Desugars operators to primitive calls
//! - Converts patterns to IR patterns
//! - Specializes polymorphic operations based on types

use std::collections::HashMap;

use crate::syntax::{
    BinOp, CommandPart, Decl, DoStmt, Expr, FunDecl, Literal, MatchArm, Pattern, Program, Spanned,
    UnOp, ValDecl,
};
use crate::types::{Type, TypeChecker, TypeError};

use super::core::{
    IRCommandPart, IRDecl, IRExpr, IRLiteral, IRPattern, IRProgram, Primitive, VarId,
};

/// Errors that can occur during lowering.
#[derive(Debug, Clone)]
pub enum LowerError {
    /// Type error from type checking phase
    TypeError(Vec<TypeError>),

    /// Unsupported feature
    Unsupported(String),

    /// Internal error
    Internal(String),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TypeError(errors) => {
                write!(f, "type errors: ")?;
                for (i, e) in errors.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{e}")?;
                }
                Ok(())
            }
            Self::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for LowerError {}

/// The lowering context.
pub struct Lower {
    /// Type checker for inferring expression types
    checker: TypeChecker,

    /// Variable name to VarId mapping
    vars: HashMap<String, VarId>,

    /// Type of each variable
    var_types: HashMap<VarId, Type>,
}

impl Lower {
    /// Create a new lowering context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            checker: TypeChecker::new(),
            vars: HashMap::new(),
            var_types: HashMap::new(),
        }
    }

    /// Register a global variable binding (for REPL use).
    ///
    /// This adds the binding to the internal type checker's environment
    /// so that `expr_type` can resolve it during lowering.
    pub fn register_global(&mut self, name: &str, ty: Type) {
        use crate::types::TypeScheme;
        self.checker.register_global(name, TypeScheme::mono(ty));
    }

    /// Get the type of an expression using local bindings.
    ///
    /// For simple expressions, computes the type directly.
    /// For complex expressions like pipes, falls back to the type checker.
    fn expr_type(&mut self, expr: &Spanned<Expr>) -> Result<Type, LowerError> {
        match &expr.value {
            Expr::Lit(lit) => Ok(match lit {
                Literal::Int(_) => Type::int(),
                Literal::Float(_) => Type::float(),
                Literal::String(_) => Type::string(),
                Literal::Char(_) => Type::char(),
                Literal::Bool(_) => Type::bool(),
            }),

            Expr::Var(name) => {
                // First check local variables
                if let Some(&var_id) = self.vars.get(name)
                    && let Some(ty) = self.var_types.get(&var_id)
                {
                    return Ok(ty.clone());
                }
                // Fall back to global environment
                self.checker
                    .env()
                    .lookup(name)
                    .map(|s| s.instantiate())
                    .ok_or_else(|| LowerError::Internal(format!("variable {name} not found")))
            }

            Expr::Con(name) => {
                self.checker
                    .env()
                    .lookup_constructor(name)
                    .map(|s| s.instantiate())
                    .ok_or_else(|| {
                        LowerError::Internal(format!("constructor {name} not found"))
                    })
            }

            Expr::Unit => Ok(Type::unit()),

            Expr::Tuple(elems) => {
                let mut elem_tys = Vec::new();
                for e in elems {
                    elem_tys.push(self.expr_type(e)?);
                }
                Ok(Type::Tuple(elem_tys))
            }

            Expr::BinOp(op, lhs, _) => {
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        self.expr_type(lhs) // Same as operand type
                    }
                    BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        Ok(Type::bool())
                    }
                    BinOp::And | BinOp::Or => Ok(Type::bool()),
                    BinOp::Concat => Ok(Type::string()),
                    BinOp::Cons | BinOp::Append => self.expr_type(lhs),
                }
            }

            Expr::UnOp(op, operand) => {
                match op {
                    UnOp::Not => Ok(Type::bool()),
                    UnOp::Neg => self.expr_type(operand),
                }
            }

            Expr::If(_, then_branch, _) => self.expr_type(then_branch),

            Expr::Annot(_, ty_expr) => {
                // For now, just return a placeholder - type annotations
                // should have been resolved during type checking
                self.type_expr_to_type(ty_expr)
            }

            Expr::Command(_) => Ok(Type::command_result()),

            Expr::Field(record, field) => {
                let record_ty = self.expr_type(record)?;
                if let Type::Record(fields) = record_ty {
                    for (f, ty) in fields {
                        if f == field.value {
                            return Ok(ty);
                        }
                    }
                    Err(LowerError::Internal(format!(
                        "field {} not found",
                        field.value
                    )))
                } else {
                    Err(LowerError::Internal(format!(
                        "expected record type for field access, got {:?}",
                        record_ty
                    )))
                }
            }

            Expr::Pipe(lhs, rhs) => {
                // Check if both sides are commands - this is a shell pipeline
                if matches!((&lhs.value, &rhs.value), (Expr::Command(_), Expr::Command(_))) {
                    return Ok(Type::command_result());
                }
                // Check for chained pipeline
                if let Expr::Pipe(inner_lhs, inner_rhs) = &lhs.value {
                    if matches!(
                        (&inner_lhs.value, &inner_rhs.value, &rhs.value),
                        (Expr::Command(_), Expr::Command(_), Expr::Command(_))
                    ) {
                        return Ok(Type::command_result());
                    }
                }
                // For function pipes (lhs |> rhs = rhs(lhs)), use the type checker
                // to infer the full type
                self.checker
                    .infer_expr(expr)
                    .map_err(|e| LowerError::TypeError(vec![e]))
            }

            // For complex expressions, we need more context
            // Return an error so caller uses expected type instead
            _ => Err(LowerError::Internal(
                "cannot determine expression type".to_string(),
            )),
        }
    }

    /// Convert a TypeExpr to a Type (simple cases only).
    fn type_expr_to_type(&self, ty_expr: &Spanned<crate::syntax::TypeExpr>) -> Result<Type, LowerError> {
        use crate::syntax::TypeExpr;
        match &ty_expr.value {
            TypeExpr::Con(name) => {
                match name.as_str() {
                    "int" => Ok(Type::int()),
                    "float" => Ok(Type::float()),
                    "string" => Ok(Type::string()),
                    "char" => Ok(Type::char()),
                    "bool" => Ok(Type::bool()),
                    "unit" => Ok(Type::unit()),
                    _ => Err(LowerError::Internal(format!("unknown type {name}"))),
                }
            }
            TypeExpr::Paren(inner) => self.type_expr_to_type(inner),
            _ => Err(LowerError::Internal("complex type expression".to_string())),
        }
    }

    /// Lower a program from AST to IR.
    pub fn lower_program(&mut self, program: &Program) -> Result<IRProgram, LowerError> {
        // First, type check the entire program
        self.checker
            .check_program(program)
            .map_err(LowerError::TypeError)?;

        // Then lower each declaration
        let mut decls = Vec::new();
        for decl in &program.decls {
            if let Some(ir_decl) = self.lower_decl(&decl.value)? {
                decls.push(ir_decl);
            }
        }

        Ok(IRProgram::new(decls))
    }

    /// Lower a standalone expression with a known type (for REPL use).
    ///
    /// The type should be pre-computed by the caller's type checker.
    pub fn lower_expr_with_type(&mut self, expr: &Spanned<Expr>, ty: &Type) -> Result<IRExpr, LowerError> {
        self.lower_expr(expr, ty)
    }

    /// Lower a standalone function declaration (for REPL use).
    ///
    /// Note: The declaration must already be type-checked.
    pub fn lower_fun_standalone(&mut self, fun_decl: &FunDecl) -> Result<IRDecl, LowerError> {
        self.lower_fun_decl(fun_decl)?
            .ok_or_else(|| LowerError::Internal("Failed to lower function".to_string()))
    }

    /// Lower a declaration.
    fn lower_decl(&mut self, decl: &Decl) -> Result<Option<IRDecl>, LowerError> {
        match decl {
            Decl::Val(val_decl) => self.lower_val_decl(val_decl),
            Decl::Fun(fun_decl) => self.lower_fun_decl(fun_decl),
            Decl::Type(_) => {
                // Type aliases don't generate IR
                Ok(None)
            }
            Decl::Datatype(dt) => {
                // Generate data type representation
                let constructors: Vec<(String, Option<Type>)> = dt
                    .constructors
                    .iter()
                    .map(|c| {
                        let arg_ty = c.arg.as_ref().map(|_| {
                            // For now, use a placeholder type
                            // In a full implementation, we'd convert TypeExpr to Type
                            Type::unit()
                        });
                        (c.name.value.clone(), arg_ty)
                    })
                    .collect();

                Ok(Some(IRDecl::Data {
                    name: dt.name.value.clone(),
                    params: dt.params.iter().map(|p| p.value.clone()).collect(),
                    constructors,
                }))
            }
            Decl::Trait(_) => {
                // TODO: Implement trait lowering
                // - Store trait info for later dictionary generation
                Ok(None)
            }
            Decl::Impl(_) => {
                // TODO: Implement impl lowering
                // - Generate dictionary construction
                Ok(None)
            }
        }
    }

    /// Lower a value declaration.
    fn lower_val_decl(&mut self, val: &ValDecl) -> Result<Option<IRDecl>, LowerError> {
        // Get the name from the pattern (must be a simple variable for now)
        let name = match &val.pattern.value {
            Pattern::Var(name) => name.clone(),
            _ => {
                return Err(LowerError::Unsupported(
                    "complex patterns in val declarations".to_string(),
                ))
            }
        };

        // Get the type from the environment (set during check_program)
        let scheme = self
            .checker
            .env()
            .lookup(&name)
            .ok_or_else(|| LowerError::Internal(format!("let {name} not in environment")))?;
        let ty = scheme.instantiate();

        // Lower the expression
        let value = self.lower_expr(&val.expr, &ty)?;

        Ok(Some(IRDecl::Val { name, ty, value }))
    }

    /// Lower a function declaration.
    fn lower_fun_decl(&mut self, fun: &FunDecl) -> Result<Option<IRDecl>, LowerError> {
        let name = fun.name.value.clone();

        // Get the function type from the environment
        let scheme = self
            .checker
            .env()
            .lookup(&name)
            .ok_or_else(|| LowerError::Internal(format!("function {name} not in environment")))?;

        let ty = scheme.instantiate();

        // For now, handle single-clause functions
        if fun.clauses.len() != 1 {
            return Err(LowerError::Unsupported(
                "multi-clause function definitions".to_string(),
            ));
        }

        let clause = &fun.clauses[0];

        // Create parameters
        let mut params = Vec::new();
        let mut current_ty = &ty;

        for param in &clause.params {
            let (param_ty, rest_ty) = match current_ty {
                Type::Arrow(p, r) => (p.as_ref().clone(), r.as_ref()),
                _ => {
                    return Err(LowerError::Internal(
                        "function type mismatch".to_string(),
                    ))
                }
            };

            let var_id = self.bind_pattern(&param.value, &param_ty)?;
            params.push((var_id, param_ty));
            current_ty = rest_ty;
        }

        // The remaining type is the body type
        let body_ty = current_ty.clone();

        // Lower the body
        let body = self.lower_expr(&clause.body, &body_ty)?;

        // Clean up parameter bindings
        for param in &clause.params {
            self.unbind_pattern(&param.value);
        }

        Ok(Some(IRDecl::Fun {
            name,
            ty,
            params,
            body,
        }))
    }

    /// Lower an expression to IR.
    fn lower_expr(&mut self, expr: &Spanned<Expr>, ty: &Type) -> Result<IRExpr, LowerError> {
        match &expr.value {
            Expr::Lit(lit) => self.lower_literal(lit, ty),

            Expr::Var(name) => {
                if let Some(&var_id) = self.vars.get(name) {
                    Ok(IRExpr::Var(var_id, ty.clone()))
                } else {
                    // Must be a global variable
                    Ok(IRExpr::Global(name.clone(), ty.clone()))
                }
            }

            Expr::Con(name) => {
                // Constructor without argument
                Ok(IRExpr::Construct {
                    ctor: name.clone(),
                    arg: None,
                    ty: ty.clone(),
                })
            }

            Expr::Unit => Ok(IRExpr::Unit),

            Expr::App(func, arg) => self.lower_app(func, arg, ty),

            Expr::Lambda(params, body) => self.lower_lambda(params, body, ty),

            Expr::Let(pattern, value, body) => self.lower_let_expr(pattern, value, body, ty),

            Expr::If(cond, then_expr, else_expr) => {
                self.lower_if(cond, then_expr, else_expr, ty)
            }

            Expr::Match(scrutinee, arms) => self.lower_match(scrutinee, arms, ty),

            Expr::BinOp(op, lhs, rhs) => self.lower_binop(*op, lhs, rhs, ty),

            Expr::UnOp(op, operand) => self.lower_unop(*op, operand, ty),

            Expr::Tuple(elems) => self.lower_tuple(elems, ty),

            Expr::List(elems) => self.lower_list(elems, ty),

            Expr::Record(fields) => self.lower_record(fields, ty),

            Expr::Field(record, field) => self.lower_field(record, &field.value, ty),

            Expr::Pipe(lhs, rhs) => self.lower_pipe(lhs, rhs, ty),

            Expr::EnvVar(name) => {
                // Environment variable access becomes getEnv call
                let name_expr = IRExpr::Lit(IRLiteral::String(name.clone()), Type::string());
                Ok(IRExpr::Prim {
                    op: Primitive::GetEnv,
                    args: vec![name_expr],
                    ty: ty.clone(),
                })
            }

            Expr::Annot(inner, _) => {
                // Type annotations are erased
                self.lower_expr(inner, ty)
            }

            Expr::Do(stmts) => self.lower_do(stmts, ty),

            Expr::Command(parts) => self.lower_command(parts, ty),

            Expr::Redirect { .. } => {
                Err(LowerError::Unsupported("redirect expressions".to_string()))
            }
        }
    }

    /// Lower a shell command expression.
    fn lower_command(
        &mut self,
        parts: &[CommandPart],
        ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        // Build the command parts into IR
        let mut ir_parts = Vec::new();
        for part in parts {
            match part {
                CommandPart::Literal(s) => {
                    ir_parts.push(IRCommandPart::Literal(s.clone()));
                }
                CommandPart::Interpolation(expr) => {
                    let expr_ty = self.expr_type(expr)?;
                    let ir_expr = self.lower_expr(expr, &expr_ty)?;
                    ir_parts.push(IRCommandPart::Interpolation(Box::new(ir_expr)));
                }
            }
        }

        Ok(IRExpr::Command {
            parts: ir_parts,
            stdin: None,
            ty: ty.clone(),
        })
    }

    /// Lower a shell command expression with stdin from a previous command.
    fn lower_command_with_stdin(
        &mut self,
        parts: &[CommandPart],
        stdin: IRExpr,
        ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        // Build the command parts into IR
        let mut ir_parts = Vec::new();
        for part in parts {
            match part {
                CommandPart::Literal(s) => {
                    ir_parts.push(IRCommandPart::Literal(s.clone()));
                }
                CommandPart::Interpolation(expr) => {
                    let expr_ty = self.expr_type(expr)?;
                    let ir_expr = self.lower_expr(expr, &expr_ty)?;
                    ir_parts.push(IRCommandPart::Interpolation(Box::new(ir_expr)));
                }
            }
        }

        Ok(IRExpr::Command {
            parts: ir_parts,
            stdin: Some(Box::new(stdin)),
            ty: ty.clone(),
        })
    }

    /// Lower a literal.
    fn lower_literal(&self, lit: &Literal, ty: &Type) -> Result<IRExpr, LowerError> {
        let ir_lit = match lit {
            Literal::Int(n) => IRLiteral::Int(*n),
            Literal::Float(f) => IRLiteral::Float(*f),
            Literal::String(s) => IRLiteral::String(s.clone()),
            Literal::Char(c) => IRLiteral::Char(*c),
            Literal::Bool(b) => IRLiteral::Bool(*b),
        };
        Ok(IRExpr::Lit(ir_lit, ty.clone()))
    }

    /// Lower a function application.
    fn lower_app(
        &mut self,
        func: &Spanned<Expr>,
        arg: &Spanned<Expr>,
        result_ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        // Check if this is a constructor application
        if let Expr::Con(name) = &func.value {
            let arg_ty = self.expr_type(arg)?;
            let ir_arg = self.lower_expr(arg, &arg_ty)?;
            return Ok(IRExpr::Construct {
                ctor: name.clone(),
                arg: Some(Box::new(ir_arg)),
                ty: result_ty.clone(),
            });
        }

        // Check if this is a trait method call
        if let Expr::Var(name) = &func.value {
            if let Some(impl_name) = self.resolve_trait_method(name, arg)? {
                // Found a trait method - call the implementation function instead
                if let Some(builtin) = self.lookup_builtin(&impl_name) {
                    return self.lower_builtin_call(builtin, arg, result_ty);
                }
            }
        }

        // Check if this is a built-in function call
        if let Expr::Var(name) = &func.value {
            if let Some(builtin) = self.lookup_builtin(name) {
                return self.lower_builtin_call(builtin, arg, result_ty);
            }
        }

        // Infer types
        let arg_ty = self.expr_type(arg)?;
        let func_ty = Type::arrow(arg_ty.clone(), result_ty.clone());

        // Lower function and argument
        let ir_func = self.lower_expr(func, &func_ty)?;
        let ir_arg = self.lower_expr(arg, &arg_ty)?;

        // In ANF, we need to ensure both func and arg are values
        // If not, we bind them to temporary variables using Let
        self.ensure_anf_app(ir_func, ir_arg, result_ty)
    }

    /// Resolve a trait method call to its implementation function.
    ///
    /// Returns the implementation function name if this is a trait method call,
    /// or None if it's not a trait method.
    fn resolve_trait_method(
        &mut self,
        method_name: &str,
        arg: &Spanned<Expr>,
    ) -> Result<Option<String>, LowerError> {
        // Find which trait class this method belongs to
        let class_name = self.find_trait_for_method(method_name);
        let class_name = match class_name {
            Some(name) => name,
            None => return Ok(None),
        };

        // Get the argument type
        let arg_ty = self.expr_type(arg)?;

        // Find the instance for this type
        let instance = self
            .checker
            .env()
            .find_instance(&class_name, &arg_ty)
            .ok_or_else(|| {
                LowerError::Internal(format!(
                    "no instance of '{}' for type {}",
                    class_name, arg_ty
                ))
            })?;

        // Look up the implementation function for this method
        let impl_name = instance
            .methods
            .iter()
            .find(|(name, _)| name == method_name)
            .map(|(_, impl_name)| impl_name.clone())
            .ok_or_else(|| {
                LowerError::Internal(format!(
                    "method '{}' not found in instance",
                    method_name
                ))
            })?;

        Ok(Some(impl_name))
    }

    /// Find which trait class a method belongs to.
    fn find_trait_for_method(&self, method_name: &str) -> Option<String> {
        for (class_name, class) in self.checker.env().type_classes_iter() {
            for (name, _) in &class.methods {
                if name == method_name {
                    return Some(class_name.clone());
                }
            }
        }
        None
    }

    /// Look up a built-in function by name.
    fn lookup_builtin(&self, name: &str) -> Option<Primitive> {
        match name {
            // Conversion functions
            "intToFloat" => Some(Primitive::IntToFloat),
            "floatToInt" => Some(Primitive::FloatToInt),
            "intToString" => Some(Primitive::IntToString),
            "floatToString" => Some(Primitive::FloatToString),
            "charToString" => Some(Primitive::CharToString),
            "charToInt" => Some(Primitive::CharToInt),
            "intToChar" => Some(Primitive::IntToChar),
            "__bool_to_string" => Some(Primitive::BoolToString),
            "__string_identity" => Some(Primitive::StringIdentity),

            // String operations
            "stringLength" => Some(Primitive::StringLength),
            "charAt" => Some(Primitive::CharAt),
            "substring" => Some(Primitive::Substring),

            // List operations
            "listLength" => Some(Primitive::ListLength),

            // IO operations
            "print" => Some(Primitive::Print),
            "printNoNewline" => Some(Primitive::PrintNoNewline),
            "readLine" => Some(Primitive::ReadLine),
            "readFile" => Some(Primitive::ReadFile),
            "writeFile" => Some(Primitive::WriteFile),
            "getEnv" => Some(Primitive::GetEnv),

            // Assertions
            "assert" => Some(Primitive::Assert),
            "panic" => Some(Primitive::Panic),

            _ => None,
        }
    }

    /// Lower a built-in function call to IR.
    fn lower_builtin_call(
        &mut self,
        builtin: Primitive,
        arg: &Spanned<Expr>,
        result_ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        // Handle multi-argument builtins (passed as tuples)
        match builtin {
            // Two-argument functions
            Primitive::CharAt | Primitive::WriteFile => {
                // These take a tuple argument
                if let Expr::Tuple(elems) = &arg.value {
                    if elems.len() == 2 {
                        let ty0 = self.expr_type(&elems[0])?;
                        let ty1 = self.expr_type(&elems[1])?;
                        let arg0 = self.lower_expr(&elems[0], &ty0)?;
                        let arg1 = self.lower_expr(&elems[1], &ty1)?;
                        return Ok(IRExpr::Prim {
                            op: builtin,
                            args: vec![arg0, arg1],
                            ty: result_ty.clone(),
                        });
                    }
                }
                // Fall through to single arg handling for partial application
            }
            // Three-argument functions
            Primitive::Substring => {
                if let Expr::Tuple(elems) = &arg.value {
                    if elems.len() == 3 {
                        let ty0 = self.expr_type(&elems[0])?;
                        let ty1 = self.expr_type(&elems[1])?;
                        let ty2 = self.expr_type(&elems[2])?;
                        let arg0 = self.lower_expr(&elems[0], &ty0)?;
                        let arg1 = self.lower_expr(&elems[1], &ty1)?;
                        let arg2 = self.lower_expr(&elems[2], &ty2)?;
                        return Ok(IRExpr::Prim {
                            op: builtin,
                            args: vec![arg0, arg1, arg2],
                            ty: result_ty.clone(),
                        });
                    }
                }
                // Fall through to single arg handling
            }
            _ => {}
        }

        // Single-argument builtins
        let arg_ty = self.expr_type(arg)?;
        let ir_arg = self.lower_expr(arg, &arg_ty)?;
        Ok(IRExpr::Prim {
            op: builtin,
            args: vec![ir_arg],
            ty: result_ty.clone(),
        })
    }

    /// Ensure application is in ANF form by wrapping non-value expressions in Let bindings.
    ///
    /// Returns a complete expression that evaluates both func and arg, binds non-values
    /// to temporaries, and then applies the function.
    fn ensure_anf_app(
        &mut self,
        func: IRExpr,
        arg: IRExpr,
        result_ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        // If both are values, create direct App
        if func.is_value() && arg.is_value() {
            return Ok(IRExpr::App {
                func: Box::new(func),
                arg: Box::new(arg),
                result_ty: result_ty.clone(),
            });
        }

        let func_ty = func.ty();
        let arg_ty = arg.ty();

        if !func.is_value() && !arg.is_value() {
            // Both need binding - create nested lets:
            // let func_var = func in let arg_var = arg in func_var arg_var
            let func_var = VarId::fresh();
            let arg_var = VarId::fresh();

            let inner_app = IRExpr::App {
                func: Box::new(IRExpr::Var(func_var, func_ty.clone())),
                arg: Box::new(IRExpr::Var(arg_var, arg_ty.clone())),
                result_ty: result_ty.clone(),
            };

            let with_arg = IRExpr::Let {
                var: arg_var,
                ty: arg_ty,
                value: Box::new(arg),
                body: Box::new(inner_app),
            };

            let with_func = IRExpr::Let {
                var: func_var,
                ty: func_ty,
                value: Box::new(func),
                body: Box::new(with_arg),
            };

            Ok(with_func)
        } else if !func.is_value() {
            // Only func needs binding:
            // let func_var = func in func_var arg
            let func_var = VarId::fresh();

            let inner_app = IRExpr::App {
                func: Box::new(IRExpr::Var(func_var, func_ty.clone())),
                arg: Box::new(arg),
                result_ty: result_ty.clone(),
            };

            Ok(IRExpr::Let {
                var: func_var,
                ty: func_ty,
                value: Box::new(func),
                body: Box::new(inner_app),
            })
        } else {
            // Only arg needs binding:
            // let arg_var = arg in func arg_var
            let arg_var = VarId::fresh();

            let inner_app = IRExpr::App {
                func: Box::new(func),
                arg: Box::new(IRExpr::Var(arg_var, arg_ty.clone())),
                result_ty: result_ty.clone(),
            };

            Ok(IRExpr::Let {
                var: arg_var,
                ty: arg_ty,
                value: Box::new(arg),
                body: Box::new(inner_app),
            })
        }
    }

    /// Lower a lambda expression.
    fn lower_lambda(
        &mut self,
        params: &[Spanned<Pattern>],
        body: &Spanned<Expr>,
        ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        // Decompose the function type
        let mut current_ty = ty;
        let mut param_tys = Vec::new();

        for _ in params {
            match current_ty {
                Type::Arrow(param_ty, rest_ty) => {
                    param_tys.push(param_ty.as_ref().clone());
                    current_ty = rest_ty;
                }
                _ => {
                    return Err(LowerError::Internal(
                        "lambda type mismatch".to_string(),
                    ))
                }
            }
        }

        let result_ty = current_ty.clone();

        // Build nested lambdas (curried form)
        self.build_curried_lambda(params, &param_tys, body, &result_ty)
    }

    /// Build a curried lambda from multiple parameters.
    fn build_curried_lambda(
        &mut self,
        params: &[Spanned<Pattern>],
        param_tys: &[Type],
        body: &Spanned<Expr>,
        result_ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        if params.is_empty() {
            return self.lower_expr(body, result_ty);
        }

        let param = &params[0];
        let param_ty = &param_tys[0];
        let var_id = self.bind_pattern(&param.value, param_ty)?;

        let inner_result_ty = if params.len() == 1 {
            result_ty.clone()
        } else {
            // Build the inner function type
            let mut inner_ty = result_ty.clone();
            for ty in param_tys[1..].iter().rev() {
                inner_ty = Type::arrow(ty.clone(), inner_ty);
            }
            inner_ty
        };

        let inner = self.build_curried_lambda(&params[1..], &param_tys[1..], body, result_ty)?;

        self.unbind_pattern(&param.value);

        Ok(IRExpr::Lambda {
            param: var_id,
            param_ty: param_ty.clone(),
            body: Box::new(inner),
            result_ty: inner_result_ty,
        })
    }

    /// Lower a let expression: `let p = e1 in e2`
    fn lower_let_expr(
        &mut self,
        pattern: &Spanned<Pattern>,
        value: &Spanned<Expr>,
        body: &Spanned<Expr>,
        ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        let value_ty = self.expr_type(value)?;
        let ir_value = self.lower_expr(value, &value_ty)?;

        // For simple variable pattern, create a direct let binding
        if let Pattern::Var(name) = &pattern.value {
            let var = VarId::fresh();
            self.vars.insert(name.clone(), var);
            self.var_types.insert(var, value_ty.clone());

            let ir_body = self.lower_expr(body, ty)?;

            return Ok(IRExpr::Let {
                var,
                ty: value_ty,
                value: Box::new(ir_value),
                body: Box::new(ir_body),
            });
        }

        // For complex patterns, translate to a match expression
        // let p = e1 in e2  =>  match e1 { p -> e2 }
        // First bind pattern variables
        self.bind_pattern(&pattern.value, &value_ty)?;

        let ir_body = self.lower_expr(body, ty)?;

        // Lower the pattern match
        let scrutinee_var = VarId::fresh();
        let ir_pattern = self.lower_pattern(&pattern.value, &value_ty)?;

        Ok(IRExpr::Let {
            var: scrutinee_var,
            ty: value_ty.clone(),
            value: Box::new(ir_value),
            body: Box::new(IRExpr::Match {
                scrutinee: Box::new(IRExpr::Var(scrutinee_var, value_ty)),
                arms: vec![(ir_pattern, ir_body)],
                ty: ty.clone(),
            }),
        })
    }

    /// Lower an if expression.
    fn lower_if(
        &mut self,
        cond: &Spanned<Expr>,
        then_expr: &Spanned<Expr>,
        else_expr: &Spanned<Expr>,
        ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        let ir_cond = self.lower_expr(cond, &Type::bool())?;
        let ir_then = self.lower_expr(then_expr, ty)?;
        let ir_else = self.lower_expr(else_expr, ty)?;

        // In ANF, condition should be a value
        if ir_cond.is_value() {
            Ok(IRExpr::If {
                cond: Box::new(ir_cond),
                then_branch: Box::new(ir_then),
                else_branch: Box::new(ir_else),
                ty: ty.clone(),
            })
        } else {
            let cond_var = VarId::fresh();
            Ok(IRExpr::Let {
                var: cond_var,
                ty: Type::bool(),
                value: Box::new(ir_cond),
                body: Box::new(IRExpr::If {
                    cond: Box::new(IRExpr::Var(cond_var, Type::bool())),
                    then_branch: Box::new(ir_then),
                    else_branch: Box::new(ir_else),
                    ty: ty.clone(),
                }),
            })
        }
    }

    /// Lower a match expression.
    fn lower_match(
        &mut self,
        scrutinee: &Spanned<Expr>,
        arms: &[MatchArm],
        ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        let scrutinee_ty = self.expr_type(scrutinee)?;
        let ir_scrutinee = self.lower_expr(scrutinee, &scrutinee_ty)?;

        let mut ir_arms = Vec::new();
        for arm in arms {
            if arm.guard.is_some() {
                return Err(LowerError::Unsupported("match guards".to_string()));
            }

            let ir_pattern = self.lower_pattern(&arm.pattern.value, &scrutinee_ty)?;

            // Bind pattern variables
            self.bind_pattern(&arm.pattern.value, &scrutinee_ty)?;
            let ir_body = self.lower_expr(&arm.body, ty)?;
            self.unbind_pattern(&arm.pattern.value);

            ir_arms.push((ir_pattern, ir_body));
        }

        // Ensure scrutinee is a value
        if ir_scrutinee.is_value() {
            Ok(IRExpr::Match {
                scrutinee: Box::new(ir_scrutinee),
                arms: ir_arms,
                ty: ty.clone(),
            })
        } else {
            let scrutinee_var = VarId::fresh();
            Ok(IRExpr::Let {
                var: scrutinee_var,
                ty: scrutinee_ty.clone(),
                value: Box::new(ir_scrutinee),
                body: Box::new(IRExpr::Match {
                    scrutinee: Box::new(IRExpr::Var(scrutinee_var, scrutinee_ty)),
                    arms: ir_arms,
                    ty: ty.clone(),
                }),
            })
        }
    }

    /// Lower a binary operation.
    fn lower_binop(
        &mut self,
        op: BinOp,
        lhs: &Spanned<Expr>,
        rhs: &Spanned<Expr>,
        result_ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        let lhs_ty = self.expr_type(lhs)?;
        let ir_lhs = self.lower_expr(lhs, &lhs_ty)?;
        let ir_rhs = self.lower_expr(rhs, &lhs_ty)?;

        // Specialize the primitive based on operand type
        let prim = self.specialize_binop(op, &lhs_ty)?;

        // Short-circuit for And/Or
        if matches!(op, BinOp::And | BinOp::Or) {
            return self.lower_short_circuit(op, ir_lhs, ir_rhs, result_ty);
        }

        Ok(IRExpr::Prim {
            op: prim,
            args: vec![ir_lhs, ir_rhs],
            ty: result_ty.clone(),
        })
    }

    /// Lower short-circuit boolean operations.
    fn lower_short_circuit(
        &self,
        op: BinOp,
        lhs: IRExpr,
        rhs: IRExpr,
        ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        match op {
            BinOp::And => {
                // lhs && rhs  =>  if lhs then rhs else false
                Ok(IRExpr::If {
                    cond: Box::new(lhs),
                    then_branch: Box::new(rhs),
                    else_branch: Box::new(IRExpr::Lit(IRLiteral::Bool(false), Type::bool())),
                    ty: ty.clone(),
                })
            }
            BinOp::Or => {
                // lhs || rhs  =>  if lhs then true else rhs
                Ok(IRExpr::If {
                    cond: Box::new(lhs),
                    then_branch: Box::new(IRExpr::Lit(IRLiteral::Bool(true), Type::bool())),
                    else_branch: Box::new(rhs),
                    ty: ty.clone(),
                })
            }
            _ => unreachable!(),
        }
    }

    /// Specialize a binary operator to a primitive based on type.
    fn specialize_binop(&self, op: BinOp, ty: &Type) -> Result<Primitive, LowerError> {
        let is_int = matches!(ty, Type::Con(name, _) if name == "int");
        let is_float = matches!(ty, Type::Con(name, _) if name == "float");
        let is_string = matches!(ty, Type::Con(name, _) if name == "string");
        let is_bool = matches!(ty, Type::Con(name, _) if name == "bool");
        let is_char = matches!(ty, Type::Con(name, _) if name == "char");

        match op {
            BinOp::Add if is_int => Ok(Primitive::AddInt),
            BinOp::Add if is_float => Ok(Primitive::AddFloat),
            BinOp::Sub if is_int => Ok(Primitive::SubInt),
            BinOp::Sub if is_float => Ok(Primitive::SubFloat),
            BinOp::Mul if is_int => Ok(Primitive::MulInt),
            BinOp::Mul if is_float => Ok(Primitive::MulFloat),
            BinOp::Div if is_int => Ok(Primitive::DivInt),
            BinOp::Div if is_float => Ok(Primitive::DivFloat),
            BinOp::Mod if is_int => Ok(Primitive::ModInt),

            BinOp::Eq if is_int => Ok(Primitive::EqInt),
            BinOp::Eq if is_float => Ok(Primitive::EqFloat),
            BinOp::Eq if is_string => Ok(Primitive::EqString),
            BinOp::Eq if is_bool => Ok(Primitive::EqBool),
            BinOp::Eq if is_char => Ok(Primitive::EqChar),

            BinOp::Neq if is_int => Ok(Primitive::NeqInt),
            BinOp::Neq if is_float => Ok(Primitive::NeqFloat),
            BinOp::Neq if is_string => Ok(Primitive::NeqString),
            BinOp::Neq if is_bool => Ok(Primitive::NeqBool),
            BinOp::Neq if is_char => Ok(Primitive::NeqChar),

            BinOp::Lt if is_int => Ok(Primitive::LtInt),
            BinOp::Lt if is_float => Ok(Primitive::LtFloat),
            BinOp::Lt if is_string => Ok(Primitive::LtString),
            BinOp::Lt if is_char => Ok(Primitive::LtChar),

            BinOp::Le if is_int => Ok(Primitive::LeInt),
            BinOp::Le if is_float => Ok(Primitive::LeFloat),
            BinOp::Le if is_string => Ok(Primitive::LeString),
            BinOp::Le if is_char => Ok(Primitive::LeChar),

            BinOp::Gt if is_int => Ok(Primitive::GtInt),
            BinOp::Gt if is_float => Ok(Primitive::GtFloat),
            BinOp::Gt if is_string => Ok(Primitive::GtString),
            BinOp::Gt if is_char => Ok(Primitive::GtChar),

            BinOp::Ge if is_int => Ok(Primitive::GeInt),
            BinOp::Ge if is_float => Ok(Primitive::GeFloat),
            BinOp::Ge if is_string => Ok(Primitive::GeString),
            BinOp::Ge if is_char => Ok(Primitive::GeChar),

            BinOp::And => Ok(Primitive::And),
            BinOp::Or => Ok(Primitive::Or),

            BinOp::Concat => Ok(Primitive::Concat),
            BinOp::Cons => Ok(Primitive::Cons),
            BinOp::Append => Ok(Primitive::Append),

            _ => Err(LowerError::Unsupported(format!(
                "operator {op:?} for type {ty}"
            ))),
        }
    }

    /// Lower a unary operation.
    fn lower_unop(
        &mut self,
        op: UnOp,
        operand: &Spanned<Expr>,
        result_ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        let operand_ty = self.expr_type(operand)?;
        let ir_operand = self.lower_expr(operand, &operand_ty)?;

        let prim = match op {
            UnOp::Not => Primitive::Not,
            UnOp::Neg => {
                if matches!(&operand_ty, Type::Con(name, _) if name == "float") {
                    Primitive::NegFloat
                } else {
                    Primitive::NegInt
                }
            }
        };

        Ok(IRExpr::Prim {
            op: prim,
            args: vec![ir_operand],
            ty: result_ty.clone(),
        })
    }

    /// Lower a tuple expression.
    fn lower_tuple(
        &mut self,
        elems: &[Spanned<Expr>],
        ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        let elem_tys = match ty {
            Type::Tuple(tys) => tys.clone(),
            _ => {
                return Err(LowerError::Internal(
                    "tuple expression with non-tuple type".to_string(),
                ))
            }
        };

        let mut ir_elems = Vec::new();
        for (elem, elem_ty) in elems.iter().zip(elem_tys.iter()) {
            ir_elems.push(self.lower_expr(elem, elem_ty)?);
        }

        Ok(IRExpr::Tuple {
            elems: ir_elems,
            ty: ty.clone(),
        })
    }

    /// Lower a list expression.
    fn lower_list(
        &mut self,
        elems: &[Spanned<Expr>],
        ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        let elem_ty = match ty {
            Type::Con(name, args) if name == "list" && args.len() == 1 => args[0].clone(),
            _ => {
                return Err(LowerError::Internal(
                    "list expression with non-list type".to_string(),
                ))
            }
        };

        // Build list from right to left: [a, b, c] => Cons(a, Cons(b, Cons(c, Nil)))
        let nil = IRExpr::Construct {
            ctor: "Nil".to_string(),
            arg: None,
            ty: ty.clone(),
        };

        let mut result = nil;
        for elem in elems.iter().rev() {
            let ir_elem = self.lower_expr(elem, &elem_ty)?;

            // Cons takes element and list, returns list
            result = IRExpr::Prim {
                op: Primitive::Cons,
                args: vec![ir_elem, result],
                ty: ty.clone(),
            };
        }

        Ok(result)
    }

    /// Lower a record expression.
    fn lower_record(
        &mut self,
        fields: &[(Spanned<String>, Spanned<Expr>)],
        ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        let field_tys = match ty {
            Type::Record(flds) => flds
                .iter()
                .cloned()
                .collect::<HashMap<String, Type>>(),
            _ => {
                return Err(LowerError::Internal(
                    "record expression with non-record type".to_string(),
                ))
            }
        };

        let mut ir_fields = Vec::new();
        for (name, expr) in fields {
            let field_ty = field_tys.get(&name.value).ok_or_else(|| {
                LowerError::Internal(format!("unknown field {}", name.value))
            })?;
            let ir_expr = self.lower_expr(expr, field_ty)?;
            ir_fields.push((name.value.clone(), ir_expr));
        }

        Ok(IRExpr::Record {
            fields: ir_fields,
            ty: ty.clone(),
        })
    }

    /// Lower a field access expression.
    fn lower_field(
        &mut self,
        record: &Spanned<Expr>,
        field: &str,
        ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        let record_ty = self.expr_type(record)?;
        let ir_record = self.lower_expr(record, &record_ty)?;

        Ok(IRExpr::Field {
            record: Box::new(ir_record),
            field: field.to_string(),
            ty: ty.clone(),
        })
    }

    /// Lower a pipe expression.
    ///
    /// For command pipelines (`cmd1` |> `cmd2`), pipes stdout of cmd1 to stdin of cmd2.
    /// For function pipelines (x |> f), becomes function application f(x).
    fn lower_pipe(
        &mut self,
        lhs: &Spanned<Expr>,
        rhs: &Spanned<Expr>,
        result_ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        // Check if both sides are commands - if so, create a shell pipeline
        if let (Expr::Command(lhs_parts), Expr::Command(rhs_parts)) = (&lhs.value, &rhs.value) {
            // Lower the left command first
            let ir_lhs = self.lower_command(lhs_parts, &Type::command_result())?;
            // Lower the right command with lhs as stdin
            return self.lower_command_with_stdin(rhs_parts, ir_lhs, result_ty);
        }

        // Check for chained pipeline: (cmd1 |> cmd2) |> cmd3
        // The LHS would be a Pipe expression where both sides are commands
        if let Expr::Pipe(pipe_lhs, pipe_rhs) = &lhs.value {
            if let Expr::Command(rhs_parts) = &rhs.value {
                // Check if the inner pipe is a command pipeline
                if matches!((&pipe_lhs.value, &pipe_rhs.value), (Expr::Command(_), Expr::Command(_)))
                {
                    // Lower the left pipeline first
                    let ir_lhs = self.lower_pipe(pipe_lhs, pipe_rhs, &Type::command_result())?;
                    // Lower the right command with lhs as stdin
                    return self.lower_command_with_stdin(rhs_parts, ir_lhs, result_ty);
                }
            }
        }

        // Regular function application: e1 |> e2  =>  e2 e1
        let lhs_ty = self.expr_type(lhs)?;
        let rhs_ty = Type::arrow(lhs_ty.clone(), result_ty.clone());

        let ir_lhs = self.lower_expr(lhs, &lhs_ty)?;
        let ir_rhs = self.lower_expr(rhs, &rhs_ty)?;

        Ok(IRExpr::App {
            func: Box::new(ir_rhs),
            arg: Box::new(ir_lhs),
            result_ty: result_ty.clone(),
        })
    }

    /// Lower a do block.
    fn lower_do(&mut self, stmts: &[DoStmt], ty: &Type) -> Result<IRExpr, LowerError> {
        if stmts.is_empty() {
            return Ok(IRExpr::Unit);
        }

        // For now, just sequence the statements
        // Full monadic desugaring would require more work
        self.lower_do_stmts(stmts, ty)
    }

    /// Lower do block statements.
    fn lower_do_stmts(&mut self, stmts: &[DoStmt], result_ty: &Type) -> Result<IRExpr, LowerError> {
        if stmts.len() == 1 {
            return match &stmts[0] {
                DoStmt::Expr(e) => self.lower_expr(e, result_ty),
                DoStmt::Bind(_, _) | DoStmt::Let(_, _) => {
                    Err(LowerError::Unsupported(
                        "do block ending with bind/let".to_string(),
                    ))
                }
            };
        }

        match &stmts[0] {
            DoStmt::Expr(e) => {
                // Ignore result, continue with rest
                let e_ty = self.expr_type(e)?;
                let ir_e = self.lower_expr(e, &e_ty)?;
                let rest = self.lower_do_stmts(&stmts[1..], result_ty)?;

                let var = VarId::fresh();
                Ok(IRExpr::Let {
                    var,
                    ty: e_ty,
                    value: Box::new(ir_e),
                    body: Box::new(rest),
                })
            }
            DoStmt::Let(pattern, value) => {
                let value_ty = self.expr_type(value)?;
                let var_id = self.bind_pattern(&pattern.value, &value_ty)?;
                let ir_value = self.lower_expr(value, &value_ty)?;
                let rest = self.lower_do_stmts(&stmts[1..], result_ty)?;
                self.unbind_pattern(&pattern.value);

                Ok(IRExpr::Let {
                    var: var_id,
                    ty: value_ty,
                    value: Box::new(ir_value),
                    body: Box::new(rest),
                })
            }
            DoStmt::Bind(pattern, value) => {
                // For now, treat bind same as let (no monadic desugaring)
                let value_ty = self.expr_type(value)?;
                let var_id = self.bind_pattern(&pattern.value, &value_ty)?;
                let ir_value = self.lower_expr(value, &value_ty)?;
                let rest = self.lower_do_stmts(&stmts[1..], result_ty)?;
                self.unbind_pattern(&pattern.value);

                Ok(IRExpr::Let {
                    var: var_id,
                    ty: value_ty,
                    value: Box::new(ir_value),
                    body: Box::new(rest),
                })
            }
        }
    }

    /// Lower a pattern to IR.
    fn lower_pattern(&mut self, pattern: &Pattern, ty: &Type) -> Result<IRPattern, LowerError> {
        match pattern {
            Pattern::Wildcard => Ok(IRPattern::Wildcard),

            Pattern::Var(name) => {
                let var_id = self
                    .vars
                    .get(name)
                    .copied()
                    .unwrap_or_else(VarId::fresh);
                Ok(IRPattern::Var(var_id, ty.clone()))
            }

            Pattern::Lit(lit) => {
                let ir_lit = match lit {
                    Literal::Int(n) => IRLiteral::Int(*n),
                    Literal::Float(f) => IRLiteral::Float(*f),
                    Literal::String(s) => IRLiteral::String(s.clone()),
                    Literal::Char(c) => IRLiteral::Char(*c),
                    Literal::Bool(b) => IRLiteral::Bool(*b),
                };
                Ok(IRPattern::Lit(ir_lit))
            }

            Pattern::Con(name, arg) => {
                let ir_arg = if let Some(arg_pat) = arg {
                    // For now, use unit type for constructor arg
                    // In full implementation, we'd look up constructor type
                    Some(Box::new(self.lower_pattern(&arg_pat.value, &Type::unit())?))
                } else {
                    None
                };
                Ok(IRPattern::Con {
                    ctor: name.clone(),
                    arg: ir_arg,
                })
            }

            Pattern::Tuple(elems) => {
                let elem_tys = match ty {
                    Type::Tuple(tys) => tys.clone(),
                    _ => vec![Type::unit(); elems.len()],
                };

                let mut ir_elems = Vec::new();
                for (elem, elem_ty) in elems.iter().zip(elem_tys.iter()) {
                    ir_elems.push(self.lower_pattern(&elem.value, elem_ty)?);
                }
                Ok(IRPattern::Tuple(ir_elems))
            }

            Pattern::Record(fields) => {
                let field_tys = match ty {
                    Type::Record(flds) => flds
                        .iter()
                        .cloned()
                        .collect::<HashMap<String, Type>>(),
                    _ => HashMap::new(),
                };

                let mut ir_fields = Vec::new();
                for (name, pat) in fields {
                    let field_ty = field_tys.get(&name.value).cloned().unwrap_or(Type::unit());
                    let ir_pat = self.lower_pattern(&pat.value, &field_ty)?;
                    ir_fields.push((name.value.clone(), ir_pat));
                }
                Ok(IRPattern::Record(ir_fields))
            }

            Pattern::List(_) | Pattern::Cons(_, _) => {
                Err(LowerError::Unsupported("list patterns".to_string()))
            }

            Pattern::Or(_, _) => {
                Err(LowerError::Unsupported("or patterns".to_string()))
            }

            Pattern::As(_, _) => {
                Err(LowerError::Unsupported("as patterns".to_string()))
            }

            Pattern::Annot(inner, _) => self.lower_pattern(&inner.value, ty),
        }
    }

    /// Bind pattern variables to fresh VarIds.
    fn bind_pattern(&mut self, pattern: &Pattern, ty: &Type) -> Result<VarId, LowerError> {
        match pattern {
            Pattern::Wildcard => Ok(VarId::fresh()),

            Pattern::Var(name) => {
                let var_id = VarId::fresh();
                self.vars.insert(name.clone(), var_id);
                self.var_types.insert(var_id, ty.clone());
                Ok(var_id)
            }

            Pattern::Tuple(elems) => {
                let elem_tys = match ty {
                    Type::Tuple(tys) => tys.clone(),
                    _ => vec![Type::unit(); elems.len()],
                };

                for (elem, elem_ty) in elems.iter().zip(elem_tys.iter()) {
                    self.bind_pattern(&elem.value, elem_ty)?;
                }
                Ok(VarId::fresh())
            }

            Pattern::Con(_, Some(arg)) => {
                self.bind_pattern(&arg.value, &Type::unit())?;
                Ok(VarId::fresh())
            }

            Pattern::Con(_, None) => Ok(VarId::fresh()),

            Pattern::Annot(inner, _) => self.bind_pattern(&inner.value, ty),

            _ => Ok(VarId::fresh()),
        }
    }

    /// Remove pattern bindings.
    fn unbind_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Var(name) => {
                self.vars.remove(name);
            }
            Pattern::Tuple(elems) => {
                for elem in elems {
                    self.unbind_pattern(&elem.value);
                }
            }
            Pattern::Con(_, Some(arg)) => {
                self.unbind_pattern(&arg.value);
            }
            Pattern::Annot(inner, _) => {
                self.unbind_pattern(&inner.value);
            }
            _ => {}
        }
    }
}

impl Default for Lower {
    fn default() -> Self {
        Self::new()
    }
}

// Allow TypeError to be converted to LowerError
impl From<TypeError> for LowerError {
    fn from(e: TypeError) -> Self {
        LowerError::TypeError(vec![e])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Parser;

    fn lower_program(source: &str) -> Result<IRProgram, LowerError> {
        let mut parser = Parser::new(source).expect("lexer error");
        let program = parser.parse_program().expect("parse error");
        let mut lower = Lower::new();
        lower.lower_program(&program)
    }

    #[test]
    fn lower_val_int() {
        let ir = lower_program("let x = 42").expect("lower error");
        assert_eq!(ir.decls.len(), 1);

        match &ir.decls[0] {
            IRDecl::Val { name, ty, value } => {
                assert_eq!(name, "x");
                assert_eq!(*ty, Type::int());
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Int(42), _)));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_val_string() {
        let ir = lower_program("let s = \"hello\"").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { name, ty, value } => {
                assert_eq!(name, "s");
                assert_eq!(*ty, Type::string());
                assert!(matches!(value, IRExpr::Lit(IRLiteral::String(s), _) if s == "hello"));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_val_bool() {
        let ir = lower_program("let b = true").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { name, ty, value } => {
                assert_eq!(name, "b");
                assert_eq!(*ty, Type::bool());
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Bool(true), _)));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_arithmetic() {
        let ir = lower_program("let x = 1 + 2").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Prim { op: Primitive::AddInt, .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_float_arithmetic() {
        let ir = lower_program("let x = 1.0 + 2.0").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Prim { op: Primitive::AddFloat, .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_comparison() {
        let ir = lower_program("let b = 1 < 2").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { ty, value, .. } => {
                assert_eq!(*ty, Type::bool());
                assert!(matches!(value, IRExpr::Prim { op: Primitive::LtInt, .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_fun() {
        let ir = lower_program("fn add x y = x + y").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Fun { name, params, .. } => {
                assert_eq!(name, "add");
                assert_eq!(params.len(), 2);
            }
            _ => panic!("expected fun decl"),
        }
    }

    #[test]
    fn lower_if() {
        let ir = lower_program("let x = if true then 1 else 2").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::If { .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_tuple() {
        let ir = lower_program("let t = (1, \"a\", true)").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { ty, value, .. } => {
                assert!(matches!(ty, Type::Tuple(_)));
                assert!(matches!(value, IRExpr::Tuple { .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_list() {
        let ir = lower_program("let xs = [1, 2, 3]").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                // List should be desugared to Cons chain
                assert!(matches!(value, IRExpr::Prim { op: Primitive::Cons, .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_empty_list() {
        let ir = lower_program("let xs: list<int> = []").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Construct { ctor, arg: None, .. } if ctor == "Nil"));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_record() {
        let ir = lower_program("let r = {x = 1, y = 2}").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Record { .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_lambda() {
        let ir = lower_program("let f = (x) => x + 1").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Lambda { .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_short_circuit_and() {
        let ir = lower_program("let b = true && false").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                // && should become if-then-else
                assert!(matches!(value, IRExpr::If { .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_negation() {
        let ir = lower_program("let x = -42").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Prim { op: Primitive::NegInt, .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_not() {
        let ir = lower_program("let b = !true").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Prim { op: Primitive::Not, .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_pipe() {
        let ir = lower_program("fn f x = x + 1\nlet y = 1 |> f").expect("lower error");

        match &ir.decls[1] {
            IRDecl::Val { value, .. } => {
                // Pipe should become application
                assert!(matches!(value, IRExpr::App { .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    // ========== Type Class Tests ==========

    #[test]
    fn lower_show_int() {
        let ir = lower_program("let s = show 42").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { name, value, .. } => {
                assert_eq!(name, "s");
                // show 42 should become intToString 42
                assert!(matches!(value, IRExpr::Prim { op: Primitive::IntToString, .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_show_float() {
        let ir = lower_program("let s = show 3.14").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                // show 3.14 should become floatToString 3.14
                assert!(matches!(value, IRExpr::Prim { op: Primitive::FloatToString, .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_show_bool() {
        let ir = lower_program("let s = show true").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                // show true should become __bool_to_string true
                assert!(matches!(value, IRExpr::Prim { op: Primitive::BoolToString, .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_show_string() {
        let ir = lower_program("let s = show \"hello\"").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                // show "hello" should become __string_identity "hello"
                assert!(matches!(value, IRExpr::Prim { op: Primitive::StringIdentity, .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_show_char() {
        let ir = lower_program("let s = show 'a'").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                // show 'a' should become charToString 'a'
                assert!(matches!(value, IRExpr::Prim { op: Primitive::CharToString, .. }));
            }
            _ => panic!("expected val decl"),
        }
    }
}
