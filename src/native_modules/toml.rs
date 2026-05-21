use super::NativeModuleSpec;
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_string, toml_to_value, value_to_toml};
use crate::lexer::token::Span;
use crate::runtime::value::Value;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.toml",
    kind: "toml",
    type_name: "Toml",
    methods: &["stringify", "parse"],
};

pub(crate) fn call(name: &str, args: Vec<Value>, span: Span) -> Option<IcooResult<Value>> {
    Some(dispatch(name, args, span))
}

fn dispatch(name: &str, args: Vec<Value>, span: Span) -> IcooResult<Value> {
    match name {
        "stringify" => {
            expect_arity(&args, 1, span)?;
            toml::to_string(&value_to_toml(&args[0], span)?)
                .map(Value::String)
                .map_err(|err| {
                    IcooError::runtime(format!("toml.stringify() failed: {}", err), Some(span))
                })
        }
        "parse" => {
            expect_arity(&args, 1, span)?;
            let text = expect_string(&args[0], span)?;
            let parsed = toml::from_str::<toml::Value>(&text).map_err(|err| {
                IcooError::runtime(format!("toml.parse() failed: {}", err), Some(span))
            })?;
            toml_to_value(parsed, span)
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
