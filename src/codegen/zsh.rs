//! Zsh code generator for Neap
//!
//! This module transpiles Neap IR to zsh shell scripts.
//! All values are represented as JSON for uniform handling.

use std::collections::HashSet;

use crate::ir::{IRCommandPart, IRDecl, IRExpr, IRLiteral, IRPattern, IRProgram, Primitive, VarId};

use super::runtime::RUNTIME;

/// Code generator for zsh output.
pub struct ZshCodegen {
    /// The generated code
    output: String,
    /// Current indentation level
    indent: usize,
    /// Counter for generating unique temporary variables
    temp_counter: usize,
    /// Set of function names (to distinguish from value globals)
    function_names: HashSet<String>,
}

impl Default for ZshCodegen {
    fn default() -> Self {
        Self::new()
    }
}

impl ZshCodegen {
    /// Create a new code generator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
            temp_counter: 0,
            function_names: HashSet::new(),
        }
    }

    /// Generate zsh code from an IR program.
    #[must_use]
    pub fn generate(mut self, program: &IRProgram) -> String {
        // First pass: collect function names
        for decl in &program.decls {
            if let IRDecl::Fun { name, .. } = decl {
                self.function_names.insert(name.clone());
            }
        }

        // Second pass: generate code
        self.emit_shebang();
        self.emit_runtime();
        self.emit_program(program);
        self.emit_main_call();
        self.output
    }

    /// Generate a fresh temporary variable name.
    fn fresh_temp(&mut self) -> String {
        let name = format!("__t{}", self.temp_counter);
        self.temp_counter += 1;
        name
    }

    /// Emit a line of code with current indentation.
    fn emit(&mut self, line: &str) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        self.output.push_str(line);
        self.output.push('\n');
    }

    /// Emit an empty line.
    fn emit_blank(&mut self) {
        self.output.push('\n');
    }

    /// Emit the shebang and shell options.
    fn emit_shebang(&mut self) {
        self.emit("#!/usr/bin/env zsh");
        self.emit("set -uo pipefail");
        self.emit_blank();
    }

    /// Emit the runtime library.
    fn emit_runtime(&mut self) {
        self.output.push_str(RUNTIME);
        self.emit_blank();
    }

    /// Emit the main call at the end.
    fn emit_main_call(&mut self) {
        self.emit_blank();
        self.emit("__main \"$@\"");
    }

    /// Emit the complete program.
    fn emit_program(&mut self, program: &IRProgram) {
        self.emit("# ══════════════════════════════════════════════════════════════════════════════");
        self.emit("# Compiled Program");
        self.emit("# ══════════════════════════════════════════════════════════════════════════════");
        self.emit_blank();

        // First pass: emit all function declarations
        for decl in &program.decls {
            if let IRDecl::Fun { .. } = decl {
                self.emit_decl(decl);
                self.emit_blank();
            }
        }

        // Collect all value declarations for __main
        let val_decls: Vec<_> = program
            .decls
            .iter()
            .filter(|d| matches!(d, IRDecl::Val { .. }))
            .collect();

        // Emit __main function with all value declarations
        self.emit("__main() {");
        self.indent += 1;

        for decl in val_decls {
            self.emit_decl(decl);
        }

        self.indent -= 1;
        self.emit("}");
    }

    /// Emit a declaration.
    fn emit_decl(&mut self, decl: &IRDecl) {
        match decl {
            IRDecl::Val { name, value, .. } => {
                let val_code = self.emit_expr(value);
                self.emit(&format!("local {}={}", name, val_code));
            }
            IRDecl::Fun {
                name, params, body, ..
            } => {
                self.emit_fun_decl(name, params, body);
            }
            IRDecl::Data { .. } => {
                // Data type declarations don't generate code
                // Constructors are handled inline
            }
        }
    }

    /// Emit a function declaration.
    fn emit_fun_decl(&mut self, name: &str, params: &[(VarId, crate::types::Type)], body: &IRExpr) {
        self.emit(&format!("__{}() {{", name));
        self.indent += 1;

        // Bind parameters to positional arguments
        for (i, (param, _)) in params.iter().enumerate() {
            self.emit(&format!("local _{}=${}", param.raw(), i + 1));
        }

        // Generate body
        let result = self.emit_expr(body);
        self.emit(&format!("echo {}", result));

        self.indent -= 1;
        self.emit("}");
    }

    /// Emit an expression and return the zsh expression that evaluates to its value.
    fn emit_expr(&mut self, expr: &IRExpr) -> String {
        match expr {
            IRExpr::Lit(lit, _) => self.emit_literal(lit),

            IRExpr::Var(id, _) => format!("\"$_{}\"", id.raw()),

            IRExpr::Global(name, _) => {
                if self.function_names.contains(name) {
                    // Function reference - emit the runtime function name as a string
                    format!("\"__{}\"", name)
                } else {
                    // Value reference - emit as shell variable
                    format!("\"${}\"", name)
                }
            }

            IRExpr::Unit => "null".to_string(),

            IRExpr::Let { var, value, body, .. } => {
                let val_code = self.emit_expr(value);
                self.emit(&format!("local _{}={}", var.raw(), val_code));
                self.emit_expr(body)
            }

            IRExpr::LetRec { bindings, body, .. } => {
                // For recursive bindings, we emit them as local variables
                // This works because zsh functions can reference variables defined later
                for binding in bindings {
                    let val_code = self.emit_expr(&binding.value);
                    self.emit(&format!("local _{}={}", binding.var.raw(), val_code));
                }
                self.emit_expr(body)
            }

            IRExpr::Lambda {
                param, body, ..
            } => {
                // For lambdas, we create an anonymous function and return a closure
                // This is complex - for now, we'll handle this by creating a named function
                let fn_name = format!("__lambda_{}", self.temp_counter);
                self.temp_counter += 1;

                // We need to capture the current scope - this is tricky in zsh
                // For now, emit a simple function
                self.emit(&format!("{}() {{", fn_name));
                self.indent += 1;
                self.emit(&format!("local _{}=$1", param.raw()));
                let result = self.emit_expr(body);
                self.emit(&format!("echo {}", result));
                self.indent -= 1;
                self.emit("}");

                // Return the function name as a string
                format!("\"{}\"", fn_name)
            }

            IRExpr::App { func, arg, .. } => {
                let func_code = self.emit_expr(func);
                let arg_code = self.emit_expr(arg);
                format!("$(__neap_call {} {})", func_code, arg_code)
            }

            IRExpr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                let result_var = self.fresh_temp();
                self.emit(&format!("local {}", result_var));

                let cond_code = self.emit_expr(cond);
                self.emit(&format!("if [[ {} == \"true\" ]]; then", cond_code));
                self.indent += 1;
                let then_code = self.emit_expr(then_branch);
                self.emit(&format!("{}={}", result_var, then_code));
                self.indent -= 1;
                self.emit("else");
                self.indent += 1;
                let else_code = self.emit_expr(else_branch);
                self.emit(&format!("{}={}", result_var, else_code));
                self.indent -= 1;
                self.emit("fi");

                format!("\"${}\"", result_var)
            }

            IRExpr::Match { scrutinee, arms, .. } => {
                let result_var = self.fresh_temp();
                self.emit(&format!("local {}", result_var));

                let scrutinee_code = self.emit_expr(scrutinee);
                let scrutinee_var = self.fresh_temp();
                self.emit(&format!("local {}={}", scrutinee_var, scrutinee_code));

                // Emit pattern matching as case statement for ADTs or if-else chain
                self.emit_match(&result_var, &scrutinee_var, arms);

                format!("\"${}\"", result_var)
            }

            IRExpr::Prim { op, args, .. } => {
                let arg_codes: Vec<_> = args.iter().map(|a| self.emit_expr(a)).collect();
                self.emit_primitive(op, &arg_codes)
            }

            IRExpr::Tuple { elems, .. } => {
                let elem_codes: Vec<_> = elems.iter().map(|e| self.emit_expr(e)).collect();
                format!(
                    "$(jq -nc '[{}]')",
                    elem_codes
                        .iter()
                        .map(|e| format!("--argjson a{0} {0}", e))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            }

            IRExpr::TupleProj { tuple, index, .. } => {
                let tuple_code = self.emit_expr(tuple);
                format!("$(__neap_tuple_proj {} {})", tuple_code, index)
            }

            IRExpr::Record { fields, .. } => {
                // Build a JSON object
                if fields.is_empty() {
                    "'{}'".to_string()
                } else {
                    let mut jq_args = Vec::new();
                    let mut obj_parts = Vec::new();
                    for (i, (name, expr)) in fields.iter().enumerate() {
                        let val_code = self.emit_expr(expr);
                        jq_args.push(format!("--argjson f{} {}", i, val_code));
                        obj_parts.push(format!("\"{}\": $f{}", name, i));
                    }
                    format!(
                        "$(jq -nc {} '{{ {} }}')",
                        jq_args.join(" "),
                        obj_parts.join(", ")
                    )
                }
            }

            IRExpr::Field { record, field, .. } => {
                let record_code = self.emit_expr(record);
                format!("$(__neap_field {} \"{}\")", record_code, field)
            }

            IRExpr::Construct { ctor, arg, .. } => {
                if let Some(payload) = arg {
                    let payload_code = self.emit_expr(payload);
                    format!("$(__neap_construct1 \"{}\" {})", ctor, payload_code)
                } else {
                    format!("$(__neap_construct0 \"{}\")", ctor)
                }
            }

            IRExpr::Command { parts, stdin, .. } => {
                let cmd_str = self.emit_command_parts(parts);
                if let Some(stdin_expr) = stdin {
                    let stdin_code = self.emit_expr(stdin_expr);
                    // Pipe stdin to command
                    format!(
                        "$(__neap_run_cmd \"echo {} | {}\")",
                        stdin_code, cmd_str
                    )
                } else {
                    format!("$(__neap_run_cmd \"{}\")", cmd_str)
                }
            }
        }
    }

    /// Emit a literal value.
    fn emit_literal(&self, lit: &IRLiteral) -> String {
        match lit {
            IRLiteral::Int(n) => n.to_string(),
            IRLiteral::Float(f) => f.to_string(),
            IRLiteral::String(s) => format!("$(jq -n --arg s {} '$s')", shell_escape(s)),
            IRLiteral::Char(c) => format!("\"{}\"", c),
            IRLiteral::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        }
    }

    /// Emit command parts as a shell command string.
    fn emit_command_parts(&mut self, parts: &[IRCommandPart]) -> String {
        let mut result = String::new();
        for part in parts {
            match part {
                IRCommandPart::Literal(s) => result.push_str(s),
                IRCommandPart::Interpolation(expr) => {
                    let expr_code = self.emit_expr(expr);
                    // Unwrap JSON string if needed
                    result.push_str(&format!("$({} | jq -r '.')", expr_code));
                }
            }
        }
        result
    }

    /// Emit pattern matching.
    fn emit_match(&mut self, result_var: &str, scrutinee_var: &str, arms: &[(IRPattern, IRExpr)]) {
        // Check if this is an ADT match (all patterns are Con) or other
        let is_adt_match = arms.iter().all(|(p, _)| matches!(p, IRPattern::Con { .. }));

        if is_adt_match {
            self.emit(&format!(
                "case $(__neap_tag \"${}\") in",
                scrutinee_var
            ));
            self.indent += 1;

            for (pattern, body) in arms {
                if let IRPattern::Con { ctor, arg } = pattern {
                    self.emit(&format!("{})", ctor));
                    self.indent += 1;

                    // Extract payload if present
                    if let Some(arg_pattern) = arg {
                        self.emit_pattern_bindings(&format!("$(__neap_get \"${}\" '._0')", scrutinee_var), arg_pattern);
                    }

                    let body_code = self.emit_expr(body);
                    self.emit(&format!("{}={}", result_var, body_code));
                    self.emit(";;");
                    self.indent -= 1;
                }
            }

            self.indent -= 1;
            self.emit("esac");
        } else {
            // General pattern matching with if-else chain
            let mut first = true;
            for (pattern, body) in arms {
                let cond = self.emit_pattern_condition(&format!("\"${}\"", scrutinee_var), pattern);

                if first {
                    self.emit(&format!("if {}; then", cond));
                    first = false;
                } else {
                    self.emit(&format!("elif {}; then", cond));
                }

                self.indent += 1;
                self.emit_pattern_bindings(&format!("\"${}\"", scrutinee_var), pattern);
                let body_code = self.emit_expr(body);
                self.emit(&format!("{}={}", result_var, body_code));
                self.indent -= 1;
            }
            self.emit("fi");
        }
    }

    /// Emit pattern bindings (extracting matched values).
    fn emit_pattern_bindings(&mut self, scrutinee: &str, pattern: &IRPattern) {
        match pattern {
            IRPattern::Var(id, _) => {
                self.emit(&format!("local _{}={}", id.raw(), scrutinee));
            }
            IRPattern::Tuple(pats) => {
                for (i, p) in pats.iter().enumerate() {
                    let elem = format!("$(__neap_tuple_proj {} {})", scrutinee, i);
                    self.emit_pattern_bindings(&elem, p);
                }
            }
            IRPattern::Record(fields) => {
                for (name, p) in fields {
                    let field = format!("$(__neap_field {} \"{}\")", scrutinee, name);
                    self.emit_pattern_bindings(&field, p);
                }
            }
            IRPattern::Con { arg: Some(arg), .. } => {
                let payload = format!("$(__neap_get {} '._0')", scrutinee);
                self.emit_pattern_bindings(&payload, arg);
            }
            IRPattern::Wildcard | IRPattern::Lit(_) | IRPattern::Con { arg: None, .. } => {
                // No bindings to emit
            }
        }
    }

    /// Emit a condition for pattern matching.
    fn emit_pattern_condition(&self, scrutinee: &str, pattern: &IRPattern) -> String {
        match pattern {
            IRPattern::Wildcard => "true".to_string(),
            IRPattern::Var(_, _) => "true".to_string(),
            IRPattern::Lit(lit) => {
                let lit_code = self.emit_literal(lit);
                format!("[[ {} == {} ]]", scrutinee, lit_code)
            }
            IRPattern::Con { ctor, .. } => {
                format!("[[ $(__neap_tag {}) == \"{}\" ]]", scrutinee, ctor)
            }
            IRPattern::Tuple(_) => {
                // Tuples always match in structure (type system guarantees)
                "true".to_string()
            }
            IRPattern::Record(_) => {
                // Records always match in structure (type system guarantees)
                "true".to_string()
            }
        }
    }

    /// Emit a primitive operation.
    fn emit_primitive(&self, prim: &Primitive, args: &[String]) -> String {
        match prim {
            // Integer arithmetic
            Primitive::AddInt => format!("$(__neap_add_int {} {})", args[0], args[1]),
            Primitive::SubInt => format!("$(__neap_sub_int {} {})", args[0], args[1]),
            Primitive::MulInt => format!("$(__neap_mul_int {} {})", args[0], args[1]),
            Primitive::DivInt => format!("$(__neap_div_int {} {})", args[0], args[1]),
            Primitive::ModInt => format!("$(__neap_mod_int {} {})", args[0], args[1]),
            Primitive::NegInt => format!("$(__neap_neg_int {})", args[0]),

            // Float arithmetic
            Primitive::AddFloat => format!("$(__neap_add_float {} {})", args[0], args[1]),
            Primitive::SubFloat => format!("$(__neap_sub_float {} {})", args[0], args[1]),
            Primitive::MulFloat => format!("$(__neap_mul_float {} {})", args[0], args[1]),
            Primitive::DivFloat => format!("$(__neap_div_float {} {})", args[0], args[1]),
            Primitive::NegFloat => format!("$(__neap_neg_float {})", args[0]),

            // Integer comparison
            Primitive::EqInt => format!("$(__neap_eq_int {} {})", args[0], args[1]),
            Primitive::NeqInt => format!("$(__neap_neq_int {} {})", args[0], args[1]),
            Primitive::LtInt => format!("$(__neap_lt_int {} {})", args[0], args[1]),
            Primitive::LeInt => format!("$(__neap_le_int {} {})", args[0], args[1]),
            Primitive::GtInt => format!("$(__neap_gt_int {} {})", args[0], args[1]),
            Primitive::GeInt => format!("$(__neap_ge_int {} {})", args[0], args[1]),

            // Float comparison
            Primitive::EqFloat => format!("$(__neap_eq_float {} {})", args[0], args[1]),
            Primitive::NeqFloat => format!("$(__neap_neq_float {} {})", args[0], args[1]),
            Primitive::LtFloat => format!("$(__neap_lt_float {} {})", args[0], args[1]),
            Primitive::LeFloat => format!("$(__neap_le_float {} {})", args[0], args[1]),
            Primitive::GtFloat => format!("$(__neap_gt_float {} {})", args[0], args[1]),
            Primitive::GeFloat => format!("$(__neap_ge_float {} {})", args[0], args[1]),

            // String comparison
            Primitive::EqString => format!("$(__neap_eq_string {} {})", args[0], args[1]),
            Primitive::NeqString => format!("$(__neap_neq_string {} {})", args[0], args[1]),
            Primitive::LtString => format!("$(__neap_lt_string {} {})", args[0], args[1]),
            Primitive::LeString => format!("$(__neap_le_string {} {})", args[0], args[1]),
            Primitive::GtString => format!("$(__neap_gt_string {} {})", args[0], args[1]),
            Primitive::GeString => format!("$(__neap_ge_string {} {})", args[0], args[1]),

            // Bool comparison
            Primitive::EqBool => format!("$(__neap_eq_bool {} {})", args[0], args[1]),
            Primitive::NeqBool => format!("$(__neap_neq_bool {} {})", args[0], args[1]),

            // Char comparison
            Primitive::EqChar => format!("$(__neap_eq_char {} {})", args[0], args[1]),
            Primitive::NeqChar => format!("$(__neap_neq_char {} {})", args[0], args[1]),
            Primitive::LtChar => format!("$(__neap_lt_char {} {})", args[0], args[1]),
            Primitive::LeChar => format!("$(__neap_le_char {} {})", args[0], args[1]),
            Primitive::GtChar => format!("$(__neap_gt_char {} {})", args[0], args[1]),
            Primitive::GeChar => format!("$(__neap_ge_char {} {})", args[0], args[1]),

            // Logical
            Primitive::Not => format!("$(__neap_not {})", args[0]),
            Primitive::And => format!("$(__neap_and {} {})", args[0], args[1]),
            Primitive::Or => format!("$(__neap_or {} {})", args[0], args[1]),

            // String operations
            Primitive::Concat => format!("$(__neap_concat {} {})", args[0], args[1]),
            Primitive::StringLength => format!("$(__neap_string_length {})", args[0]),
            Primitive::Substring => {
                format!("$(__neap_substring {} {} {})", args[0], args[1], args[2])
            }
            Primitive::CharAt => format!("$(__neap_char_at {} {})", args[0], args[1]),

            // Conversions
            Primitive::IntToFloat => format!("$(__neap_int_to_float {})", args[0]),
            Primitive::FloatToInt => format!("$(__neap_float_to_int {})", args[0]),
            Primitive::IntToString => format!("$(__neap_int_to_string {})", args[0]),
            Primitive::FloatToString => format!("$(__neap_float_to_string {})", args[0]),
            Primitive::CharToString => format!("$(__neap_char_to_string {})", args[0]),
            Primitive::CharToInt => format!("$(__neap_char_to_int {})", args[0]),
            Primitive::IntToChar => format!("$(__neap_int_to_char {})", args[0]),
            Primitive::BoolToString => format!("$(__neap_bool_to_string {})", args[0]),
            Primitive::StringIdentity => format!("$(__neap_string_identity {})", args[0]),

            // List operations
            Primitive::Cons => format!("$(__neap_cons {} {})", args[0], args[1]),
            Primitive::Append => format!("$(__neap_append {} {})", args[0], args[1]),
            Primitive::ListLength => format!("$(__neap_list_length {})", args[0]),

            // I/O
            Primitive::Print => {
                // Print has a side effect, so we emit it as a statement
                format!("$(__neap_print {})", args[0])
            }
            Primitive::PrintNoNewline => format!("$(__neap_print_no_newline {})", args[0]),
            Primitive::ReadLine => "$(__neap_read_line)".to_string(),
            Primitive::ReadFile => format!("$(__neap_read_file {})", args[0]),
            Primitive::WriteFile => format!("$(__neap_write_file {} {})", args[0], args[1]),
            Primitive::GetEnv => format!("$(__neap_get_env {})", args[0]),

            // Assertions
            Primitive::Assert => format!("$(__neap_assert {})", args[0]),
            Primitive::Panic => format!("$(__neap_panic {})", args[0]),
        }
    }
}

/// Escape a string for shell use.
fn shell_escape(s: &str) -> String {
    // Use single quotes and escape embedded single quotes
    let escaped = s.replace('\'', "'\"'\"'");
    format!("'{}'", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::IRLiteral;
    use crate::types::Type;

    #[test]
    fn codegen_literal_int() {
        let codegen = ZshCodegen::new();
        let lit = IRLiteral::Int(42);
        assert_eq!(codegen.emit_literal(&lit), "42");
    }

    #[test]
    fn codegen_literal_bool() {
        let codegen = ZshCodegen::new();
        assert_eq!(codegen.emit_literal(&IRLiteral::Bool(true)), "true");
        assert_eq!(codegen.emit_literal(&IRLiteral::Bool(false)), "false");
    }

    #[test]
    fn codegen_literal_string() {
        let codegen = ZshCodegen::new();
        let lit = IRLiteral::String("hello".to_string());
        assert_eq!(
            codegen.emit_literal(&lit),
            "$(jq -n --arg s 'hello' '$s')"
        );
    }

    #[test]
    fn codegen_empty_program() {
        let program = IRProgram { decls: vec![] };
        let output = ZshCodegen::new().generate(&program);
        assert!(output.contains("#!/usr/bin/env zsh"));
        assert!(output.contains("__main()"));
    }

    #[test]
    fn codegen_simple_val() {
        let program = IRProgram {
            decls: vec![IRDecl::Val {
                name: "x".to_string(),
                ty: Type::int(),
                value: IRExpr::Lit(IRLiteral::Int(42), Type::int()),
            }],
        };
        let output = ZshCodegen::new().generate(&program);
        assert!(output.contains("local x=42"));
    }

    #[test]
    fn codegen_simple_fun() {
        let program = IRProgram {
            decls: vec![IRDecl::Fun {
                name: "double".to_string(),
                ty: Type::arrow(Type::int(), Type::int()),
                params: vec![(VarId::from_raw(0), Type::int())],
                body: IRExpr::Prim {
                    op: Primitive::MulInt,
                    args: vec![
                        IRExpr::Var(VarId::from_raw(0), Type::int()),
                        IRExpr::Lit(IRLiteral::Int(2), Type::int()),
                    ],
                    ty: Type::int(),
                },
            }],
        };
        let output = ZshCodegen::new().generate(&program);
        assert!(output.contains("__double()"));
        assert!(output.contains("local _0=$1"));
        assert!(output.contains("__neap_mul_int"));
    }
}
