use icoo_lang_r::runtime::logging::{LogLevel, RuntimeLogRecord, RuntimeLogger};
use std::sync::{Arc, Mutex};

#[test]
fn log_level_display_uses_lowercase_names() {
    assert_eq!(LogLevel::Debug.to_string(), "debug");
    assert_eq!(LogLevel::Info.to_string(), "info");
    assert_eq!(LogLevel::Warn.to_string(), "warn");
    assert_eq!(LogLevel::Error.to_string(), "error");
}

#[test]
fn runtime_log_record_construction_stores_owned_fields() {
    let record = RuntimeLogRecord::new(LogLevel::Warn, "std.log", "disk is almost full");

    assert_eq!(record.level, LogLevel::Warn);
    assert_eq!(record.target, "std.log");
    assert_eq!(record.message, "disk is almost full");
}

#[test]
fn runtime_logger_callback_captures_records() {
    let records = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&records);
    let logger = RuntimeLogger::callback(move |record| {
        captured.lock().unwrap().push(record);
    });

    logger.log(RuntimeLogRecord::new(LogLevel::Info, "runtime", "started"));
    logger.log_message(LogLevel::Error, "runtime", "failed");

    let records = records.lock().unwrap();
    assert_eq!(
        records.as_slice(),
        &[
            RuntimeLogRecord::new(LogLevel::Info, "runtime", "started"),
            RuntimeLogRecord::new(LogLevel::Error, "runtime", "failed"),
        ]
    );
}

#[test]
fn runtime_logger_default_is_noop() {
    let logger = RuntimeLogger::default();

    assert!(logger.is_noop());
    logger.log(RuntimeLogRecord::new(
        LogLevel::Debug,
        "runtime",
        "nothing receives this",
    ));
}

#[test]
fn runtime_logger_noop_constructor_drops_records() {
    let logger = RuntimeLogger::noop();

    assert!(logger.is_noop());
    logger.log_message(LogLevel::Info, "runtime", "nothing receives this");
}
