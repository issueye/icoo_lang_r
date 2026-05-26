pub mod error;
pub mod interpreter;
pub mod lexer;
pub mod native_modules;
pub mod parser;
pub mod resolver;
pub mod runtime;
pub mod typechecker;
pub mod vm;

use error::IcooError;
use std::panic::{AssertUnwindSafe, catch_unwind};
pub use runtime::config::RuntimeConfig;
pub use runtime::http_config::{HttpProxyConfig, RuntimeHttpConfig};
pub use runtime::logging::{LogLevel, RuntimeLogRecord, RuntimeLogger};
pub use runtime::permissions::{PermissionRule, RuntimePermissions};
use std::path::Path;

fn parse_and_check(source: &str) -> Result<parser::ast::Program, IcooError> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(tokens)?;
    resolver::resolve(&program)?;
    typechecker::check(&program)?;
    Ok(program)
}

fn run_protected<F>(f: F) -> Result<(), IcooError>
where
    F: FnOnce() -> Result<(), IcooError>,
{
    let result = catch_unwind(AssertUnwindSafe(f));
    match result {
        Ok(ok) => ok,
        Err(panic_info) => {
            let msg = panic_info
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic_info.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "internal panic".to_string());
            Err(IcooError::runtime(format!("internal runtime panic: {}", msg), None))
        }
    }
}

pub fn check_source(source: &str) -> Result<(), IcooError> {
    parse_and_check(source).map(|_| ())
}

pub fn check_file(path: impl AsRef<Path>) -> Result<(), IcooError> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path).map_err(|err| {
        IcooError::runtime(
            format!("failed to read file '{}': {}", path.display(), err),
            None,
        )
    })?;
    check_source(&source)
}

pub fn run_source(source: &str) -> Result<(), IcooError> {
    let program = parse_and_check(source)?;
    let mut interpreter = interpreter::Interpreter::new();
    run_protected(|| interpreter.interpret(&program))
}

pub fn run_source_with_permissions(
    source: &str,
    permissions: RuntimePermissions,
) -> Result<(), IcooError> {
    let program = parse_and_check(source)?;
    let mut interpreter = interpreter::Interpreter::with_permissions(permissions);
    run_protected(|| interpreter.interpret(&program))
}

pub fn run_source_with_logger(source: &str, logger: RuntimeLogger) -> Result<(), IcooError> {
    run_source_with_permissions_and_logger(source, RuntimePermissions::default(), logger)
}

pub fn run_source_with_permissions_and_logger(
    source: &str,
    permissions: RuntimePermissions,
    logger: RuntimeLogger,
) -> Result<(), IcooError> {
    let program = parse_and_check(source)?;
    let mut interpreter = interpreter::Interpreter::with_output_permissions_and_logger(
        |line| println!("{}", line),
        permissions,
        logger,
    );
    run_protected(|| interpreter.interpret(&program))
}

pub fn run_source_with_http_tls_roots(
    source: &str,
    roots: rustls::RootCertStore,
) -> Result<(), IcooError> {
    run_source_with_permissions_and_http_tls_roots(source, RuntimePermissions::default(), roots)
}

pub fn run_source_with_permissions_and_http_tls_roots(
    source: &str,
    permissions: RuntimePermissions,
    roots: rustls::RootCertStore,
) -> Result<(), IcooError> {
    let program = parse_and_check(source)?;
    let mut interpreter = interpreter::Interpreter::with_output_permissions_logger_and_tls_roots(
        |line| println!("{}", line),
        permissions,
        RuntimeLogger::default(),
        Some(std::sync::Arc::new(roots)),
    );
    run_protected(|| interpreter.interpret(&program))
}

pub fn run_source_with_output_and_http_tls_roots<F>(
    source: &str,
    output: F,
    roots: rustls::RootCertStore,
) -> Result<(), IcooError>
where
    F: FnMut(String) + 'static,
{
    let program = parse_and_check(source)?;
    let mut interpreter = interpreter::Interpreter::with_output_permissions_logger_and_tls_roots(
        output,
        RuntimePermissions::default(),
        RuntimeLogger::default(),
        Some(std::sync::Arc::new(roots)),
    );
    run_protected(|| interpreter.interpret(&program))
}

pub fn run_source_with_output_and_http_config<F>(
    source: &str,
    output: F,
    http_config: RuntimeHttpConfig,
) -> Result<(), IcooError>
where
    F: FnMut(String) + 'static,
{
    let program = parse_and_check(source)?;
    let mut interpreter =
        interpreter::Interpreter::with_output_permissions_logger_tls_roots_and_http_config(
            output,
            RuntimePermissions::default(),
            RuntimeLogger::default(),
            None,
            http_config,
            RuntimeConfig::default(),
        );
    run_protected(|| interpreter.interpret(&program))
}

pub fn run_source_with_output_http_config_and_tls_roots<F>(
    source: &str,
    output: F,
    http_config: RuntimeHttpConfig,
    roots: rustls::RootCertStore,
) -> Result<(), IcooError>
where
    F: FnMut(String) + 'static,
{
    let program = parse_and_check(source)?;
    let mut interpreter =
        interpreter::Interpreter::with_output_permissions_logger_tls_roots_and_http_config(
            output,
            RuntimePermissions::default(),
            RuntimeLogger::default(),
            Some(std::sync::Arc::new(roots)),
            http_config,
            RuntimeConfig::default(),
        );
    run_protected(|| interpreter.interpret(&program))
}

pub fn run_source_with_config(
    source: &str,
    config: RuntimeConfig,
) -> Result<(), IcooError> {
    let program = parse_and_check(source)?;
    let mut interpreter =
        interpreter::Interpreter::with_output_permissions_logger_tls_roots_and_http_config(
            |line| println!("{}", line),
            RuntimePermissions::default(),
            RuntimeLogger::default(),
            None,
            RuntimeHttpConfig::default(),
            config,
        );
    run_protected(|| interpreter.interpret(&program))
}

pub fn run_source_with_output<F>(source: &str, output: F) -> Result<(), IcooError>
where
    F: FnMut(String) + 'static,
{
    let program = parse_and_check(source)?;
    let mut interpreter = interpreter::Interpreter::with_output(output);
    run_protected(|| interpreter.interpret(&program))
}

pub fn run_file(path: impl AsRef<Path>) -> Result<(), IcooError> {
    let mut interpreter = interpreter::Interpreter::new();
    run_protected(|| interpreter.interpret_file(path))
}

pub fn run_file_with_output<F>(path: impl AsRef<Path>, output: F) -> Result<(), IcooError>
where
    F: FnMut(String) + 'static,
{
    let mut interpreter = interpreter::Interpreter::with_output(output);
    run_protected(|| interpreter.interpret_file(path))
}
