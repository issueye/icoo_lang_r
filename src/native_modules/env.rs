use super::NativeModuleSpec;
use crate::error::IcooResult;
use crate::interpreter::{expect_arity, expect_string};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.env",
    kind: "env",
    type_name: "Env",
    methods: &["cwd", "args", "get", "has"],
};

pub(crate) fn call(name: &str, args: Vec<Value>, span: Span) -> Option<IcooResult<Value>> {
    Some(dispatch(name, args, span))
}

fn dispatch(name: &str, args: Vec<Value>, span: Span) -> IcooResult<Value> {
    match name {
        "cwd" => {
            expect_arity(&args, 0, span)?;
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
            Ok(Value::Array(Rc::new(RefCell::new(
                std::env::args().map(Value::String).collect(),
            ))))
        }
        "get" => {
            expect_arity(&args, 1, span)?;
            let name = expect_string(&args[0], span)?;
            Ok(std::env::var(name).map(Value::String).unwrap_or(Value::Nil))
        }
        "has" => {
            expect_arity(&args, 1, span)?;
            let name = expect_string(&args[0], span)?;
            Ok(Value::Bool(std::env::var_os(name).is_some()))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
