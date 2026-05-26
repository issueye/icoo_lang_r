#[test]
fn test_stack_trace_shows_function_names() {
    let source = r#"
fn inner() -> Int {
    return int("not_a_number")
}
fn outer() -> Int {
    return inner()
}
outer()
"#;
    let err = icoo_lang_r::run_source(source).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("inner"),
        "trace should contain inner function, got: {}",
        msg
    );
    assert!(
        msg.contains("outer"),
        "trace should contain outer function, got: {}",
        msg
    );
}

#[test]
fn test_stack_trace_shows_at_format() {
    let source = r#"
fn cause_error() -> Int {
    return int("xyz")
}
cause_error()
"#;
    let err = icoo_lang_r::run_source(source).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("at "),
        "trace should contain 'at ' prefix, got: {}",
        msg
    );
    assert!(
        msg.contains("cause_error"),
        "trace should contain function name, got: {}",
        msg
    );
}
