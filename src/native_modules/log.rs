use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::IcooResult;
use crate::interpreter::{expect_arity, expect_string, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::logging::LogLevel;
use crate::runtime::value::Value;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.log",
    kind: "log",
    type_name: "Log",
    methods: &[
        NativeMethodSpec {
            name: "debug",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "info",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "warn",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "error",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Nil",
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
    expect_arity(&args, 1, span)?;
    let message = expect_string(&args[0], span)?;
    let level = match name {
        "debug" => LogLevel::Debug,
        "info" => LogLevel::Info,
        "warn" => LogLevel::Warn,
        "error" => LogLevel::Error,
        _ => unreachable!("native module method should be registered before dispatch"),
    };
    runtime.logger().log_message(level, "std.log", message);
    Ok(Value::Nil)
}
