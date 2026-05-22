use super::NativeModuleSpec;
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{
    expect_string, http_client_request, http_stream_method_name, HttpClientHeaders, Interpreter,
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
            expect_arity_range(&args, 1, 2, span)?;
            let url = expect_string(&args[0], span)?;
            let headers = optional_headers(&args, 1, span)?;
            http_client_request("GET", &url, "", &headers, span)
        }
        "post" | "put" => {
            expect_arity_range(&args, 2, 3, span)?;
            let url = expect_string(&args[0], span)?;
            let body = expect_string(&args[1], span)?;
            let headers = optional_headers(&args, 2, span)?;
            http_client_request(&name.to_ascii_uppercase(), &url, &body, &headers, span)
        }
        "delete" | "options" => {
            expect_arity_range(&args, 1, 2, span)?;
            let url = expect_string(&args[0], span)?;
            let headers = optional_headers(&args, 1, span)?;
            http_client_request(&name.to_ascii_uppercase(), &url, "", &headers, span)
        }
        "stream_get" => {
            expect_arity_range(&args, 2, 3, span)?;
            let url = expect_string(&args[0], span)?;
            let (headers, handler) = stream_headers_and_handler(&args, 1, span)?;
            runtime.http_client_stream_request("GET", &url, "", &headers, handler, span)
        }
        "stream_post" | "stream_put" => {
            expect_arity_range(&args, 3, 4, span)?;
            let url = expect_string(&args[0], span)?;
            let body = expect_string(&args[1], span)?;
            let (headers, handler) = stream_headers_and_handler(&args, 2, span)?;
            runtime.http_client_stream_request(
                http_stream_method_name(name),
                &url,
                &body,
                &headers,
                handler,
                span,
            )
        }
        "stream_delete" | "stream_options" => {
            expect_arity_range(&args, 2, 3, span)?;
            let url = expect_string(&args[0], span)?;
            let (headers, handler) = stream_headers_and_handler(&args, 1, span)?;
            runtime.http_client_stream_request(
                http_stream_method_name(name),
                &url,
                "",
                &headers,
                handler,
                span,
            )
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}

fn expect_arity_range(args: &[Value], min: usize, max: usize, span: Span) -> IcooResult<()> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else if min == max {
        Err(IcooError::runtime(
            format!("expected {} arguments but got {}", min, args.len()),
            Some(span),
        ))
    } else {
        Err(IcooError::runtime(
            format!(
                "expected {} to {} arguments but got {}",
                min,
                max,
                args.len()
            ),
            Some(span),
        ))
    }
}

fn optional_headers(args: &[Value], index: usize, span: Span) -> IcooResult<HttpClientHeaders> {
    if args.len() > index {
        expect_headers(&args[index], span)
    } else {
        Ok(Vec::new())
    }
}

fn stream_headers_and_handler(
    args: &[Value],
    header_index: usize,
    span: Span,
) -> IcooResult<(HttpClientHeaders, Value)> {
    if args.len() == header_index + 1 {
        Ok((Vec::new(), args[header_index].clone()))
    } else {
        Ok((
            expect_headers(&args[header_index], span)?,
            args[header_index + 1].clone(),
        ))
    }
}

fn expect_headers(value: &Value, span: Span) -> IcooResult<HttpClientHeaders> {
    let Value::Map(values) = value else {
        return Err(IcooError::runtime(
            "expected Map<String, String> headers argument",
            Some(span),
        ));
    };
    values
        .borrow()
        .iter()
        .map(|(name, value)| Ok((name.clone(), expect_string(value, span)?)))
        .collect()
}
