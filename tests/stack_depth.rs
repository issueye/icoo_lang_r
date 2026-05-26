#[test]
fn test_stack_depth_exceeded() {
    let source = r#"
fn recurse(n: Int) -> Int {
    if n == 0 {
        return 0
    }
    return recurse(n - 1)
}
recurse(150)
"#;
    let err = icoo_lang_r::run_source(source).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("stack depth"),
        "expected 'stack depth' in error message, got: {}",
        msg
    );
}

#[test]
fn test_stack_depth_within_limit() {
    let source = r#"
fn recurse(n: Int) -> Int {
    if n == 0 {
        return 0
    }
    return recurse(n - 1)
}
recurse(50)
"#;
    icoo_lang_r::run_source(source).expect("50-level recursion should succeed");
}
