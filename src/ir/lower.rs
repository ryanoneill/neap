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
    BinOp, Decl, DoStmt, Expr, FunDecl, Literal, MatchArm, Pattern, Program, Spanned, UnOp,
    ValDecl,
};
use crate::types::{Type, TypeChecker, TypeError};

use super::core::{Binding, IRDecl, IRExpr, IRLiteral, IRPattern, IRProgram, Primitive, VarId};

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

    /// Get the type of an expression using local bindings.
    ///
    /// This avoids calling the type checker's infer_expr which doesn't
    /// have our local variable bindings in scope.
    fn expr_type(&self, expr: &Spanned<Expr>) -> Result<Type, LowerError> {
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
            .ok_or_else(|| LowerError::Internal(format!("val {name} not in environment")))?;
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

            Expr::Let {
                rec,
                pattern,
                value,
                body,
                ..
            } => self.lower_let(*rec, pattern, value, body, ty),

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

            Expr::Redirect { .. } => {
                Err(LowerError::Unsupported("redirect expressions".to_string()))
            }
        }
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

        // In ANF, we need to ensure both are values
        // If not, we bind them to temporary variables
        let (ir_func, ir_arg) = self.ensure_anf_app(ir_func, ir_arg, result_ty)?;

        Ok(IRExpr::App {
            func: Box::new(ir_func),
            arg: Box::new(ir_arg),
            result_ty: result_ty.clone(),
        })
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

    /// Ensure application is in ANF form.
    fn ensure_anf_app(
        &mut self,
        func: IRExpr,
        arg: IRExpr,
        result_ty: &Type,
    ) -> Result<(IRExpr, IRExpr), LowerError> {
        // If both are values, we're done
        if func.is_value() && arg.is_value() {
            return Ok((func, arg));
        }

        // Need to bind non-values to temporaries
        let func_ty = func.ty();
        let arg_ty = arg.ty();

        if !func.is_value() && !arg.is_value() {
            // Both need binding - create nested lets
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

            Ok((func, with_arg))
        } else if !func.is_value() {
            // Only func needs binding
            let func_var = VarId::fresh();
            let app = IRExpr::App {
                func: Box::new(IRExpr::Var(func_var, func_ty.clone())),
                arg: Box::new(arg),
                result_ty: result_ty.clone(),
            };
            Ok((func, app))
        } else {
            // Only arg needs binding
            let arg_var = VarId::fresh();
            let app = IRExpr::App {
                func: Box::new(func),
                arg: Box::new(IRExpr::Var(arg_var, arg_ty.clone())),
                result_ty: result_ty.clone(),
            };
            Ok((arg, app))
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

    /// Lower a let expression.
    fn lower_let(
        &mut self,
        rec: bool,
        pattern: &Spanned<Pattern>,
        value: &Spanned<Expr>,
        body: &Spanned<Expr>,
        body_ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        let value_ty = self.expr_type(value)?;

        if rec {
            // Recursive let
            let name = match &pattern.value {
                Pattern::Var(n) => n.clone(),
                _ => {
                    return Err(LowerError::Unsupported(
                        "recursive let with complex pattern".to_string(),
                    ))
                }
            };

            let var_id = VarId::fresh();
            self.vars.insert(name.clone(), var_id);
            self.var_types.insert(var_id, value_ty.clone());

            let ir_value = self.lower_expr(value, &value_ty)?;
            let ir_body = self.lower_expr(body, body_ty)?;

            self.vars.remove(&name);

            Ok(IRExpr::LetRec {
                bindings: vec![Binding {
                    var: var_id,
                    ty: value_ty,
                    value: ir_value,
                }],
                body: Box::new(ir_body),
            })
        } else {
            // Non-recursive let
            let var_id = self.bind_pattern(&pattern.value, &value_ty)?;
            let ir_value = self.lower_expr(value, &value_ty)?;
            let ir_body = self.lower_expr(body, body_ty)?;
            self.unbind_pattern(&pattern.value);

            Ok(IRExpr::Let {
                var: var_id,
                ty: value_ty,
                value: Box::new(ir_value),
                body: Box::new(ir_body),
            })
        }
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
    fn lower_pipe(
        &mut self,
        lhs: &Spanned<Expr>,
        rhs: &Spanned<Expr>,
        result_ty: &Type,
    ) -> Result<IRExpr, LowerError> {
        // e1 |> e2  =>  e2 e1
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
        let ir = lower_program("fun add x y = x + y").expect("lower error");

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
    fn lower_let_expr() {
        let ir = lower_program("let x = let y = 1 in y + 1").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Let { .. }));
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
        let ir = lower_program("let xs: int list = []").expect("lower error");

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
        let ir = lower_program("let f = fn x => x + 1").expect("lower error");

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
        let ir = lower_program("let b = not true").expect("lower error");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Prim { op: Primitive::Not, .. }));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn lower_pipe() {
        let ir = lower_program("fun f x = x + 1\nlet y = 1 |> f").expect("lower error");

        match &ir.decls[1] {
            IRDecl::Val { value, .. } => {
                // Pipe should become application
                assert!(matches!(value, IRExpr::App { .. }));
            }
            _ => panic!("expected val decl"),
        }
    }
}
