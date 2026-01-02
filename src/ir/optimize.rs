//! IR optimizations
//!
//! This module provides basic optimizations on the IR:
//! - Constant folding
//! - Dead code elimination
//! - Simple beta reduction
//! - If simplification

use std::collections::HashSet;

use super::core::{Binding, IRDecl, IRExpr, IRLiteral, IRPattern, IRProgram, Primitive, VarId};
use crate::types::Type;

/// The optimizer.
pub struct Optimizer {
    /// How many optimization passes to run
    max_passes: usize,
}

impl Optimizer {
    /// Create a new optimizer.
    #[must_use]
    pub fn new() -> Self {
        Self { max_passes: 10 }
    }

    /// Create an optimizer with a custom pass limit.
    #[must_use]
    pub fn with_max_passes(max_passes: usize) -> Self {
        Self { max_passes }
    }

    /// Optimize an IR program.
    #[must_use]
    pub fn optimize(&self, program: IRProgram) -> IRProgram {
        let mut decls = program.decls;

        for _ in 0..self.max_passes {
            let mut changed = false;

            decls = decls
                .into_iter()
                .map(|decl| {
                    let (new_decl, decl_changed) = self.optimize_decl(decl);
                    changed |= decl_changed;
                    new_decl
                })
                .collect();

            if !changed {
                break;
            }
        }

        IRProgram::new(decls)
    }

    /// Optimize a declaration.
    fn optimize_decl(&self, decl: IRDecl) -> (IRDecl, bool) {
        match decl {
            IRDecl::Val { name, ty, value } => {
                let (new_value, changed) = self.optimize_expr_internal(value);
                (IRDecl::Val { name, ty, value: new_value }, changed)
            }
            IRDecl::Fun { name, ty, params, body } => {
                let (new_body, changed) = self.optimize_expr_internal(body);
                (IRDecl::Fun { name, ty, params, body: new_body }, changed)
            }
            IRDecl::Data { .. } => (decl, false),
        }
    }

    /// Optimize a standalone expression (for REPL use).
    pub fn optimize_expr(&self, expr: IRExpr) -> IRExpr {
        let (optimized, _) = self.optimize_expr_internal(expr);
        optimized
    }

    /// Optimize an expression (internal, returns changed flag).
    fn optimize_expr_internal(&self, expr: IRExpr) -> (IRExpr, bool) {
        // First, recursively optimize subexpressions
        let (expr, mut changed) = self.optimize_subexprs(expr);

        // Then apply local optimizations
        let (expr, const_folded) = self.constant_fold(expr);
        changed |= const_folded;

        let (expr, if_simplified) = self.simplify_if(expr);
        changed |= if_simplified;

        let (expr, beta_reduced) = self.beta_reduce(expr);
        changed |= beta_reduced;

        let (expr, dce) = self.dead_code_elimination(expr);
        changed |= dce;

        (expr, changed)
    }

    /// Optimize subexpressions recursively.
    fn optimize_subexprs(&self, expr: IRExpr) -> (IRExpr, bool) {
        match expr {
            IRExpr::Let { var, ty, value, body } => {
                let (new_value, c1) = self.optimize_expr_internal(*value);
                let (new_body, c2) = self.optimize_expr_internal(*body);
                (
                    IRExpr::Let {
                        var,
                        ty,
                        value: Box::new(new_value),
                        body: Box::new(new_body),
                    },
                    c1 || c2,
                )
            }

            IRExpr::LetRec { bindings, body } => {
                let mut changed = false;
                let new_bindings: Vec<Binding> = bindings
                    .into_iter()
                    .map(|b| {
                        let (new_value, c) = self.optimize_expr_internal(b.value);
                        changed |= c;
                        Binding {
                            var: b.var,
                            ty: b.ty,
                            value: new_value,
                        }
                    })
                    .collect();
                let (new_body, c) = self.optimize_expr_internal(*body);
                changed |= c;
                (
                    IRExpr::LetRec {
                        bindings: new_bindings,
                        body: Box::new(new_body),
                    },
                    changed,
                )
            }

            IRExpr::Lambda { param, param_ty, body, result_ty } => {
                let (new_body, changed) = self.optimize_expr_internal(*body);
                (
                    IRExpr::Lambda {
                        param,
                        param_ty,
                        body: Box::new(new_body),
                        result_ty,
                    },
                    changed,
                )
            }

            IRExpr::App { func, arg, result_ty } => {
                let (new_func, c1) = self.optimize_expr_internal(*func);
                let (new_arg, c2) = self.optimize_expr_internal(*arg);
                (
                    IRExpr::App {
                        func: Box::new(new_func),
                        arg: Box::new(new_arg),
                        result_ty,
                    },
                    c1 || c2,
                )
            }

            IRExpr::If { cond, then_branch, else_branch, ty } => {
                let (new_cond, c1) = self.optimize_expr_internal(*cond);
                let (new_then, c2) = self.optimize_expr_internal(*then_branch);
                let (new_else, c3) = self.optimize_expr_internal(*else_branch);
                (
                    IRExpr::If {
                        cond: Box::new(new_cond),
                        then_branch: Box::new(new_then),
                        else_branch: Box::new(new_else),
                        ty,
                    },
                    c1 || c2 || c3,
                )
            }

            IRExpr::Match { scrutinee, arms, ty } => {
                let (new_scrutinee, c1) = self.optimize_expr_internal(*scrutinee);
                let mut changed = c1;
                let new_arms: Vec<_> = arms
                    .into_iter()
                    .map(|(pat, body)| {
                        let (new_body, c) = self.optimize_expr_internal(body);
                        changed |= c;
                        (pat, new_body)
                    })
                    .collect();
                (
                    IRExpr::Match {
                        scrutinee: Box::new(new_scrutinee),
                        arms: new_arms,
                        ty,
                    },
                    changed,
                )
            }

            IRExpr::Prim { op, args, ty } => {
                let mut changed = false;
                let new_args: Vec<_> = args
                    .into_iter()
                    .map(|arg| {
                        let (new_arg, c) = self.optimize_expr_internal(arg);
                        changed |= c;
                        new_arg
                    })
                    .collect();
                (IRExpr::Prim { op, args: new_args, ty }, changed)
            }

            IRExpr::Construct { ctor, arg, ty } => {
                if let Some(a) = arg {
                    let (new_arg, changed) = self.optimize_expr_internal(*a);
                    (
                        IRExpr::Construct {
                            ctor,
                            arg: Some(Box::new(new_arg)),
                            ty,
                        },
                        changed,
                    )
                } else {
                    (IRExpr::Construct { ctor, arg: None, ty }, false)
                }
            }

            IRExpr::Tuple { elems, ty } => {
                let mut changed = false;
                let new_elems: Vec<_> = elems
                    .into_iter()
                    .map(|e| {
                        let (new_e, c) = self.optimize_expr_internal(e);
                        changed |= c;
                        new_e
                    })
                    .collect();
                (IRExpr::Tuple { elems: new_elems, ty }, changed)
            }

            IRExpr::TupleProj { tuple, index, ty } => {
                let (new_tuple, changed) = self.optimize_expr_internal(*tuple);
                (
                    IRExpr::TupleProj {
                        tuple: Box::new(new_tuple),
                        index,
                        ty,
                    },
                    changed,
                )
            }

            IRExpr::Record { fields, ty } => {
                let mut changed = false;
                let new_fields: Vec<_> = fields
                    .into_iter()
                    .map(|(name, e)| {
                        let (new_e, c) = self.optimize_expr_internal(e);
                        changed |= c;
                        (name, new_e)
                    })
                    .collect();
                (IRExpr::Record { fields: new_fields, ty }, changed)
            }

            IRExpr::Field { record, field, ty } => {
                let (new_record, changed) = self.optimize_expr_internal(*record);
                (
                    IRExpr::Field {
                        record: Box::new(new_record),
                        field,
                        ty,
                    },
                    changed,
                )
            }

            // Values don't need optimization
            IRExpr::Lit(_, _) | IRExpr::Var(_, _) | IRExpr::Global(_, _) | IRExpr::Unit => {
                (expr, false)
            }

            // Commands are passed through (we could optimize interpolations in the future)
            IRExpr::Command { parts, stdin, ty } => {
                let (opt_stdin, stdin_changed) = if let Some(s) = stdin {
                    let (opt_s, changed) = self.optimize_expr_internal(*s);
                    (Some(Box::new(opt_s)), changed)
                } else {
                    (None, false)
                };
                (
                    IRExpr::Command {
                        parts,
                        stdin: opt_stdin,
                        ty,
                    },
                    stdin_changed,
                )
            }
        }
    }

    /// Constant folding: evaluate constant expressions at compile time.
    fn constant_fold(&self, expr: IRExpr) -> (IRExpr, bool) {
        match &expr {
            IRExpr::Prim { op, args, ty } => {
                // Try to fold if all arguments are literals
                if let Some(result) = self.try_fold_prim(*op, args, ty) {
                    return (result, true);
                }
                (expr, false)
            }
            _ => (expr, false),
        }
    }

    /// Try to fold a primitive operation on constant arguments.
    fn try_fold_prim(&self, op: Primitive, args: &[IRExpr], ty: &Type) -> Option<IRExpr> {
        match op {
            // Integer arithmetic
            Primitive::AddInt => {
                if let (Some(a), Some(b)) = (self.as_int(&args[0]), self.as_int(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Int(a.wrapping_add(b)), ty.clone()));
                }
            }
            Primitive::SubInt => {
                if let (Some(a), Some(b)) = (self.as_int(&args[0]), self.as_int(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Int(a.wrapping_sub(b)), ty.clone()));
                }
            }
            Primitive::MulInt => {
                if let (Some(a), Some(b)) = (self.as_int(&args[0]), self.as_int(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Int(a.wrapping_mul(b)), ty.clone()));
                }
            }
            Primitive::DivInt => {
                if let (Some(a), Some(b)) = (self.as_int(&args[0]), self.as_int(&args[1]))
                    && b != 0
                {
                    return Some(IRExpr::Lit(IRLiteral::Int(a / b), ty.clone()));
                }
            }
            Primitive::ModInt => {
                if let (Some(a), Some(b)) = (self.as_int(&args[0]), self.as_int(&args[1]))
                    && b != 0
                {
                    return Some(IRExpr::Lit(IRLiteral::Int(a % b), ty.clone()));
                }
            }
            Primitive::NegInt => {
                if let Some(a) = self.as_int(&args[0]) {
                    return Some(IRExpr::Lit(IRLiteral::Int(-a), ty.clone()));
                }
            }

            // Float arithmetic
            Primitive::AddFloat => {
                if let (Some(a), Some(b)) = (self.as_float(&args[0]), self.as_float(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Float(a + b), ty.clone()));
                }
            }
            Primitive::SubFloat => {
                if let (Some(a), Some(b)) = (self.as_float(&args[0]), self.as_float(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Float(a - b), ty.clone()));
                }
            }
            Primitive::MulFloat => {
                if let (Some(a), Some(b)) = (self.as_float(&args[0]), self.as_float(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Float(a * b), ty.clone()));
                }
            }
            Primitive::DivFloat => {
                if let (Some(a), Some(b)) = (self.as_float(&args[0]), self.as_float(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Float(a / b), ty.clone()));
                }
            }
            Primitive::NegFloat => {
                if let Some(a) = self.as_float(&args[0]) {
                    return Some(IRExpr::Lit(IRLiteral::Float(-a), ty.clone()));
                }
            }

            // Integer comparison
            Primitive::EqInt => {
                if let (Some(a), Some(b)) = (self.as_int(&args[0]), self.as_int(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Bool(a == b), ty.clone()));
                }
            }
            Primitive::NeqInt => {
                if let (Some(a), Some(b)) = (self.as_int(&args[0]), self.as_int(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Bool(a != b), ty.clone()));
                }
            }
            Primitive::LtInt => {
                if let (Some(a), Some(b)) = (self.as_int(&args[0]), self.as_int(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Bool(a < b), ty.clone()));
                }
            }
            Primitive::LeInt => {
                if let (Some(a), Some(b)) = (self.as_int(&args[0]), self.as_int(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Bool(a <= b), ty.clone()));
                }
            }
            Primitive::GtInt => {
                if let (Some(a), Some(b)) = (self.as_int(&args[0]), self.as_int(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Bool(a > b), ty.clone()));
                }
            }
            Primitive::GeInt => {
                if let (Some(a), Some(b)) = (self.as_int(&args[0]), self.as_int(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Bool(a >= b), ty.clone()));
                }
            }

            // Boolean comparison
            Primitive::EqBool => {
                if let (Some(a), Some(b)) = (self.as_bool(&args[0]), self.as_bool(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Bool(a == b), ty.clone()));
                }
            }
            Primitive::NeqBool => {
                if let (Some(a), Some(b)) = (self.as_bool(&args[0]), self.as_bool(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::Bool(a != b), ty.clone()));
                }
            }

            // Logical
            Primitive::Not => {
                if let Some(a) = self.as_bool(&args[0]) {
                    return Some(IRExpr::Lit(IRLiteral::Bool(!a), ty.clone()));
                }
            }

            // String
            Primitive::Concat => {
                if let (Some(a), Some(b)) = (self.as_string(&args[0]), self.as_string(&args[1])) {
                    return Some(IRExpr::Lit(IRLiteral::String(a + &b), ty.clone()));
                }
            }

            _ => {}
        }

        None
    }

    /// Extract integer from a literal expression.
    fn as_int(&self, expr: &IRExpr) -> Option<i64> {
        match expr {
            IRExpr::Lit(IRLiteral::Int(n), _) => Some(*n),
            _ => None,
        }
    }

    /// Extract float from a literal expression.
    fn as_float(&self, expr: &IRExpr) -> Option<f64> {
        match expr {
            IRExpr::Lit(IRLiteral::Float(f), _) => Some(*f),
            _ => None,
        }
    }

    /// Extract boolean from a literal expression.
    fn as_bool(&self, expr: &IRExpr) -> Option<bool> {
        match expr {
            IRExpr::Lit(IRLiteral::Bool(b), _) => Some(*b),
            _ => None,
        }
    }

    /// Extract string from a literal expression.
    fn as_string(&self, expr: &IRExpr) -> Option<String> {
        match expr {
            IRExpr::Lit(IRLiteral::String(s), _) => Some(s.clone()),
            _ => None,
        }
    }

    /// Simplify if expressions with constant conditions.
    fn simplify_if(&self, expr: IRExpr) -> (IRExpr, bool) {
        match expr {
            IRExpr::If { cond, then_branch, else_branch, ty } => {
                match cond.as_ref() {
                    IRExpr::Lit(IRLiteral::Bool(true), _) => (*then_branch, true),
                    IRExpr::Lit(IRLiteral::Bool(false), _) => (*else_branch, true),
                    _ => (
                        IRExpr::If {
                            cond,
                            then_branch,
                            else_branch,
                            ty,
                        },
                        false,
                    ),
                }
            }
            _ => (expr, false),
        }
    }

    /// Simple beta reduction: inline trivial let bindings.
    fn beta_reduce(&self, expr: IRExpr) -> (IRExpr, bool) {
        match expr {
            IRExpr::Let { var, ty, value, body } => {
                // Only inline if value is simple (literal or variable)
                // and var is used at most once
                if value.is_value() {
                    let uses = count_uses(var, &body);
                    if uses == 0 {
                        // Dead binding, remove it
                        return (*body, true);
                    } else if uses == 1 {
                        // Single use, inline it
                        let inlined = substitute(var, &value, *body);
                        return (inlined, true);
                    }
                }
                (
                    IRExpr::Let {
                        var,
                        ty,
                        value,
                        body,
                    },
                    false,
                )
            }
            _ => (expr, false),
        }
    }

    /// Dead code elimination for let bindings.
    fn dead_code_elimination(&self, expr: IRExpr) -> (IRExpr, bool) {
        match &expr {
            IRExpr::Let { var, body, .. } => {
                // If variable is unused in body, remove the binding
                if count_uses(*var, body) == 0 {
                    // Need to move out of the expr
                    if let IRExpr::Let { body, .. } = expr {
                        return (*body, true);
                    }
                }
                (expr, false)
            }
            _ => (expr, false),
        }
    }
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Count uses of a variable in an expression.
fn count_uses(var: VarId, expr: &IRExpr) -> usize {
    match expr {
        IRExpr::Var(v, _) if *v == var => 1,
        IRExpr::Var(_, _) | IRExpr::Global(_, _) | IRExpr::Lit(_, _) | IRExpr::Unit => 0,

        IRExpr::Let { var: bound_var, value, body, .. } => {
            let in_value = count_uses(var, value);
            if *bound_var == var {
                // Shadowed in body
                in_value
            } else {
                in_value + count_uses(var, body)
            }
        }

        IRExpr::LetRec { bindings, body } => {
            let in_bindings: usize = bindings.iter().map(|b| count_uses(var, &b.value)).sum();
            let shadowed = bindings.iter().any(|b| b.var == var);
            if shadowed {
                in_bindings
            } else {
                in_bindings + count_uses(var, body)
            }
        }

        IRExpr::Lambda { param, body, .. } => {
            if *param == var {
                0
            } else {
                count_uses(var, body)
            }
        }

        IRExpr::App { func, arg, .. } => count_uses(var, func) + count_uses(var, arg),

        IRExpr::If { cond, then_branch, else_branch, .. } => {
            count_uses(var, cond) + count_uses(var, then_branch) + count_uses(var, else_branch)
        }

        IRExpr::Match { scrutinee, arms, .. } => {
            let in_scrutinee = count_uses(var, scrutinee);
            let in_arms: usize = arms
                .iter()
                .map(|(pat, body)| {
                    if pattern_binds(pat, var) {
                        0
                    } else {
                        count_uses(var, body)
                    }
                })
                .sum();
            in_scrutinee + in_arms
        }

        IRExpr::Prim { args, .. } => args.iter().map(|a| count_uses(var, a)).sum(),

        IRExpr::Construct { arg, .. } => {
            arg.as_ref().map_or(0, |a| count_uses(var, a))
        }

        IRExpr::Tuple { elems, .. } => elems.iter().map(|e| count_uses(var, e)).sum(),

        IRExpr::TupleProj { tuple, .. } => count_uses(var, tuple),

        IRExpr::Record { fields, .. } => {
            fields.iter().map(|(_, e)| count_uses(var, e)).sum()
        }

        IRExpr::Field { record, .. } => count_uses(var, record),

        IRExpr::Command { parts, stdin, .. } => {
            let in_parts: usize = parts
                .iter()
                .map(|p| match p {
                    crate::ir::IRCommandPart::Literal(_) => 0,
                    crate::ir::IRCommandPart::Interpolation(e) => count_uses(var, e),
                })
                .sum();
            let in_stdin = stdin.as_ref().map_or(0, |s| count_uses(var, s));
            in_parts + in_stdin
        }
    }
}

/// Check if a pattern binds a variable.
fn pattern_binds(pattern: &IRPattern, var: VarId) -> bool {
    match pattern {
        IRPattern::Var(v, _) => *v == var,
        IRPattern::Tuple(elems) => elems.iter().any(|p| pattern_binds(p, var)),
        IRPattern::Record(fields) => fields.iter().any(|(_, p)| pattern_binds(p, var)),
        IRPattern::Con { arg, .. } => {
            arg.as_ref().is_some_and(|p| pattern_binds(p, var))
        }
        IRPattern::Wildcard | IRPattern::Lit(_) => false,
    }
}

/// Substitute a variable with an expression.
fn substitute(var: VarId, replacement: &IRExpr, expr: IRExpr) -> IRExpr {
    match expr {
        IRExpr::Var(v, _) if v == var => replacement.clone(),
        IRExpr::Var(_, _) | IRExpr::Global(_, _) | IRExpr::Lit(_, _) | IRExpr::Unit => expr,

        IRExpr::Let { var: bound_var, ty, value, body } => {
            let new_value = Box::new(substitute(var, replacement, *value));
            let new_body = if bound_var == var {
                body
            } else {
                Box::new(substitute(var, replacement, *body))
            };
            IRExpr::Let {
                var: bound_var,
                ty,
                value: new_value,
                body: new_body,
            }
        }

        IRExpr::LetRec { bindings, body } => {
            let shadowed = bindings.iter().any(|b| b.var == var);
            let new_bindings: Vec<_> = bindings
                .into_iter()
                .map(|b| Binding {
                    var: b.var,
                    ty: b.ty,
                    value: if shadowed {
                        b.value
                    } else {
                        substitute(var, replacement, b.value)
                    },
                })
                .collect();
            let new_body = if shadowed {
                body
            } else {
                Box::new(substitute(var, replacement, *body))
            };
            IRExpr::LetRec {
                bindings: new_bindings,
                body: new_body,
            }
        }

        IRExpr::Lambda { param, param_ty, body, result_ty } => {
            if param == var {
                IRExpr::Lambda { param, param_ty, body, result_ty }
            } else {
                IRExpr::Lambda {
                    param,
                    param_ty,
                    body: Box::new(substitute(var, replacement, *body)),
                    result_ty,
                }
            }
        }

        IRExpr::App { func, arg, result_ty } => IRExpr::App {
            func: Box::new(substitute(var, replacement, *func)),
            arg: Box::new(substitute(var, replacement, *arg)),
            result_ty,
        },

        IRExpr::If { cond, then_branch, else_branch, ty } => IRExpr::If {
            cond: Box::new(substitute(var, replacement, *cond)),
            then_branch: Box::new(substitute(var, replacement, *then_branch)),
            else_branch: Box::new(substitute(var, replacement, *else_branch)),
            ty,
        },

        IRExpr::Match { scrutinee, arms, ty } => {
            let new_scrutinee = Box::new(substitute(var, replacement, *scrutinee));
            let new_arms: Vec<_> = arms
                .into_iter()
                .map(|(pat, body)| {
                    let new_body = if pattern_binds(&pat, var) {
                        body
                    } else {
                        substitute(var, replacement, body)
                    };
                    (pat, new_body)
                })
                .collect();
            IRExpr::Match {
                scrutinee: new_scrutinee,
                arms: new_arms,
                ty,
            }
        }

        IRExpr::Prim { op, args, ty } => IRExpr::Prim {
            op,
            args: args
                .into_iter()
                .map(|a| substitute(var, replacement, a))
                .collect(),
            ty,
        },

        IRExpr::Construct { ctor, arg, ty } => IRExpr::Construct {
            ctor,
            arg: arg.map(|a| Box::new(substitute(var, replacement, *a))),
            ty,
        },

        IRExpr::Tuple { elems, ty } => IRExpr::Tuple {
            elems: elems
                .into_iter()
                .map(|e| substitute(var, replacement, e))
                .collect(),
            ty,
        },

        IRExpr::TupleProj { tuple, index, ty } => IRExpr::TupleProj {
            tuple: Box::new(substitute(var, replacement, *tuple)),
            index,
            ty,
        },

        IRExpr::Record { fields, ty } => IRExpr::Record {
            fields: fields
                .into_iter()
                .map(|(n, e)| (n, substitute(var, replacement, e)))
                .collect(),
            ty,
        },

        IRExpr::Field { record, field, ty } => IRExpr::Field {
            record: Box::new(substitute(var, replacement, *record)),
            field,
            ty,
        },

        IRExpr::Command { parts, stdin, ty } => {
            let new_parts: Vec<_> = parts
                .into_iter()
                .map(|p| match p {
                    crate::ir::IRCommandPart::Literal(s) => crate::ir::IRCommandPart::Literal(s),
                    crate::ir::IRCommandPart::Interpolation(e) => {
                        crate::ir::IRCommandPart::Interpolation(Box::new(substitute(
                            var,
                            replacement,
                            *e,
                        )))
                    }
                })
                .collect();
            let new_stdin = stdin.map(|s| Box::new(substitute(var, replacement, *s)));
            IRExpr::Command {
                parts: new_parts,
                stdin: new_stdin,
                ty,
            }
        }
    }
}

/// Collect all free variables in an expression.
#[allow(dead_code)]
fn free_vars(expr: &IRExpr) -> HashSet<VarId> {
    let mut vars = HashSet::new();
    collect_free_vars(expr, &mut vars, &HashSet::new());
    vars
}

/// Helper for collecting free variables.
fn collect_free_vars(expr: &IRExpr, vars: &mut HashSet<VarId>, bound: &HashSet<VarId>) {
    match expr {
        IRExpr::Var(v, _) => {
            if !bound.contains(v) {
                vars.insert(*v);
            }
        }
        IRExpr::Global(_, _) | IRExpr::Lit(_, _) | IRExpr::Unit => {}

        IRExpr::Let { var, value, body, .. } => {
            collect_free_vars(value, vars, bound);
            let mut new_bound = bound.clone();
            new_bound.insert(*var);
            collect_free_vars(body, vars, &new_bound);
        }

        IRExpr::LetRec { bindings, body } => {
            let mut new_bound = bound.clone();
            for b in bindings {
                new_bound.insert(b.var);
            }
            for b in bindings {
                collect_free_vars(&b.value, vars, &new_bound);
            }
            collect_free_vars(body, vars, &new_bound);
        }

        IRExpr::Lambda { param, body, .. } => {
            let mut new_bound = bound.clone();
            new_bound.insert(*param);
            collect_free_vars(body, vars, &new_bound);
        }

        IRExpr::App { func, arg, .. } => {
            collect_free_vars(func, vars, bound);
            collect_free_vars(arg, vars, bound);
        }

        IRExpr::If { cond, then_branch, else_branch, .. } => {
            collect_free_vars(cond, vars, bound);
            collect_free_vars(then_branch, vars, bound);
            collect_free_vars(else_branch, vars, bound);
        }

        IRExpr::Match { scrutinee, arms, .. } => {
            collect_free_vars(scrutinee, vars, bound);
            for (pat, body) in arms {
                let mut new_bound = bound.clone();
                collect_pattern_vars(pat, &mut new_bound);
                collect_free_vars(body, vars, &new_bound);
            }
        }

        IRExpr::Prim { args, .. } => {
            for arg in args {
                collect_free_vars(arg, vars, bound);
            }
        }

        IRExpr::Construct { arg, .. } => {
            if let Some(a) = arg {
                collect_free_vars(a, vars, bound);
            }
        }

        IRExpr::Tuple { elems, .. } => {
            for e in elems {
                collect_free_vars(e, vars, bound);
            }
        }

        IRExpr::TupleProj { tuple, .. } => {
            collect_free_vars(tuple, vars, bound);
        }

        IRExpr::Record { fields, .. } => {
            for (_, e) in fields {
                collect_free_vars(e, vars, bound);
            }
        }

        IRExpr::Field { record, .. } => {
            collect_free_vars(record, vars, bound);
        }

        IRExpr::Command { parts, stdin, .. } => {
            for part in parts {
                if let crate::ir::IRCommandPart::Interpolation(e) = part {
                    collect_free_vars(e, vars, bound);
                }
            }
            if let Some(s) = stdin {
                collect_free_vars(s, vars, bound);
            }
        }
    }
}

/// Collect variables bound by a pattern.
fn collect_pattern_vars(pattern: &IRPattern, vars: &mut HashSet<VarId>) {
    match pattern {
        IRPattern::Var(v, _) => {
            vars.insert(*v);
        }
        IRPattern::Tuple(elems) => {
            for e in elems {
                collect_pattern_vars(e, vars);
            }
        }
        IRPattern::Record(fields) => {
            for (_, p) in fields {
                collect_pattern_vars(p, vars);
            }
        }
        IRPattern::Con { arg, .. } => {
            if let Some(a) = arg {
                collect_pattern_vars(a, vars);
            }
        }
        IRPattern::Wildcard | IRPattern::Lit(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Lower;
    use crate::syntax::Parser;

    fn lower_and_optimize(source: &str) -> IRProgram {
        let mut parser = Parser::new(source).expect("lexer error");
        let program = parser.parse_program().expect("parse error");
        let mut lower = Lower::new();
        let ir = lower.lower_program(&program).expect("lower error");
        let optimizer = Optimizer::new();
        optimizer.optimize(ir)
    }

    #[test]
    fn optimize_constant_add() {
        let ir = lower_and_optimize("let x = 1 + 2");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                // Should be folded to literal 3
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Int(3), _)));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn optimize_constant_mul() {
        let ir = lower_and_optimize("let x = 3 * 4");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Int(12), _)));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn optimize_constant_sub() {
        let ir = lower_and_optimize("let x = 10 - 3");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Int(7), _)));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn optimize_constant_div() {
        let ir = lower_and_optimize("let x = 10 / 2");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Int(5), _)));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn optimize_constant_comparison() {
        let ir = lower_and_optimize("let b = 1 < 2");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Bool(true), _)));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn optimize_constant_negation() {
        let ir = lower_and_optimize("let x = -42");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Int(-42), _)));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn optimize_constant_not() {
        let ir = lower_and_optimize("let b = not true");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Bool(false), _)));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn optimize_if_true() {
        let ir = lower_and_optimize("let x = if true then 1 else 2");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                // Should simplify to just 1
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Int(1), _)));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn optimize_if_false() {
        let ir = lower_and_optimize("let x = if false then 1 else 2");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                // Should simplify to just 2
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Int(2), _)));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn optimize_nested_constant() {
        let ir = lower_and_optimize("let x = (1 + 2) * (3 + 4)");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                // Should fold to 3 * 7 = 21
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Int(21), _)));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn optimize_constant_string_concat() {
        let ir = lower_and_optimize("let s = \"hello\" ++ \" world\"");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Lit(IRLiteral::String(s), _) if s == "hello world"));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn optimize_float_arithmetic() {
        let ir = lower_and_optimize("let x = 1.5 + 2.5");

        match &ir.decls[0] {
            IRDecl::Val { value, .. } => {
                assert!(matches!(value, IRExpr::Lit(IRLiteral::Float(f), _) if (*f - 4.0).abs() < 0.001));
            }
            _ => panic!("expected val decl"),
        }
    }

    #[test]
    fn no_optimize_variable() {
        let ir = lower_and_optimize("let x = 1\nlet y = x + 1");

        // Can't fold x + 1 because x is not a constant in this context
        match &ir.decls[1] {
            IRDecl::Val { value, .. } => {
                // Should still be a primitive operation
                assert!(matches!(value, IRExpr::Prim { .. }));
            }
            _ => panic!("expected val decl"),
        }
    }
}
