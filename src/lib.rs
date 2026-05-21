pub mod error;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod resolver;
pub mod runtime;
pub mod typechecker;

use error::IcooError;

pub fn run_source(source: &str) -> Result<(), IcooError> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(tokens)?;
    resolver::resolve(&program)?;
    typechecker::check(&program)?;
    let mut interpreter = interpreter::Interpreter::new();
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
