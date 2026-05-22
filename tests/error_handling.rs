use std::cell::RefCell;
use std::rc::Rc;

fn run(source: &str) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    icoo_lang_r::run_source_with_output(source, move |line| {
        captured.borrow_mut().push(line);
    })
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
}

#[test]
fn catches_runtime_errors_and_binds_string_error() {
    let output = run(r#"
try:
    int("not-an-int")
catch err:
    print(err.type_name())
    print(err.to_string().contains("runtime error").to_string())
    print(err.to_string().contains("cannot convert 'not-an-int' to Int").to_string())
"#)
    .unwrap();

    assert_eq!(output, vec!["String", "true", "true"]);
}

#[test]
fn does_not_catch_return_signal() {
    let output = run(r#"
fn exits() -> String:
    try:
        return "returned"
    catch err:
        return "caught"
    return "after"

print(exits())
"#)
    .unwrap();

    assert_eq!(output, vec!["returned"]);
}

#[test]
fn does_not_catch_break_signal() {
    let output = run(r#"
let count = 0
while count < 1:
    try:
        break
    catch err:
        print("caught")
        count = 1

print("after")
"#)
    .unwrap();

    assert_eq!(output, vec!["after"]);
}

#[test]
fn catch_binding_does_not_escape_scope() {
    let err = run(r#"
try:
    int("bad")
catch err:
    print(err.to_string().contains("cannot convert").to_string())

print(err)
"#)
    .unwrap_err();

    assert!(err.contains("undefined variable 'err'"));
}

#[test]
fn skips_catch_when_try_block_succeeds() {
    let output = run(r#"
try:
    print("try")
catch err:
    print("catch")

print("after")
"#)
    .unwrap();

    assert_eq!(output, vec!["try", "after"]);
}
