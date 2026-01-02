//! Primitive operations for the Neap VM
//!
//! Implements all built-in operations like arithmetic, comparisons,
//! string operations, and IO.

use std::io::{BufRead, Write};

use crate::ir::Primitive;

use super::error::RuntimeError;
use super::eval::VM;
use super::value::Value;

/// Evaluate a primitive operation.
pub fn eval_primitive<W: Write, R: BufRead>(
    vm: &mut VM<W, R>,
    op: Primitive,
    args: &[Value],
) -> Result<Value, RuntimeError> {
    match op {
        // Integer arithmetic
        Primitive::AddInt => binary_int_op(args, |a, b| Ok(Value::Int(a.wrapping_add(b)))),
        Primitive::SubInt => binary_int_op(args, |a, b| Ok(Value::Int(a.wrapping_sub(b)))),
        Primitive::MulInt => binary_int_op(args, |a, b| Ok(Value::Int(a.wrapping_mul(b)))),
        Primitive::DivInt => binary_int_op(args, |a, b| {
            if b == 0 {
                Err(RuntimeError::DivisionByZero)
            } else {
                Ok(Value::Int(a / b))
            }
        }),
        Primitive::ModInt => binary_int_op(args, |a, b| {
            if b == 0 {
                Err(RuntimeError::DivisionByZero)
            } else {
                Ok(Value::Int(a % b))
            }
        }),
        Primitive::NegInt => unary_int_op(args, |a| Ok(Value::Int(-a))),

        // Float arithmetic
        Primitive::AddFloat => binary_float_op(args, |a, b| Ok(Value::Float(a + b))),
        Primitive::SubFloat => binary_float_op(args, |a, b| Ok(Value::Float(a - b))),
        Primitive::MulFloat => binary_float_op(args, |a, b| Ok(Value::Float(a * b))),
        Primitive::DivFloat => binary_float_op(args, |a, b| {
            if b == 0.0 {
                Err(RuntimeError::DivisionByZero)
            } else {
                Ok(Value::Float(a / b))
            }
        }),
        Primitive::NegFloat => unary_float_op(args, |a| Ok(Value::Float(-a))),

        // Integer comparisons
        Primitive::EqInt => binary_int_op(args, |a, b| Ok(Value::Bool(a == b))),
        Primitive::NeqInt => binary_int_op(args, |a, b| Ok(Value::Bool(a != b))),
        Primitive::LtInt => binary_int_op(args, |a, b| Ok(Value::Bool(a < b))),
        Primitive::LeInt => binary_int_op(args, |a, b| Ok(Value::Bool(a <= b))),
        Primitive::GtInt => binary_int_op(args, |a, b| Ok(Value::Bool(a > b))),
        Primitive::GeInt => binary_int_op(args, |a, b| Ok(Value::Bool(a >= b))),

        // Float comparisons
        Primitive::EqFloat => binary_float_op(args, |a, b| Ok(Value::Bool(a == b))),
        Primitive::NeqFloat => binary_float_op(args, |a, b| Ok(Value::Bool(a != b))),
        Primitive::LtFloat => binary_float_op(args, |a, b| Ok(Value::Bool(a < b))),
        Primitive::LeFloat => binary_float_op(args, |a, b| Ok(Value::Bool(a <= b))),
        Primitive::GtFloat => binary_float_op(args, |a, b| Ok(Value::Bool(a > b))),
        Primitive::GeFloat => binary_float_op(args, |a, b| Ok(Value::Bool(a >= b))),

        // String comparisons
        Primitive::EqString => binary_string_op(args, |a, b| Ok(Value::Bool(a == b))),
        Primitive::NeqString => binary_string_op(args, |a, b| Ok(Value::Bool(a != b))),
        Primitive::LtString => binary_string_op(args, |a, b| Ok(Value::Bool(a < b))),
        Primitive::LeString => binary_string_op(args, |a, b| Ok(Value::Bool(a <= b))),
        Primitive::GtString => binary_string_op(args, |a, b| Ok(Value::Bool(a > b))),
        Primitive::GeString => binary_string_op(args, |a, b| Ok(Value::Bool(a >= b))),

        // Char comparisons
        Primitive::EqChar => binary_char_op(args, |a, b| Ok(Value::Bool(a == b))),
        Primitive::NeqChar => binary_char_op(args, |a, b| Ok(Value::Bool(a != b))),
        Primitive::LtChar => binary_char_op(args, |a, b| Ok(Value::Bool(a < b))),
        Primitive::LeChar => binary_char_op(args, |a, b| Ok(Value::Bool(a <= b))),
        Primitive::GtChar => binary_char_op(args, |a, b| Ok(Value::Bool(a > b))),
        Primitive::GeChar => binary_char_op(args, |a, b| Ok(Value::Bool(a >= b))),

        // Bool comparisons
        Primitive::EqBool => binary_bool_op(args, |a, b| Ok(Value::Bool(a == b))),
        Primitive::NeqBool => binary_bool_op(args, |a, b| Ok(Value::Bool(a != b))),

        // Logical operations
        Primitive::Not => {
            expect_args(1, args)?;
            match &args[0] {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                v => Err(type_error("bool", v)),
            }
        }

        // Note: And/Or are typically short-circuit and handled at IR level as If expressions
        // These are non-short-circuit fallbacks
        Primitive::And => binary_bool_op(args, |a, b| Ok(Value::Bool(a && b))),
        Primitive::Or => binary_bool_op(args, |a, b| Ok(Value::Bool(a || b))),

        // String operations
        Primitive::Concat => binary_string_op(args, |a, b| {
            let mut result = a.to_string();
            result.push_str(b);
            Ok(Value::string(result))
        }),

        Primitive::StringLength => {
            expect_args(1, args)?;
            match &args[0] {
                Value::String(s) => Ok(Value::Int(s.len() as i64)),
                v => Err(type_error("string", v)),
            }
        }

        Primitive::Substring => {
            expect_args(3, args)?;
            let s = args[0]
                .as_string()
                .ok_or_else(|| type_error("string", &args[0]))?;
            let start = args[1]
                .as_int()
                .ok_or_else(|| type_error("int", &args[1]))?;
            let len = args[2]
                .as_int()
                .ok_or_else(|| type_error("int", &args[2]))?;

            if start < 0 || len < 0 {
                return Ok(Value::string(""));
            }

            let start = start as usize;
            let len = len as usize;

            let result: String = s.chars().skip(start).take(len).collect();
            Ok(Value::string(result))
        }

        Primitive::CharAt => {
            expect_args(2, args)?;
            let s = args[0]
                .as_string()
                .ok_or_else(|| type_error("string", &args[0]))?;
            let idx = args[1]
                .as_int()
                .ok_or_else(|| type_error("int", &args[1]))?;

            if idx < 0 {
                return Ok(Value::none());
            }

            match s.chars().nth(idx as usize) {
                Some(c) => Ok(Value::some(Value::Char(c))),
                None => Ok(Value::none()),
            }
        }

        // Conversions
        Primitive::IntToFloat => {
            expect_args(1, args)?;
            let n = args[0]
                .as_int()
                .ok_or_else(|| type_error("int", &args[0]))?;
            Ok(Value::Float(n as f64))
        }

        Primitive::FloatToInt => {
            expect_args(1, args)?;
            let f = args[0]
                .as_float()
                .ok_or_else(|| type_error("float", &args[0]))?;
            Ok(Value::Int(f as i64))
        }

        Primitive::IntToString => {
            expect_args(1, args)?;
            let n = args[0]
                .as_int()
                .ok_or_else(|| type_error("int", &args[0]))?;
            Ok(Value::string(n.to_string()))
        }

        Primitive::FloatToString => {
            expect_args(1, args)?;
            let f = args[0]
                .as_float()
                .ok_or_else(|| type_error("float", &args[0]))?;
            Ok(Value::string(f.to_string()))
        }

        Primitive::CharToString => {
            expect_args(1, args)?;
            let c = args[0]
                .as_char()
                .ok_or_else(|| type_error("char", &args[0]))?;
            Ok(Value::string(c.to_string()))
        }

        Primitive::CharToInt => {
            expect_args(1, args)?;
            let c = args[0]
                .as_char()
                .ok_or_else(|| type_error("char", &args[0]))?;
            Ok(Value::Int(c as i64))
        }

        Primitive::IntToChar => {
            expect_args(1, args)?;
            let n = args[0]
                .as_int()
                .ok_or_else(|| type_error("int", &args[0]))?;
            match char::from_u32(n as u32) {
                Some(c) => Ok(Value::some(Value::Char(c))),
                None => Ok(Value::none()),
            }
        }

        Primitive::BoolToString => {
            expect_args(1, args)?;
            let b = args[0]
                .as_bool()
                .ok_or_else(|| type_error("bool", &args[0]))?;
            Ok(Value::string(if b { "true" } else { "false" }))
        }

        Primitive::StringIdentity => {
            expect_args(1, args)?;
            // String identity - just return the string as-is
            match &args[0] {
                Value::String(_) => Ok(args[0].clone()),
                v => Err(type_error("string", v)),
            }
        }

        // List operations
        Primitive::Cons => {
            expect_args(2, args)?;
            Ok(Value::cons(args[0].clone(), args[1].clone()))
        }

        Primitive::Append => {
            expect_args(2, args)?;
            append_lists(&args[0], &args[1])
        }

        Primitive::ListLength => {
            expect_args(1, args)?;
            let len = list_length(&args[0])?;
            Ok(Value::Int(len))
        }

        // IO operations
        Primitive::Print => {
            expect_args(1, args)?;
            let s = args[0]
                .as_string()
                .ok_or_else(|| type_error("string", &args[0]))?;
            writeln!(vm.stdout(), "{s}")?;
            Ok(Value::Unit)
        }

        Primitive::PrintNoNewline => {
            expect_args(1, args)?;
            let s = args[0]
                .as_string()
                .ok_or_else(|| type_error("string", &args[0]))?;
            write!(vm.stdout(), "{s}")?;
            vm.stdout().flush()?;
            Ok(Value::Unit)
        }

        Primitive::ReadLine => {
            expect_args(0, args)?;
            let mut line = String::new();
            vm.stdin().read_line(&mut line)?;
            // Remove trailing newline
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Ok(Value::string(line))
        }

        Primitive::ReadFile => {
            expect_args(1, args)?;
            let path = args[0]
                .as_string()
                .ok_or_else(|| type_error("string", &args[0]))?;
            match std::fs::read_to_string(path) {
                Ok(contents) => Ok(Value::ok(Value::string(contents))),
                Err(e) => Ok(Value::err(Value::string(e.to_string()))),
            }
        }

        Primitive::WriteFile => {
            expect_args(2, args)?;
            let path = args[0]
                .as_string()
                .ok_or_else(|| type_error("string", &args[0]))?;
            let contents = args[1]
                .as_string()
                .ok_or_else(|| type_error("string", &args[1]))?;
            match std::fs::write(path, contents) {
                Ok(()) => Ok(Value::ok(Value::Unit)),
                Err(e) => Ok(Value::err(Value::string(e.to_string()))),
            }
        }

        Primitive::GetEnv => {
            expect_args(1, args)?;
            let name = args[0]
                .as_string()
                .ok_or_else(|| type_error("string", &args[0]))?;
            match std::env::var(name) {
                Ok(val) => Ok(Value::some(Value::string(val))),
                Err(_) => Ok(Value::none()),
            }
        }

        // Assertions
        Primitive::Assert => {
            expect_args(2, args)?;
            match &args[0] {
                Value::Bool(true) => Ok(Value::Unit),
                Value::Bool(false) => {
                    let msg = args[1]
                        .as_string()
                        .ok_or_else(|| type_error("string", &args[1]))?;
                    Err(RuntimeError::AssertionFailed {
                        message: msg.to_string(),
                    })
                }
                v => Err(type_error("bool", v)),
            }
        }

        Primitive::Panic => {
            expect_args(1, args)?;
            let msg = args[0]
                .as_string()
                .ok_or_else(|| type_error("string", &args[0]))?;
            Err(RuntimeError::UserError {
                message: msg.to_string(),
            })
        }
    }
}

/// Check that we have the expected number of arguments.
fn expect_args(expected: usize, args: &[Value]) -> Result<(), RuntimeError> {
    if args.len() != expected {
        Err(RuntimeError::TypeError {
            expected: format!("{expected} arguments"),
            got: format!("{} arguments", args.len()),
        })
    } else {
        Ok(())
    }
}

/// Create a type error.
fn type_error(expected: &str, got: &Value) -> RuntimeError {
    RuntimeError::TypeError {
        expected: expected.to_string(),
        got: got.type_name().to_string(),
    }
}

/// Apply a binary integer operation.
fn binary_int_op<F>(args: &[Value], f: F) -> Result<Value, RuntimeError>
where
    F: FnOnce(i64, i64) -> Result<Value, RuntimeError>,
{
    expect_args(2, args)?;
    let a = args[0].as_int().ok_or_else(|| type_error("int", &args[0]))?;
    let b = args[1].as_int().ok_or_else(|| type_error("int", &args[1]))?;
    f(a, b)
}

/// Apply a unary integer operation.
fn unary_int_op<F>(args: &[Value], f: F) -> Result<Value, RuntimeError>
where
    F: FnOnce(i64) -> Result<Value, RuntimeError>,
{
    expect_args(1, args)?;
    let a = args[0].as_int().ok_or_else(|| type_error("int", &args[0]))?;
    f(a)
}

/// Apply a binary float operation.
fn binary_float_op<F>(args: &[Value], f: F) -> Result<Value, RuntimeError>
where
    F: FnOnce(f64, f64) -> Result<Value, RuntimeError>,
{
    expect_args(2, args)?;
    let a = args[0]
        .as_float()
        .ok_or_else(|| type_error("float", &args[0]))?;
    let b = args[1]
        .as_float()
        .ok_or_else(|| type_error("float", &args[1]))?;
    f(a, b)
}

/// Apply a unary float operation.
fn unary_float_op<F>(args: &[Value], f: F) -> Result<Value, RuntimeError>
where
    F: FnOnce(f64) -> Result<Value, RuntimeError>,
{
    expect_args(1, args)?;
    let a = args[0]
        .as_float()
        .ok_or_else(|| type_error("float", &args[0]))?;
    f(a)
}

/// Apply a binary string operation.
fn binary_string_op<F>(args: &[Value], f: F) -> Result<Value, RuntimeError>
where
    F: FnOnce(&str, &str) -> Result<Value, RuntimeError>,
{
    expect_args(2, args)?;
    let a = args[0]
        .as_string()
        .ok_or_else(|| type_error("string", &args[0]))?;
    let b = args[1]
        .as_string()
        .ok_or_else(|| type_error("string", &args[1]))?;
    f(a, b)
}

/// Apply a binary char operation.
fn binary_char_op<F>(args: &[Value], f: F) -> Result<Value, RuntimeError>
where
    F: FnOnce(char, char) -> Result<Value, RuntimeError>,
{
    expect_args(2, args)?;
    let a = args[0]
        .as_char()
        .ok_or_else(|| type_error("char", &args[0]))?;
    let b = args[1]
        .as_char()
        .ok_or_else(|| type_error("char", &args[1]))?;
    f(a, b)
}

/// Apply a binary bool operation.
fn binary_bool_op<F>(args: &[Value], f: F) -> Result<Value, RuntimeError>
where
    F: FnOnce(bool, bool) -> Result<Value, RuntimeError>,
{
    expect_args(2, args)?;
    let a = args[0]
        .as_bool()
        .ok_or_else(|| type_error("bool", &args[0]))?;
    let b = args[1]
        .as_bool()
        .ok_or_else(|| type_error("bool", &args[1]))?;
    f(a, b)
}

/// Append two lists.
fn append_lists(left: &Value, right: &Value) -> Result<Value, RuntimeError> {
    match left {
        Value::Constructor {
            tag,
            payload: None,
        } if tag == "Nil" => Ok(right.clone()),
        Value::Constructor {
            tag,
            payload: Some(p),
        } if tag == "Cons" => {
            if let Value::Tuple(elems) = p.as_ref()
                && elems.len() == 2
            {
                let head = &elems[0];
                let tail = &elems[1];
                let new_tail = append_lists(tail, right)?;
                return Ok(Value::cons(head.clone(), new_tail));
            }
            Err(RuntimeError::TypeError {
                expected: "list".to_string(),
                got: "malformed cons".to_string(),
            })
        }
        _ => Err(type_error("list", left)),
    }
}

/// Get the length of a list.
fn list_length(list: &Value) -> Result<i64, RuntimeError> {
    let mut count = 0i64;
    let mut current = list;

    loop {
        match current {
            Value::Constructor {
                tag,
                payload: None,
            } if tag == "Nil" => return Ok(count),
            Value::Constructor {
                tag,
                payload: Some(p),
            } if tag == "Cons" => {
                if let Value::Tuple(elems) = p.as_ref()
                    && elems.len() == 2
                {
                    count += 1;
                    current = &elems[1];
                    continue;
                }
                return Err(RuntimeError::TypeError {
                    expected: "list".to_string(),
                    got: "malformed cons".to_string(),
                });
            }
            _ => {
                return Err(type_error("list", current));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn test_vm() -> VM<Vec<u8>, Cursor<Vec<u8>>> {
        VM::new(Vec::new(), Cursor::new(Vec::new()))
    }

    // Integer arithmetic tests
    #[test]
    fn prim_add_int() {
        let mut vm = test_vm();
        let result =
            eval_primitive(&mut vm, Primitive::AddInt, &[Value::Int(3), Value::Int(4)]).unwrap();
        assert_eq!(result, Value::Int(7));
    }

    #[test]
    fn prim_sub_int() {
        let mut vm = test_vm();
        let result =
            eval_primitive(&mut vm, Primitive::SubInt, &[Value::Int(10), Value::Int(4)]).unwrap();
        assert_eq!(result, Value::Int(6));
    }

    #[test]
    fn prim_mul_int() {
        let mut vm = test_vm();
        let result =
            eval_primitive(&mut vm, Primitive::MulInt, &[Value::Int(3), Value::Int(4)]).unwrap();
        assert_eq!(result, Value::Int(12));
    }

    #[test]
    fn prim_div_int() {
        let mut vm = test_vm();
        let result =
            eval_primitive(&mut vm, Primitive::DivInt, &[Value::Int(10), Value::Int(3)]).unwrap();
        assert_eq!(result, Value::Int(3));
    }

    #[test]
    fn prim_div_int_by_zero() {
        let mut vm = test_vm();
        let result = eval_primitive(&mut vm, Primitive::DivInt, &[Value::Int(10), Value::Int(0)]);
        assert!(matches!(result, Err(RuntimeError::DivisionByZero)));
    }

    #[test]
    fn prim_mod_int() {
        let mut vm = test_vm();
        let result =
            eval_primitive(&mut vm, Primitive::ModInt, &[Value::Int(10), Value::Int(3)]).unwrap();
        assert_eq!(result, Value::Int(1));
    }

    #[test]
    fn prim_neg_int() {
        let mut vm = test_vm();
        let result = eval_primitive(&mut vm, Primitive::NegInt, &[Value::Int(42)]).unwrap();
        assert_eq!(result, Value::Int(-42));
    }

    // Float arithmetic tests
    #[test]
    fn prim_add_float() {
        let mut vm = test_vm();
        let result = eval_primitive(
            &mut vm,
            Primitive::AddFloat,
            &[Value::Float(3.5), Value::Float(2.5)],
        )
        .unwrap();
        assert_eq!(result, Value::Float(6.0));
    }

    #[test]
    fn prim_div_float_by_zero() {
        let mut vm = test_vm();
        let result = eval_primitive(
            &mut vm,
            Primitive::DivFloat,
            &[Value::Float(10.0), Value::Float(0.0)],
        );
        assert!(matches!(result, Err(RuntimeError::DivisionByZero)));
    }

    // Integer comparison tests
    #[test]
    fn prim_eq_int() {
        let mut vm = test_vm();
        let result =
            eval_primitive(&mut vm, Primitive::EqInt, &[Value::Int(5), Value::Int(5)]).unwrap();
        assert_eq!(result, Value::Bool(true));

        let result =
            eval_primitive(&mut vm, Primitive::EqInt, &[Value::Int(5), Value::Int(6)]).unwrap();
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn prim_lt_int() {
        let mut vm = test_vm();
        let result =
            eval_primitive(&mut vm, Primitive::LtInt, &[Value::Int(3), Value::Int(5)]).unwrap();
        assert_eq!(result, Value::Bool(true));

        let result =
            eval_primitive(&mut vm, Primitive::LtInt, &[Value::Int(5), Value::Int(3)]).unwrap();
        assert_eq!(result, Value::Bool(false));
    }

    // Logical tests
    #[test]
    fn prim_not() {
        let mut vm = test_vm();
        let result = eval_primitive(&mut vm, Primitive::Not, &[Value::Bool(true)]).unwrap();
        assert_eq!(result, Value::Bool(false));

        let result = eval_primitive(&mut vm, Primitive::Not, &[Value::Bool(false)]).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    // String tests
    #[test]
    fn prim_concat() {
        let mut vm = test_vm();
        let result = eval_primitive(
            &mut vm,
            Primitive::Concat,
            &[Value::string("hello "), Value::string("world")],
        )
        .unwrap();
        assert_eq!(result.as_string(), Some("hello world"));
    }

    #[test]
    fn prim_string_length() {
        let mut vm = test_vm();
        let result =
            eval_primitive(&mut vm, Primitive::StringLength, &[Value::string("hello")]).unwrap();
        assert_eq!(result, Value::Int(5));
    }

    #[test]
    fn prim_substring() {
        let mut vm = test_vm();
        let result = eval_primitive(
            &mut vm,
            Primitive::Substring,
            &[Value::string("hello world"), Value::Int(6), Value::Int(5)],
        )
        .unwrap();
        assert_eq!(result.as_string(), Some("world"));
    }

    #[test]
    fn prim_char_at() {
        let mut vm = test_vm();
        let result = eval_primitive(
            &mut vm,
            Primitive::CharAt,
            &[Value::string("hello"), Value::Int(1)],
        )
        .unwrap();
        if let Value::Constructor { tag, payload } = result {
            assert_eq!(tag, "Some");
            assert!(payload.is_some());
            assert_eq!(*payload.unwrap(), Value::Char('e'));
        } else {
            panic!("expected Some");
        }
    }

    #[test]
    fn prim_char_at_out_of_bounds() {
        let mut vm = test_vm();
        let result = eval_primitive(
            &mut vm,
            Primitive::CharAt,
            &[Value::string("hi"), Value::Int(5)],
        )
        .unwrap();
        assert!(matches!(
            result,
            Value::Constructor { tag, payload: None } if tag == "None"
        ));
    }

    // Conversion tests
    #[test]
    fn prim_int_to_float() {
        let mut vm = test_vm();
        let result = eval_primitive(&mut vm, Primitive::IntToFloat, &[Value::Int(42)]).unwrap();
        assert_eq!(result, Value::Float(42.0));
    }

    #[test]
    fn prim_float_to_int() {
        let mut vm = test_vm();
        let result = eval_primitive(&mut vm, Primitive::FloatToInt, &[Value::Float(42.9)]).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn prim_int_to_string() {
        let mut vm = test_vm();
        let result = eval_primitive(&mut vm, Primitive::IntToString, &[Value::Int(42)]).unwrap();
        assert_eq!(result.as_string(), Some("42"));
    }

    // List tests
    #[test]
    fn prim_cons() {
        let mut vm = test_vm();
        let result =
            eval_primitive(&mut vm, Primitive::Cons, &[Value::Int(1), Value::nil()]).unwrap();
        // Should be Cons((1, Nil))
        if let Value::Constructor { tag, .. } = result {
            assert_eq!(tag, "Cons");
        } else {
            panic!("expected Cons");
        }
    }

    #[test]
    fn prim_list_length() {
        let mut vm = test_vm();
        let list = Value::cons(
            Value::Int(1),
            Value::cons(Value::Int(2), Value::cons(Value::Int(3), Value::nil())),
        );
        let result = eval_primitive(&mut vm, Primitive::ListLength, &[list]).unwrap();
        assert_eq!(result, Value::Int(3));
    }

    #[test]
    fn prim_append() {
        let mut vm = test_vm();
        let list1 = Value::cons(Value::Int(1), Value::cons(Value::Int(2), Value::nil()));
        let list2 = Value::cons(Value::Int(3), Value::nil());
        let result = eval_primitive(&mut vm, Primitive::Append, &[list1, list2]).unwrap();

        // Result should be [1, 2, 3]
        let len = list_length(&result).unwrap();
        assert_eq!(len, 3);
    }

    // IO tests
    #[test]
    fn prim_print() {
        let mut vm = test_vm();
        let result =
            eval_primitive(&mut vm, Primitive::Print, &[Value::string("hello")]).unwrap();
        assert_eq!(result, Value::Unit);

        let output = String::from_utf8(vm.stdout().clone()).unwrap();
        assert_eq!(output, "hello\n");
    }

    #[test]
    fn prim_read_line() {
        let input = b"hello world\n";
        let mut vm = VM::new(Vec::new(), Cursor::new(input.to_vec()));
        let result = eval_primitive(&mut vm, Primitive::ReadLine, &[]).unwrap();
        assert_eq!(result.as_string(), Some("hello world"));
    }

    // Assertion tests
    #[test]
    fn prim_assert_pass() {
        let mut vm = test_vm();
        let result = eval_primitive(
            &mut vm,
            Primitive::Assert,
            &[Value::Bool(true), Value::string("should pass")],
        )
        .unwrap();
        assert_eq!(result, Value::Unit);
    }

    #[test]
    fn prim_assert_fail() {
        let mut vm = test_vm();
        let result = eval_primitive(
            &mut vm,
            Primitive::Assert,
            &[Value::Bool(false), Value::string("test failed")],
        );
        assert!(matches!(
            result,
            Err(RuntimeError::AssertionFailed { message }) if message == "test failed"
        ));
    }

    #[test]
    fn prim_panic() {
        let mut vm = test_vm();
        let result = eval_primitive(&mut vm, Primitive::Panic, &[Value::string("oh no!")]);
        assert!(matches!(
            result,
            Err(RuntimeError::UserError { message }) if message == "oh no!"
        ));
    }

    // Type error tests
    #[test]
    fn prim_add_type_error() {
        let mut vm = test_vm();
        let result = eval_primitive(
            &mut vm,
            Primitive::AddInt,
            &[Value::Int(1), Value::string("not int")],
        );
        assert!(matches!(result, Err(RuntimeError::TypeError { .. })));
    }

    #[test]
    fn prim_wrong_arg_count() {
        let mut vm = test_vm();
        let result = eval_primitive(&mut vm, Primitive::AddInt, &[Value::Int(1)]);
        assert!(matches!(result, Err(RuntimeError::TypeError { .. })));
    }
}
