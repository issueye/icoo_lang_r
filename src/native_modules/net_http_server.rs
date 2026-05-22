use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{
    expect_arity, expect_int, expect_string, http_server_serve_once, Interpreter,
};
use crate::lexer::token::Span;
use crate::runtime::value::Value;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.net.http.server",
    kind: "net.http.server",
    type_name: "NetHttpServer",
    methods: &[NativeMethodSpec {
        name: "serve_once",
        arity: NativeAritySpec::Exact(3),
        params: &["String", "Int", "String"],
        variadic: None,
        return_type: "Nil",
    }],
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
        "serve_once" => {
            expect_arity(&args, 3, span)?;
            let host = expect_string(&args[0], span)?;
            let port = expect_int(&args[1], span)?;
            if !(1..=65535).contains(&port) {
                return Err(IcooError::runtime(
                    "server port must be between 1 and 65535",
                    Some(span),
                ));
            }
            let body = expect_string(&args[2], span)?;
            http_server_serve_once(runtime.permissions(), &host, port as u16, &body, span)?;
            Ok(Value::Nil)
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
