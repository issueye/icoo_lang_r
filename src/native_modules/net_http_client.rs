use super::NativeModuleSpec;
use crate::error::IcooResult;
use crate::interpreter::{
    expect_arity, expect_string, http_client_request, http_stream_method_name, Interpreter,
};
use crate::lexer::token::Span;
use crate::runtime::value::Value;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.net.http.client",
    kind: "net.http.client",
    type_name: "NetHttpClient",
    methods: &[
        "get",
        "post",
        "put",
        "delete",
        "options",
        "stream_get",
        "stream_post",
        "stream_put",
        "stream_delete",
        "stream_options",
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
        "get" => {
            expect_arity(&args, 1, span)?;
            let url = expect_string(&args[0], span)?;
            http_client_request("GET", &url, "", span)
        }
        "post" | "put" => {
            expect_arity(&args, 2, span)?;
            let url = expect_string(&args[0], span)?;
            let body = expect_string(&args[1], span)?;
            http_client_request(&name.to_ascii_uppercase(), &url, &body, span)
        }
        "delete" | "options" => {
            expect_arity(&args, 1, span)?;
            let url = expect_string(&args[0], span)?;
            http_client_request(&name.to_ascii_uppercase(), &url, "", span)
        }
        "stream_get" => {
            expect_arity(&args, 2, span)?;
            let url = expect_string(&args[0], span)?;
            let handler = args[1].clone();
            runtime.http_client_stream_request("GET", &url, "", handler, span)
        }
        "stream_post" | "stream_put" => {
            expect_arity(&args, 3, span)?;
            let url = expect_string(&args[0], span)?;
            let body = expect_string(&args[1], span)?;
            let handler = args[2].clone();
            runtime.http_client_stream_request(
                http_stream_method_name(name),
                &url,
                &body,
                handler,
                span,
            )
        }
        "stream_delete" | "stream_options" => {
            expect_arity(&args, 2, span)?;
            let url = expect_string(&args[0], span)?;
            let handler = args[1].clone();
            runtime.http_client_stream_request(
                http_stream_method_name(name),
                &url,
                "",
                handler,
                span,
            )
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
