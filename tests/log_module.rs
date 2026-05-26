use icoo_lang_r::{LogLevel, RuntimeLogRecord, RuntimeLogger};
use std::sync::{Arc, Mutex};

#[test]
fn std_log_routes_records_to_runtime_logger() {
    let records = Arc::new(Mutex::new(Vec::new()));
    let captured = records.clone();
    let logger = RuntimeLogger::callback(move |record| {
        captured.lock().unwrap().push(record);
    });

    icoo_lang_r::run_source_with_logger(
        r#"import "std.log" as log

log.debug("starting")
log.info("ready")
log.warn("slow")
log.error("failed")"#,
        logger,
    )
    .unwrap();

    assert_eq!(
        *records.lock().unwrap(),
        vec![
            RuntimeLogRecord::new(LogLevel::Debug, "std.log", "starting"),
            RuntimeLogRecord::new(LogLevel::Info, "std.log", "ready"),
            RuntimeLogRecord::new(LogLevel::Warn, "std.log", "slow"),
            RuntimeLogRecord::new(LogLevel::Error, "std.log", "failed"),
        ]
    );
}

#[test]
fn std_log_default_logger_is_noop() {
    icoo_lang_r::run_source(
        r#"import "std.log" as log

log.info("discarded")"#,
    )
    .unwrap();
}

#[test]
fn typechecker_uses_std_log_metadata_for_argument_checks() {
    let err = icoo_lang_r::check_source(
        r#"import "std.log" as log
log.warn(1)"#,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("type error: expected String for argument 1 but got Int"));
}
