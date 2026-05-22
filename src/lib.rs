pub mod error;
pub mod interpreter;
pub mod lexer;
pub mod native_modules;
pub mod parser;
pub mod resolver;
pub mod runtime;
pub mod typechecker;

use error::IcooError;
pub use runtime::permissions::{PermissionRule, RuntimePermissions};
use std::path::Path;

pub fn run_source(source: &str) -> Result<(), IcooError> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(tokens)?;
    resolver::resolve(&program)?;
    typechecker::check(&program)?;
    let mut interpreter = interpreter::Interpreter::new();
    interpreter.interpret(&program)
}

pub fn run_source_with_permissions(
    source: &str,
    permissions: RuntimePermissions,
) -> Result<(), IcooError> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(tokens)?;
    resolver::resolve(&program)?;
    typechecker::check(&program)?;
    let mut interpreter = interpreter::Interpreter::with_permissions(permissions);
    interpreter.interpret(&program)
}

pub fn run_source_with_output<F>(source: &str, output: F) -> Result<(), IcooError>
where
    F: FnMut(String) + 'static,
{
    let tokens = lexer::lex(source)?;
    let program = parser::parse(tokens)?;
    resolver::resolve(&program)?;
    typechecker::check(&program)?;
    let mut interpreter = interpreter::Interpreter::with_output(output);
    interpreter.interpret(&program)
}

pub fn run_file(path: impl AsRef<Path>) -> Result<(), IcooError> {
    let mut interpreter = interpreter::Interpreter::new();
    interpreter.interpret_file(path)
}

pub fn run_file_with_output<F>(path: impl AsRef<Path>, output: F) -> Result<(), IcooError>
where
    F: FnMut(String) + 'static,
{
    let mut interpreter = interpreter::Interpreter::with_output(output);
    interpreter.interpret_file(path)
}
