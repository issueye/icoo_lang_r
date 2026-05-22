use super::NativeModuleSpec;
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_string, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.os",
    kind: "os",
    type_name: "Os",
    methods: &[
        "name", "family", "arch", "pid", "cwd", "args", "exe_path", "get_env", "has_env",
    ],
};

pub(crate) fn call(
    runtime: &mut Interpreter,
    name: &str,
    args: Vec<Value>,
    span: Span,
) -> Option<IcooResult<Value>> {
    Some(dispatch(runtime, name, args, span))
}

fn dispatch(
    runtime: &mut Interpreter,
    name: &str,
    args: Vec<Value>,
    span: Span,
) -> IcooResult<Value> {
    match name {
        "name" => {
            expect_arity(&args, 0, span)?;
            runtime.permissions().check_os_info(span)?;
            Ok(Value::String(std::env::consts::OS.to_string()))
        }
        "family" => {
            expect_arity(&args, 0, span)?;
            runtime.permissions().check_os_info(span)?;
            Ok(Value::String(std::env::consts::FAMILY.to_string()))
        }
        "arch" => {
            expect_arity(&args, 0, span)?;
            runtime.permissions().check_os_info(span)?;
            Ok(Value::String(std::env::consts::ARCH.to_string()))
        }
        "pid" => {
            expect_arity(&args, 0, span)?;
            runtime.permissions().check_os_info(span)?;
            Ok(Value::Int(std::process::id() as i64))
        }
        "cwd" => {
            expect_arity(&args, 0, span)?;
            runtime.permissions().check_os_info(span)?;
            std::env::current_dir()
                .map(|path| Value::String(path.to_string_lossy().into_owned()))
                .map_err(|err| IcooError::runtime(format!("os.cwd() failed: {}", err), Some(span)))
        }
        "args" => {
            expect_arity(&args, 0, span)?;
            runtime.permissions().check_os_info(span)?;
            Ok(Value::Array(Rc::new(RefCell::new(
                std::env::args().map(Value::String).collect(),
            ))))
        }
        "exe_path" => {
            expect_arity(&args, 0, span)?;
            runtime.permissions().check_os_info(span)?;
            Ok(std::env::current_exe()
                .map(|path| Value::String(path.to_string_lossy().into_owned()))
                .unwrap_or(Value::Nil))
        }
        "get_env" => {
            expect_arity(&args, 1, span)?;
            let name = expect_string(&args[0], span)?;
            runtime.permissions().check_env_read(span)?;
            Ok(std::env::var(name).map(Value::String).unwrap_or(Value::Nil))
        }
        "has_env" => {
            expect_arity(&args, 1, span)?;
            let name = expect_string(&args[0], span)?;
            runtime.permissions().check_env_read(span)?;
            Ok(Value::Bool(std::env::var_os(name).is_some()))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
