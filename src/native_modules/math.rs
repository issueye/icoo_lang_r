use super::NativeModuleSpec;
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_number, now_duration, numeric_min_max};
use crate::lexer::token::Span;
use crate::runtime::value::Value;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.math",
    kind: "math",
    type_name: "Math",
    methods: &["abs", "floor", "ceil", "round", "min", "max", "random"],
};

pub(crate) fn call(name: &str, args: Vec<Value>, span: Span) -> Option<IcooResult<Value>> {
    Some(dispatch(name, args, span))
}

fn dispatch(name: &str, args: Vec<Value>, span: Span) -> IcooResult<Value> {
    match name {
        "abs" => {
            expect_arity(&args, 1, span)?;
            match &args[0] {
                Value::Int(value) => Ok(Value::Int(value.abs())),
                Value::Float(value) => Ok(Value::Float(value.abs())),
                _ => Err(IcooError::runtime("expected numeric argument", Some(span))),
            }
        }
        "floor" => {
            expect_arity(&args, 1, span)?;
            Ok(Value::Int(expect_number(&args[0], span)?.floor() as i64))
        }
        "ceil" => {
            expect_arity(&args, 1, span)?;
            Ok(Value::Int(expect_number(&args[0], span)?.ceil() as i64))
        }
        "round" => {
            expect_arity(&args, 1, span)?;
            Ok(Value::Int(expect_number(&args[0], span)?.round() as i64))
        }
        "min" => {
            expect_arity(&args, 2, span)?;
            numeric_min_max(&args[0], &args[1], span, f64::min)
        }
        "max" => {
            expect_arity(&args, 2, span)?;
            numeric_min_max(&args[0], &args[1], span, f64::max)
        }
        "random" => {
            expect_arity(&args, 0, span)?;
            let nanos = now_duration(span)?.subsec_nanos();
            Ok(Value::Float((nanos as f64) / 1_000_000_000.0))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
