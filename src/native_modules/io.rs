use super::NativeModuleSpec;
use crate::error::IcooResult;
use crate::interpreter::{expect_arity, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::value::Value;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.io",
    kind: "io",
    type_name: "Io",
    methods: &["print"],
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
        "print" => {
            expect_arity(&args, 1, span)?;
            runtime.emit_output(args[0].display());
            Ok(Value::Nil)
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
