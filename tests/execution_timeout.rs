use std::time::Duration;

#[test]
fn test_execution_timeout_infinite_loop() {
    let err = icoo_lang_r::run_source_with_config(
        "while true {}",
        icoo_lang_r::RuntimeConfig::default().with_timeout(Duration::from_millis(100)),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("timed out"),
        "expected 'timed out' in error message, got: {}",
        err
    );
}

#[test]
fn test_execution_timeout_busy_loop() {
    let source = r#"
let i = 0
while i < 10000000 {
    i = i + 1
}
"#;
    let err = icoo_lang_r::run_source_with_config(
        source,
        icoo_lang_r::RuntimeConfig::default().with_timeout(Duration::from_millis(50)),
    )
    .unwrap_err();
    assert!(err.to_string().contains("timed out"));
}

#[test]
fn test_no_timeout_within_limit() {
    let result = icoo_lang_r::run_source_with_config(
        "let x = 1 + 2",
        icoo_lang_r::RuntimeConfig::default().with_timeout(Duration::from_secs(5)),
    );
    assert!(
        result.is_ok(),
        "simple script should complete within timeout"
    );
}
