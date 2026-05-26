#[test]
fn test_add_overflow_rejected() {
    let source = "let a = 9223372036854775807
let b = a + 1";
    let err = icoo_lang_r::run_source(source).unwrap_err();
    assert!(
        err.to_string().contains("overflow"),
        "expected overflow in add, got: {}",
        err
    );
}

#[test]
fn test_mul_overflow_rejected() {
    let source = "let a = 9223372036854775807
let b = a * 2";
    let err = icoo_lang_r::run_source(source).unwrap_err();
    assert!(
        err.to_string().contains("overflow"),
        "expected overflow in mul, got: {}",
        err
    );
}

#[test]
fn test_normal_arithmetic_still_works() {
    let source = r#"
let a = 100
let b = a + 50
let c = b - 30
let d = c * 2
let e = d / 4
"#;
    icoo_lang_r::run_source(source).expect("normal arithmetic should work");
}
