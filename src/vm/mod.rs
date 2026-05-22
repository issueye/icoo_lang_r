pub mod compiler;
pub mod instruction;
pub mod interpreter;

use crate::error::IcooResult;

pub use compiler::{compile_program, compile_source, CompiledProgram};
pub use interpreter::run_program_with_output;

pub fn run_sync_subset_with_output<F>(source: &str, output: F) -> IcooResult<()>
where
    F: FnMut(String),
{
    let program = compile_source(source)?;
    run_program_with_output(&program, output)
}
