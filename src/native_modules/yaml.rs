use super::NativeModuleSpec;
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_string, json_to_value, value_to_json};
use crate::lexer::token::Span;
use crate::runtime::value::Value;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.yaml",
    kind: "yaml",
    type_name: "Yaml",
    methods: &["stringify", "parse"],
};

pub(crate) fn call(name: &str, args: Vec<Value>, span: Span) -> Option<IcooResult<Value>> {
    Some(dispatch(name, args, span))
}

fn dispatch(name: &str, args: Vec<Value>, span: Span) -> IcooResult<Value> {
    match name {
        "stringify" => {
            expect_arity(&args, 1, span)?;
            serde_yml::to_string(&value_to_json(&args[0], span)?)
                .map(Value::String)
                .map_err(|err| {
                    IcooError::runtime(format!("yaml.stringify() failed: {}", err), Some(span))
                })
        }
        "parse" => {
            expect_arity(&args, 1, span)?;
            let text = expect_string(&args[0], span)?;
            let parsed = serde_yml::from_str::<serde_json::Value>(&text).map_err(|err| {
                IcooError::runtime(format!("yaml.parse() failed: {}", err), Some(span))
            })?;
            json_to_value(parsed, span)
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
