//! Tests for the REPL harness.

use super::ReplTestHarness;

#[tokio::test]
async fn test_expression_evaluation() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input("1 + 2").await;
    assert_eq!(r.binding_name(), Some("it"));
    assert_eq!(r.type_str(), Some("int"));
    assert_eq!(r.value_str(), Some("3"));
}

#[tokio::test]
async fn test_arithmetic() {
    let mut repl = ReplTestHarness::new();

    let r = repl.input("10 - 3").await;
    assert_eq!(r.value_str(), Some("7"));

    let r = repl.input("4 * 5").await;
    assert_eq!(r.value_str(), Some("20"));

    let r = repl.input("15 / 3").await;
    assert_eq!(r.value_str(), Some("5"));
}

#[tokio::test]
async fn test_variable_persistence() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input("let x = 42").await;
    assert_eq!(r.binding_name(), Some("x"));
    assert_eq!(r.value_str(), Some("42"));

    let r = repl.input("x * 2").await;
    assert_eq!(r.value_str(), Some("84"));
}

#[tokio::test]
async fn test_function_definition() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input("fn double n = n * 2").await;
    assert!(r.is_function());
    assert_eq!(r.binding_name(), Some("double"));

    let r = repl.input("double 21").await;
    assert_eq!(r.value_str(), Some("42"));
}

#[tokio::test]
async fn test_recursive_function() {
    let mut repl = ReplTestHarness::new();
    repl.input("fn fact n = if n == 0 then 1 else n * fact (n - 1)")
        .await;

    let r = repl.input("fact 5").await;
    assert_eq!(r.value_str(), Some("120"));
}

#[tokio::test]
async fn test_recursive_function_type() {
    let mut repl = ReplTestHarness::new();
    let r = repl
        .input("fn fact n = if n == 0 then 1 else n * fact (n - 1)")
        .await;

    // The type should be int -> int, not int -> 'tN
    assert_eq!(r.type_str(), Some("int -> int"));
}

#[tokio::test]
async fn test_type_error() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input("1 + true").await;
    assert!(r.is_error());
    assert!(r.error_message().unwrap().contains("type"));
}

#[tokio::test]
async fn test_unbound_variable() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input("undefined_var").await;
    assert!(r.is_error());
}

#[tokio::test]
async fn test_type_command() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input(":type 1 + 2").await;
    assert_eq!(r.type_str(), Some("int"));
}

#[tokio::test]
async fn test_type_command_function() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input(":type (x) => x + 1").await;
    assert_eq!(r.type_str(), Some("int -> int"));
}

#[tokio::test]
async fn test_clear_command() {
    let mut repl = ReplTestHarness::new();
    repl.input("let x = 42").await;
    repl.input(":clear").await;

    let r = repl.input("x").await;
    assert!(r.is_error()); // x no longer defined
}

#[tokio::test]
async fn test_help_command() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input(":help").await;
    assert!(r.text().contains(":quit"));
    assert!(r.text().contains(":type"));
}

#[tokio::test]
async fn test_unknown_command() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input(":unknown").await;
    assert!(r.text().contains("Unknown command"));
}

#[tokio::test]
async fn test_string_operations() {
    let mut repl = ReplTestHarness::new();

    let r = repl.input("\"hello\"").await;
    // Value display shows strings with quotes
    assert!(r.value_str().unwrap().contains("hello"));

    let r = repl.input("\"hello\" ++ \" world\"").await;
    assert!(r.value_str().unwrap().contains("hello world"));
}

#[tokio::test]
async fn test_boolean_operations() {
    let mut repl = ReplTestHarness::new();

    let r = repl.input("true && false").await;
    assert_eq!(r.value_str(), Some("false"));

    let r = repl.input("true || false").await;
    assert_eq!(r.value_str(), Some("true"));

    let r = repl.input("!true").await;
    assert_eq!(r.value_str(), Some("false"));
}

#[tokio::test]
async fn test_comparison() {
    let mut repl = ReplTestHarness::new();

    let r = repl.input("5 > 3").await;
    assert_eq!(r.value_str(), Some("true"));

    let r = repl.input("5 < 3").await;
    assert_eq!(r.value_str(), Some("false"));

    let r = repl.input("5 == 5").await;
    assert_eq!(r.value_str(), Some("true"));
}

#[tokio::test]
async fn test_if_expression() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input("if true then 1 else 2").await;
    assert_eq!(r.value_str(), Some("1"));

    let r = repl.input("if false then 1 else 2").await;
    assert_eq!(r.value_str(), Some("2"));
}

#[tokio::test]
async fn test_tuple() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input("(1, 2, 3)").await;
    assert_eq!(r.value_str(), Some("(1, 2, 3)"));
}

#[tokio::test]
async fn test_list() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input("[1, 2, 3]").await;
    assert_eq!(r.value_str(), Some("[1, 2, 3]"));
}

#[tokio::test]
async fn test_record() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input("{ x = 1, y = 2 }").await;
    // Records may be displayed in different order
    assert!(r.text().contains("x = 1"));
    assert!(r.text().contains("y = 2"));
}

#[tokio::test]
async fn test_record_field_access() {
    let mut repl = ReplTestHarness::new();
    repl.input("let r = { x = 1, y = 2 }").await;
    let r = repl.input("r.x").await;
    assert_eq!(r.value_str(), Some("1"));
}

#[tokio::test]
async fn test_lambda() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input("((x) => x + 1) 5").await;
    assert_eq!(r.value_str(), Some("6"));
}

#[tokio::test]
async fn test_pipe() {
    let mut repl = ReplTestHarness::new();
    repl.input("fn add1 x = x + 1").await;
    repl.input("fn mul2 x = x * 2").await;

    let r = repl.input("5 |> add1 |> mul2").await;
    eprintln!("Pipe result: {}, is_error: {}", r.text(), r.is_error());
    if r.is_error() {
        eprintln!("Error: {:?}", r.error_message());
    }
    assert_eq!(r.value_str(), Some("12"));
}

#[tokio::test]
async fn test_show() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input("show 42").await;
    // show returns a string, which displays without quotes
    assert_eq!(r.value_str(), Some("42"));

    let r = repl.input("show true").await;
    assert_eq!(r.value_str(), Some("true"));
}

#[tokio::test]
async fn test_it_binding() {
    let mut repl = ReplTestHarness::new();
    repl.input("5 + 5").await;

    let r = repl.input("it * 2").await;
    assert_eq!(r.value_str(), Some("20"));
}

#[tokio::test]
async fn test_empty_input() {
    let mut repl = ReplTestHarness::new();
    let r = repl.input("").await;
    assert!(r.is_empty());

    let r = repl.input("   ").await;
    assert!(r.is_empty());
}
