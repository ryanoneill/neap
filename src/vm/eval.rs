//! Expression evaluator for the Neap VM
//!
//! A tree-walking interpreter that evaluates IR expressions directly.
//! The ANF form makes this straightforward since all intermediate values
//! are explicitly named via Let bindings.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::rc::Rc;

use crate::ir::{Binding, IRDecl, IRExpr, IRLiteral, IRProgram, VarId};

use super::env::Env;
use super::error::RuntimeError;
use super::pattern::match_pattern;
use super::primitive::eval_primitive;
use super::value::Value;

/// The virtual machine state.
pub struct VM<W: Write, R: BufRead> {
    /// Global variable bindings (top-level definitions)
    globals: HashMap<String, Value>,
    /// Standard output stream
    stdout: W,
    /// Standard input stream
    stdin: R,
    /// Maximum recursion depth
    max_depth: usize,
    /// Current recursion depth
    current_depth: usize,
}

impl<W: Write, R: BufRead> VM<W, R> {
    /// Create a new VM with custom IO streams.
    pub fn new(stdout: W, stdin: R) -> Self {
        Self {
            globals: HashMap::new(),
            stdout,
            stdin,
            max_depth: 1000,
            current_depth: 0,
        }
    }

    /// Set the maximum recursion depth.
    pub fn set_max_depth(&mut self, depth: usize) {
        self.max_depth = depth;
    }

    /// Define a global variable.
    pub fn define_global(&mut self, name: impl Into<String>, value: Value) {
        self.globals.insert(name.into(), value);
    }

    /// Look up a global variable.
    pub fn lookup_global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    /// Get a mutable reference to stdout.
    pub fn stdout(&mut self) -> &mut W {
        &mut self.stdout
    }

    /// Get a mutable reference to stdin.
    pub fn stdin(&mut self) -> &mut R {
        &mut self.stdin
    }

    /// Consume the VM and return the stdout writer.
    pub fn into_writer(self) -> W {
        self.stdout
    }

    /// Evaluate a program.
    pub fn eval_program(&mut self, program: &IRProgram) -> Result<(), RuntimeError> {
        for decl in &program.decls {
            self.eval_decl(decl)?;
        }
        Ok(())
    }

    /// Evaluate a standalone expression (for REPL use).
    ///
    /// Global variables are accessed via IRExpr::Global, so we just
    /// need an empty local environment.
    pub fn eval_expr_standalone(&mut self, expr: &IRExpr) -> Result<Value, RuntimeError> {
        let env = Rc::new(Env::new());
        self.eval_expr(expr, &env)
    }

    /// Evaluate a standalone declaration (for REPL use).
    pub fn eval_decl_standalone(&mut self, decl: &IRDecl) -> Result<(), RuntimeError> {
        self.eval_decl(decl)
    }

    /// Evaluate a declaration.
    fn eval_decl(&mut self, decl: &IRDecl) -> Result<(), RuntimeError> {
        match decl {
            IRDecl::Val { name, value, .. } => {
                let env = Rc::new(Env::new());
                let val = self.eval_expr(value, &env)?;
                self.define_global(name.clone(), val);
            }
            IRDecl::Fun {
                name, params, body, ..
            } => {
                // Create a closure for the function
                // For multi-parameter functions, we curry them
                let env = Rc::new(Env::new());

                if params.is_empty() {
                    // Thunk - evaluate immediately
                    let val = self.eval_expr(body, &env)?;
                    self.define_global(name.clone(), val);
                } else if params.len() == 1 {
                    // Single parameter - straightforward closure
                    let (param, _) = &params[0];
                    let closure = Value::Closure {
                        param: *param,
                        body: Rc::new(body.clone()),
                        env,
                    };
                    self.define_global(name.clone(), closure);
                } else {
                    // Multi-parameter - curry into nested closures
                    let closure = self.curry_function(params, body, &env);
                    self.define_global(name.clone(), closure);
                }
            }
            IRDecl::Data { .. } => {
                // Data declarations don't produce runtime values
                // Constructors are handled by IRExpr::Construct
            }
        }
        Ok(())
    }

    /// Curry a multi-parameter function into nested closures.
    fn curry_function(&self, params: &[(VarId, crate::types::Type)], body: &IRExpr, env: &Rc<Env>) -> Value {
        if params.len() == 1 {
            Value::Closure {
                param: params[0].0,
                body: Rc::new(body.clone()),
                env: Rc::clone(env),
            }
        } else {
            // Create nested lambdas for currying
            let (first_param, _) = &params[0];
            let rest_params = &params[1..];

            // Build the inner body that captures remaining params
            let inner_body = self.build_curried_body(rest_params, body);

            Value::Closure {
                param: *first_param,
                body: Rc::new(inner_body),
                env: Rc::clone(env),
            }
        }
    }

    /// Build a curried function body for remaining parameters.
    fn build_curried_body(&self, params: &[(VarId, crate::types::Type)], body: &IRExpr) -> IRExpr {
        if params.len() == 1 {
            let (param, param_ty) = &params[0];
            IRExpr::Lambda {
                param: *param,
                param_ty: param_ty.clone(),
                body: Box::new(body.clone()),
                result_ty: body.ty(),
            }
        } else {
            let (first_param, first_ty) = &params[0];
            let rest = &params[1..];
            let inner = self.build_curried_body(rest, body);
            IRExpr::Lambda {
                param: *first_param,
                param_ty: first_ty.clone(),
                body: Box::new(inner.clone()),
                result_ty: inner.ty(),
            }
        }
    }

    /// Evaluate an expression in the given environment.
    pub fn eval_expr(&mut self, expr: &IRExpr, env: &Rc<Env>) -> Result<Value, RuntimeError> {
        // Check recursion depth
        if self.current_depth >= self.max_depth {
            return Err(RuntimeError::StackOverflow);
        }

        match expr {
            IRExpr::Lit(lit, _) => Ok(self.eval_literal(lit)),

            IRExpr::Var(var, _) => env
                .lookup(*var)
                .cloned()
                .ok_or(RuntimeError::UnboundVariable { var: *var }),

            IRExpr::Global(name, _) => self
                .lookup_global(name)
                .cloned()
                .ok_or_else(|| RuntimeError::UnboundGlobal { name: name.clone() }),

            IRExpr::Unit => Ok(Value::Unit),

            IRExpr::Let { var, value, body, .. } => {
                let val = self.eval_expr(value, env)?;
                let new_env = Rc::new(env.extend(*var, val));
                self.eval_expr(body, &new_env)
            }

            IRExpr::LetRec { bindings, body } => {
                // Create environment with all recursive bindings
                let rec_env = self.eval_letrec_bindings(bindings, env)?;
                self.eval_expr(body, &rec_env)
            }

            IRExpr::If { cond, then_branch, else_branch, .. } => {
                let cond_val = self.eval_expr(cond, env)?;
                if cond_val.is_truthy() {
                    self.eval_expr(then_branch, env)
                } else {
                    self.eval_expr(else_branch, env)
                }
            }

            IRExpr::Lambda { param, body, .. } => {
                // Capture the current environment in the closure
                Ok(Value::Closure {
                    param: *param,
                    body: Rc::new(body.as_ref().clone()),
                    env: Rc::clone(env),
                })
            }

            IRExpr::App { func, arg, .. } => {
                let func_val = self.eval_expr(func, env)?;
                let arg_val = self.eval_expr(arg, env)?;

                match func_val {
                    Value::Closure {
                        param,
                        body,
                        env: closure_env,
                    } => {
                        self.current_depth += 1;
                        let call_env = Rc::new(closure_env.extend(param, arg_val));
                        let result = self.eval_expr(&body, &call_env);
                        self.current_depth -= 1;
                        result
                    }
                    _ => Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        got: func_val.type_name().to_string(),
                    }),
                }
            }

            IRExpr::Prim { op, args, .. } => {
                let mut arg_vals = Vec::with_capacity(args.len());
                for arg in args {
                    arg_vals.push(self.eval_expr(arg, env)?);
                }
                eval_primitive(self, *op, &arg_vals)
            }

            IRExpr::Match { scrutinee, arms, .. } => {
                let scrutinee_val = self.eval_expr(scrutinee, env)?;

                for (pattern, body) in arms {
                    if let Some(bindings) = match_pattern(pattern, &scrutinee_val) {
                        let new_env = Rc::new(env.extend_many(bindings));
                        return self.eval_expr(body, &new_env);
                    }
                }

                Err(RuntimeError::MatchFailure)
            }

            IRExpr::Construct { ctor, arg, .. } => {
                let payload = if let Some(arg_expr) = arg {
                    Some(Box::new(self.eval_expr(arg_expr, env)?))
                } else {
                    None
                };

                Ok(Value::Constructor {
                    tag: ctor.clone(),
                    payload,
                })
            }

            IRExpr::Tuple { elems, .. } => {
                let mut values = Vec::with_capacity(elems.len());
                for elem in elems {
                    values.push(self.eval_expr(elem, env)?);
                }
                Ok(Value::tuple(values))
            }

            IRExpr::TupleProj { tuple, index, .. } => {
                let tuple_val = self.eval_expr(tuple, env)?;
                match tuple_val {
                    Value::Tuple(elems) => {
                        if *index < elems.len() {
                            Ok(elems[*index].clone())
                        } else {
                            Err(RuntimeError::IndexOutOfBounds {
                                index: *index,
                                len: elems.len(),
                            })
                        }
                    }
                    _ => Err(RuntimeError::TypeError {
                        expected: "tuple".to_string(),
                        got: tuple_val.type_name().to_string(),
                    }),
                }
            }

            IRExpr::Record { fields, .. } => {
                let mut field_vals = HashMap::new();
                for (name, expr) in fields {
                    field_vals.insert(name.clone(), self.eval_expr(expr, env)?);
                }
                Ok(Value::record(field_vals))
            }

            IRExpr::Field { record, field, .. } => {
                let record_val = self.eval_expr(record, env)?;
                match record_val {
                    Value::Record(fields) => fields
                        .get(field)
                        .cloned()
                        .ok_or_else(|| RuntimeError::UnknownField {
                            field: field.clone(),
                        }),
                    _ => Err(RuntimeError::TypeError {
                        expected: "record".to_string(),
                        got: record_val.type_name().to_string(),
                    }),
                }
            }

            IRExpr::Command { parts, stdin, .. } => {
                self.eval_command(parts, stdin.as_deref(), env)
            }
        }
    }

    /// Evaluate a shell command.
    fn eval_command(
        &mut self,
        parts: &[crate::ir::IRCommandPart],
        stdin: Option<&IRExpr>,
        env: &Rc<Env>,
    ) -> Result<Value, RuntimeError> {
        use std::collections::HashMap;
        use std::io::Write;
        use std::process::{Command, Stdio};

        // Build the command string from parts
        let mut cmd_str = String::new();
        for part in parts {
            match part {
                crate::ir::IRCommandPart::Literal(s) => cmd_str.push_str(s),
                crate::ir::IRCommandPart::Interpolation(expr) => {
                    let val = self.eval_expr(expr, env)?;
                    cmd_str.push_str(&format!("{val}"));
                }
            }
        }

        // Evaluate stdin if provided
        let stdin_content = if let Some(stdin_expr) = stdin {
            let stdin_val = self.eval_expr(stdin_expr, env)?;
            // If stdin is a command result, use its stdout
            if let Value::Record(fields) = &stdin_val {
                fields.get("stdout").map(|v| format!("{v}"))
            } else {
                Some(format!("{stdin_val}"))
            }
        } else {
            None
        };

        // Parse command string - simple whitespace split for now
        // TODO: proper shell parsing with quotes, etc.
        let mut parts_iter = cmd_str.split_whitespace();
        let program = parts_iter.next().ok_or_else(|| RuntimeError::CommandError {
            message: "empty command".to_string(),
        })?;
        let args: Vec<&str> = parts_iter.collect();

        // Execute the command
        let mut cmd = Command::new(program);
        cmd.args(&args);

        if stdin_content.is_some() {
            cmd.stdin(Stdio::piped());
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| RuntimeError::CommandError {
            message: format!("failed to spawn command '{}': {}", program, e),
        })?;

        // Write stdin if provided
        if let Some(input) = &stdin_content {
            if let Some(mut stdin_handle) = child.stdin.take() {
                stdin_handle.write_all(input.as_bytes()).map_err(|e| {
                    RuntimeError::CommandError {
                        message: format!("failed to write to stdin: {}", e),
                    }
                })?;
            }
        }

        let output = child.wait_with_output().map_err(|e| RuntimeError::CommandError {
            message: format!("failed to wait for command: {}", e),
        })?;

        // Build the result record
        let mut fields = HashMap::new();
        fields.insert(
            "exitCode".to_string(),
            Value::Int(output.status.code().unwrap_or(-1) as i64),
        );
        fields.insert(
            "stdout".to_string(),
            Value::string(String::from_utf8_lossy(&output.stdout).into_owned()),
        );
        fields.insert(
            "stderr".to_string(),
            Value::string(String::from_utf8_lossy(&output.stderr).into_owned()),
        );

        Ok(Value::record(fields))
    }

    /// Evaluate letrec bindings, handling mutual recursion.
    fn eval_letrec_bindings(
        &mut self,
        bindings: &[Binding],
        env: &Rc<Env>,
    ) -> Result<Rc<Env>, RuntimeError> {
        // First pass: create placeholder closures for all bindings
        let mut new_bindings: Vec<(VarId, Value)> = Vec::new();

        for binding in bindings {
            // For now, assume all letrec bindings are functions (lambdas)
            // This is the common case and avoids the need for mutable cells
            match &binding.value {
                IRExpr::Lambda { param, body, .. } => {
                    // Create a closure that will capture the final environment
                    // We use a placeholder env for now
                    new_bindings.push((
                        binding.var,
                        Value::Closure {
                            param: *param,
                            body: Rc::new(body.as_ref().clone()),
                            env: Rc::clone(env), // Placeholder
                        },
                    ));
                }
                _ => {
                    // For non-lambda bindings, evaluate immediately
                    // This works for simple recursive values
                    let val = self.eval_expr(&binding.value, env)?;
                    new_bindings.push((binding.var, val));
                }
            }
        }

        // Create the recursive environment
        let rec_env = Rc::new(env.extend_many(new_bindings.clone()));

        // Second pass: update closures to capture the recursive environment
        let mut final_bindings: Vec<(VarId, Value)> = Vec::new();
        for binding in bindings {
            match &binding.value {
                IRExpr::Lambda { param, body, .. } => {
                    final_bindings.push((
                        binding.var,
                        Value::Closure {
                            param: *param,
                            body: Rc::new(body.as_ref().clone()),
                            env: Rc::clone(&rec_env),
                        },
                    ));
                }
                _ => {
                    // Keep the already evaluated value
                    if let Some((_, val)) = new_bindings.iter().find(|(v, _)| *v == binding.var) {
                        final_bindings.push((binding.var, val.clone()));
                    }
                }
            }
        }

        Ok(Rc::new(env.extend_many(final_bindings)))
    }

    /// Evaluate a literal to a value.
    fn eval_literal(&self, lit: &IRLiteral) -> Value {
        match lit {
            IRLiteral::Bool(b) => Value::Bool(*b),
            IRLiteral::Int(n) => Value::Int(*n),
            IRLiteral::Float(f) => Value::Float(*f),
            IRLiteral::Char(c) => Value::Char(*c),
            IRLiteral::String(s) => Value::string(s.clone()),
        }
    }
}

/// Create a VM with standard IO.
impl Default for VM<std::io::Stdout, std::io::StdinLock<'static>> {
    fn default() -> Self {
        Self::new(std::io::stdout(), std::io::stdin().lock())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{IRCommandPart, Primitive};
    use crate::types::Type;
    use std::io::Cursor;

    fn test_vm() -> VM<Vec<u8>, Cursor<Vec<u8>>> {
        VM::new(Vec::new(), Cursor::new(Vec::new()))
    }

    #[test]
    fn eval_literal_int() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let expr = IRExpr::Lit(IRLiteral::Int(42), Type::int());

        let result = vm.eval_expr(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn eval_literal_bool() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());

        let expr = IRExpr::Lit(IRLiteral::Bool(true), Type::bool());
        assert_eq!(vm.eval_expr(&expr, &env).unwrap(), Value::Bool(true));

        let expr = IRExpr::Lit(IRLiteral::Bool(false), Type::bool());
        assert_eq!(vm.eval_expr(&expr, &env).unwrap(), Value::Bool(false));
    }

    #[test]
    fn eval_literal_string() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let expr = IRExpr::Lit(IRLiteral::String("hello".to_string()), Type::string());

        let result = vm.eval_expr(&expr, &env).unwrap();
        assert_eq!(result.as_string(), Some("hello"));
    }

    #[test]
    fn eval_unit() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let expr = IRExpr::Unit;

        let result = vm.eval_expr(&expr, &env).unwrap();
        assert_eq!(result, Value::Unit);
    }

    #[test]
    fn eval_variable() {
        let mut vm = test_vm();
        let var = VarId::fresh();
        let mut base_env = Env::new();
        base_env.bind(var, Value::Int(42));
        let env = Rc::new(base_env);

        let expr = IRExpr::Var(var, Type::int());
        assert_eq!(vm.eval_expr(&expr, &env).unwrap(), Value::Int(42));
    }

    #[test]
    fn eval_unbound_variable() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let var = VarId::fresh();
        let expr = IRExpr::Var(var, Type::int());

        let result = vm.eval_expr(&expr, &env);
        assert!(matches!(result, Err(RuntimeError::UnboundVariable { .. })));
    }

    #[test]
    fn eval_global() {
        let mut vm = test_vm();
        vm.define_global("x", Value::Int(42));
        let env = Rc::new(Env::new());

        let expr = IRExpr::Global("x".to_string(), Type::int());
        assert_eq!(vm.eval_expr(&expr, &env).unwrap(), Value::Int(42));
    }

    #[test]
    fn eval_let() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let var = VarId::fresh();

        // let x = 42 in x
        let expr = IRExpr::Let {
            var,
            ty: Type::int(),
            value: Box::new(IRExpr::Lit(IRLiteral::Int(42), Type::int())),
            body: Box::new(IRExpr::Var(var, Type::int())),
        };

        assert_eq!(vm.eval_expr(&expr, &env).unwrap(), Value::Int(42));
    }

    #[test]
    fn eval_nested_let() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let x = VarId::fresh();
        let y = VarId::fresh();

        // let x = 1 in let y = 2 in (x, y)
        let tuple_ty = Type::Tuple(vec![Type::int(), Type::int()]);
        let expr = IRExpr::Let {
            var: x,
            ty: Type::int(),
            value: Box::new(IRExpr::Lit(IRLiteral::Int(1), Type::int())),
            body: Box::new(IRExpr::Let {
                var: y,
                ty: Type::int(),
                value: Box::new(IRExpr::Lit(IRLiteral::Int(2), Type::int())),
                body: Box::new(IRExpr::Tuple {
                    elems: vec![IRExpr::Var(x, Type::int()), IRExpr::Var(y, Type::int())],
                    ty: tuple_ty,
                }),
            }),
        };

        let result = vm.eval_expr(&expr, &env).unwrap();
        assert!(matches!(result, Value::Tuple(_)));
    }

    #[test]
    fn eval_if_true() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());

        let expr = IRExpr::If {
            cond: Box::new(IRExpr::Lit(IRLiteral::Bool(true), Type::bool())),
            then_branch: Box::new(IRExpr::Lit(IRLiteral::Int(1), Type::int())),
            else_branch: Box::new(IRExpr::Lit(IRLiteral::Int(2), Type::int())),
            ty: Type::int(),
        };

        assert_eq!(vm.eval_expr(&expr, &env).unwrap(), Value::Int(1));
    }

    #[test]
    fn eval_if_false() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());

        let expr = IRExpr::If {
            cond: Box::new(IRExpr::Lit(IRLiteral::Bool(false), Type::bool())),
            then_branch: Box::new(IRExpr::Lit(IRLiteral::Int(1), Type::int())),
            else_branch: Box::new(IRExpr::Lit(IRLiteral::Int(2), Type::int())),
            ty: Type::int(),
        };

        assert_eq!(vm.eval_expr(&expr, &env).unwrap(), Value::Int(2));
    }

    #[test]
    fn eval_lambda_and_app() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let param = VarId::fresh();

        // (\x -> x)(42)
        let lambda = IRExpr::Lambda {
            param,
            param_ty: Type::int(),
            body: Box::new(IRExpr::Var(param, Type::int())),
            result_ty: Type::int(),
        };

        let expr = IRExpr::App {
            func: Box::new(lambda),
            arg: Box::new(IRExpr::Lit(IRLiteral::Int(42), Type::int())),
            result_ty: Type::int(),
        };

        assert_eq!(vm.eval_expr(&expr, &env).unwrap(), Value::Int(42));
    }

    #[test]
    fn eval_closure_capture() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let x = VarId::fresh();
        let y = VarId::fresh();

        // let x = 10 in (\y -> x)(42)
        let expr = IRExpr::Let {
            var: x,
            ty: Type::int(),
            value: Box::new(IRExpr::Lit(IRLiteral::Int(10), Type::int())),
            body: Box::new(IRExpr::App {
                func: Box::new(IRExpr::Lambda {
                    param: y,
                    param_ty: Type::int(),
                    body: Box::new(IRExpr::Var(x, Type::int())),
                    result_ty: Type::int(),
                }),
                arg: Box::new(IRExpr::Lit(IRLiteral::Int(42), Type::int())),
                result_ty: Type::int(),
            }),
        };

        assert_eq!(vm.eval_expr(&expr, &env).unwrap(), Value::Int(10));
    }

    #[test]
    fn eval_tuple() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let ty = Type::Tuple(vec![Type::int(), Type::int(), Type::int()]);

        let expr = IRExpr::Tuple {
            elems: vec![
                IRExpr::Lit(IRLiteral::Int(1), Type::int()),
                IRExpr::Lit(IRLiteral::Int(2), Type::int()),
                IRExpr::Lit(IRLiteral::Int(3), Type::int()),
            ],
            ty,
        };

        let result = vm.eval_expr(&expr, &env).unwrap();
        if let Value::Tuple(elems) = result {
            assert_eq!(elems.len(), 3);
            assert_eq!(elems[0], Value::Int(1));
            assert_eq!(elems[1], Value::Int(2));
            assert_eq!(elems[2], Value::Int(3));
        } else {
            panic!("expected tuple");
        }
    }

    #[test]
    fn eval_tuple_proj() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let tuple_ty = Type::Tuple(vec![Type::int(), Type::int()]);

        let expr = IRExpr::TupleProj {
            tuple: Box::new(IRExpr::Tuple {
                elems: vec![
                    IRExpr::Lit(IRLiteral::Int(1), Type::int()),
                    IRExpr::Lit(IRLiteral::Int(2), Type::int()),
                ],
                ty: tuple_ty,
            }),
            index: 1,
            ty: Type::int(),
        };

        assert_eq!(vm.eval_expr(&expr, &env).unwrap(), Value::Int(2));
    }

    #[test]
    fn eval_tuple_proj_out_of_bounds() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let tuple_ty = Type::Tuple(vec![Type::int()]);

        let expr = IRExpr::TupleProj {
            tuple: Box::new(IRExpr::Tuple {
                elems: vec![IRExpr::Lit(IRLiteral::Int(1), Type::int())],
                ty: tuple_ty,
            }),
            index: 5,
            ty: Type::int(),
        };

        let result = vm.eval_expr(&expr, &env);
        assert!(matches!(
            result,
            Err(RuntimeError::IndexOutOfBounds { index: 5, len: 1 })
        ));
    }

    #[test]
    fn eval_record() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let record_ty = Type::Record(vec![
            ("x".to_string(), Type::int()),
            ("y".to_string(), Type::int()),
        ]);

        let expr = IRExpr::Record {
            fields: vec![
                ("x".to_string(), IRExpr::Lit(IRLiteral::Int(10), Type::int())),
                ("y".to_string(), IRExpr::Lit(IRLiteral::Int(20), Type::int())),
            ],
            ty: record_ty,
        };

        let result = vm.eval_expr(&expr, &env).unwrap();
        if let Value::Record(fields) = result {
            assert_eq!(fields.get("x"), Some(&Value::Int(10)));
            assert_eq!(fields.get("y"), Some(&Value::Int(20)));
        } else {
            panic!("expected record");
        }
    }

    #[test]
    fn eval_field() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let record_ty = Type::Record(vec![
            ("x".to_string(), Type::int()),
            ("y".to_string(), Type::int()),
        ]);

        let expr = IRExpr::Field {
            record: Box::new(IRExpr::Record {
                fields: vec![
                    ("x".to_string(), IRExpr::Lit(IRLiteral::Int(10), Type::int())),
                    ("y".to_string(), IRExpr::Lit(IRLiteral::Int(20), Type::int())),
                ],
                ty: record_ty,
            }),
            field: "y".to_string(),
            ty: Type::int(),
        };

        assert_eq!(vm.eval_expr(&expr, &env).unwrap(), Value::Int(20));
    }

    #[test]
    fn eval_field_unknown() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let record_ty = Type::Record(vec![("x".to_string(), Type::int())]);

        let expr = IRExpr::Field {
            record: Box::new(IRExpr::Record {
                fields: vec![("x".to_string(), IRExpr::Lit(IRLiteral::Int(10), Type::int()))],
                ty: record_ty,
            }),
            field: "z".to_string(),
            ty: Type::int(),
        };

        let result = vm.eval_expr(&expr, &env);
        assert!(matches!(
            result,
            Err(RuntimeError::UnknownField { field }) if field == "z"
        ));
    }

    #[test]
    fn eval_construct_no_payload() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let ty = Type::option(Type::int());

        let expr = IRExpr::Construct {
            ctor: "None".to_string(),
            arg: None,
            ty,
        };

        let result = vm.eval_expr(&expr, &env).unwrap();
        assert!(matches!(
            result,
            Value::Constructor { tag, payload: None } if tag == "None"
        ));
    }

    #[test]
    fn eval_construct_with_payload() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());
        let ty = Type::option(Type::int());

        let expr = IRExpr::Construct {
            ctor: "Some".to_string(),
            arg: Some(Box::new(IRExpr::Lit(IRLiteral::Int(42), Type::int()))),
            ty,
        };

        let result = vm.eval_expr(&expr, &env).unwrap();
        if let Value::Constructor { tag, payload } = result {
            assert_eq!(tag, "Some");
            assert_eq!(payload, Some(Box::new(Value::Int(42))));
        } else {
            panic!("expected constructor");
        }
    }

    #[test]
    fn eval_primitive_add() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());

        let expr = IRExpr::Prim {
            op: Primitive::AddInt,
            args: vec![
                IRExpr::Lit(IRLiteral::Int(3), Type::int()),
                IRExpr::Lit(IRLiteral::Int(4), Type::int()),
            ],
            ty: Type::int(),
        };

        assert_eq!(vm.eval_expr(&expr, &env).unwrap(), Value::Int(7));
    }

    #[test]
    fn eval_stack_overflow() {
        let mut vm = test_vm();
        vm.set_max_depth(3);
        let env = Rc::new(Env::new());

        // Create a chain of nested function applications
        // (((\x -> (\y -> (\z -> z) y) x) 1) 2) 3)
        // Each application increments depth
        let z = VarId::fresh();
        let y = VarId::fresh();
        let x = VarId::fresh();

        let innermost = IRExpr::Lambda {
            param: z,
            param_ty: Type::int(),
            body: Box::new(IRExpr::Var(z, Type::int())),
            result_ty: Type::int(),
        };

        let middle = IRExpr::Lambda {
            param: y,
            param_ty: Type::int(),
            body: Box::new(IRExpr::App {
                func: Box::new(innermost),
                arg: Box::new(IRExpr::Var(y, Type::int())),
                result_ty: Type::int(),
            }),
            result_ty: Type::int(),
        };

        let outer = IRExpr::Lambda {
            param: x,
            param_ty: Type::int(),
            body: Box::new(IRExpr::App {
                func: Box::new(middle),
                arg: Box::new(IRExpr::Var(x, Type::int())),
                result_ty: Type::int(),
            }),
            result_ty: Type::int(),
        };

        // Apply the outer function - this triggers the nested calls
        let expr = IRExpr::App {
            func: Box::new(outer),
            arg: Box::new(IRExpr::Lit(IRLiteral::Int(42), Type::int())),
            result_ty: Type::int(),
        };

        let result = vm.eval_expr(&expr, &env);
        assert!(matches!(result, Err(RuntimeError::StackOverflow)));
    }

    // ========== Command Execution Tests ==========

    #[test]
    fn eval_command_echo() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());

        // Simple echo command
        let expr = IRExpr::Command {
            parts: vec![IRCommandPart::Literal("echo hello".to_string())],
            stdin: None,
            ty: Type::command_result(),
        };

        let result = vm.eval_expr(&expr, &env).unwrap();

        // Result should be a record with exitCode, stdout, stderr
        if let Value::Record(fields) = result {
            assert!(fields.contains_key("exitCode"));
            assert!(fields.contains_key("stdout"));
            assert!(fields.contains_key("stderr"));

            // Check exit code is 0
            assert_eq!(fields.get("exitCode"), Some(&Value::Int(0)));

            // Check stdout contains "hello"
            if let Some(Value::String(stdout)) = fields.get("stdout") {
                assert!(stdout.contains("hello"), "stdout was: {}", stdout);
            } else {
                panic!("stdout should be a string");
            }
        } else {
            panic!("Expected Record, got {:?}", result);
        }
    }

    #[test]
    fn eval_command_with_interpolation() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());

        // Command with interpolation: echo {value}
        let expr = IRExpr::Command {
            parts: vec![
                IRCommandPart::Literal("echo ".to_string()),
                IRCommandPart::Interpolation(Box::new(IRExpr::Lit(
                    IRLiteral::String("world".to_string()),
                    Type::string(),
                ))),
            ],
            stdin: None,
            ty: Type::command_result(),
        };

        let result = vm.eval_expr(&expr, &env).unwrap();

        if let Value::Record(fields) = result {
            assert_eq!(fields.get("exitCode"), Some(&Value::Int(0)));
            if let Some(Value::String(stdout)) = fields.get("stdout") {
                assert!(stdout.contains("world"), "stdout was: {}", stdout);
            }
        } else {
            panic!("Expected Record");
        }
    }

    #[test]
    fn eval_command_not_found() {
        let mut vm = test_vm();
        let env = Rc::new(Env::new());

        // Try to run a command that doesn't exist
        let expr = IRExpr::Command {
            parts: vec![IRCommandPart::Literal("this_command_does_not_exist_12345".to_string())],
            stdin: None,
            ty: Type::command_result(),
        };

        let result = vm.eval_expr(&expr, &env);
        assert!(result.is_err());
    }
}
