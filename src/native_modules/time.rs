use super::NativeModuleSpec;
use crate::error::IcooResult;
use crate::interpreter::{expect_arity, now_duration};
use crate::lexer::token::Span;
use crate::runtime::value::Value;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.time",
    kind: "time",
    type_name: "Time",
    methods: &["now_ms", "now_sec"],
};

pub(crate) fn call(name: &str, args: Vec<Value>, span: Span) -> Option<IcooResult<Value>> {
    Some(dispatch(name, args, span))
}

fn dispatch(name: &str, args: Vec<Value>, span: Span) -> IcooResult<Value> {
    match name {
        "now_ms" => {
            expect_arity(&args, 0, span)?;
            Ok(Value::Int(now_duration(span)?.as_millis() as i64))
        }
        "now_sec" => {
            expect_arity(&args, 0, span)?;
            Ok(Value::Int(now_duration(span)?.as_secs() as i64))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
