use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::IcooResult;
use crate::interpreter::{expect_arity, expect_string, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.env",
    kind: "env",
    type_name: "Env",
    methods: &[
        NativeMethodSpec {
            name: "cwd",
            arity: NativeAritySpec::Exact(0),
            params: &[],
            variadic: None,
            return_type: "String",
        },
        NativeMethodSpec {
            name: "args",
            arity: NativeAritySpec::Exact(0),
            params: &[],
            variadic: None,
            return_type: "Array<String>",
        },
        NativeMethodSpec {
            name: "get",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Any",
        },
        NativeMethodSpec {
            name: "has",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Bool",
        },
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
        "cwd" => {
            expect_arity(&args, 0, span)?;
            runtime.permissions().check_os_info(span)?;
            std::env::current_dir()
                .map(|path| Value::String(path.to_string_lossy().into_owned()))
                .map_err(|err| {
                    crate::error::IcooError::runtime(
                        format!("env.cwd() failed: {}", err),
                        Some(span),
                    )
                })
        }
        "args" => {
            expect_arity(&args, 0, span)?;
            runtime.permissions().check_os_info(span)?;
            Ok(Value::Array(Rc::new(RefCell::new(
                std::env::args().map(Value::String).collect(),
            ))))
        }
        "get" => {
            expect_arity(&args, 1, span)?;
            let name = expect_string(&args[0], span)?;
            runtime.permissions().check_env_read(span)?;
            Ok(std::env::var(name).map(Value::String).unwrap_or(Value::Nil))
        }
        "has" => {
            expect_arity(&args, 1, span)?;
            let name = expect_string(&args[0], span)?;
            runtime.permissions().check_env_read(span)?;
            Ok(Value::Bool(std::env::var_os(name).is_some()))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
