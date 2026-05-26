use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{
    expect_bytes, expect_string, http_client_request, http_client_request_bytes,
    http_stream_method_name, HttpClientHeaders, Interpreter,
};
use crate::lexer::token::Span;
use crate::runtime::value::Value;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.net.http.client",
    kind: "net.http.client",
    type_name: "NetHttpClient",
    methods: &[
        NativeMethodSpec {
            name: "get",
            arity: NativeAritySpec::Range { min: 1, max: 2 },
            params: &["String", "Map"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "get_bytes",
            arity: NativeAritySpec::Range { min: 1, max: 2 },
            params: &["String", "Map"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "post",
            arity: NativeAritySpec::Range { min: 2, max: 3 },
            params: &["String", "String", "Map"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "post_bytes",
            arity: NativeAritySpec::Range { min: 2, max: 3 },
            params: &["String", "Bytes", "Map"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "put",
            arity: NativeAritySpec::Range { min: 2, max: 3 },
            params: &["String", "String", "Map"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "put_bytes",
            arity: NativeAritySpec::Range { min: 2, max: 3 },
            params: &["String", "Bytes", "Map"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "delete",
            arity: NativeAritySpec::Range { min: 1, max: 2 },
            params: &["String", "Map"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "delete_bytes",
            arity: NativeAritySpec::Range { min: 1, max: 2 },
            params: &["String", "Map"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "options",
            arity: NativeAritySpec::Range { min: 1, max: 2 },
            params: &["String", "Map"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "options_bytes",
            arity: NativeAritySpec::Range { min: 1, max: 2 },
            params: &["String", "Map"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "stream_get",
            arity: NativeAritySpec::Range { min: 2, max: 3 },
            params: &["String", "Any", "Function"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "stream_get_bytes",
            arity: NativeAritySpec::Range { min: 2, max: 3 },
            params: &["String", "Any", "Function"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "stream_post",
            arity: NativeAritySpec::Range { min: 3, max: 4 },
            params: &["String", "String", "Any", "Function"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "stream_post_bytes",
            arity: NativeAritySpec::Range { min: 3, max: 4 },
            params: &["String", "Bytes", "Any", "Function"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "stream_put",
            arity: NativeAritySpec::Range { min: 3, max: 4 },
            params: &["String", "String", "Any", "Function"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "stream_put_bytes",
            arity: NativeAritySpec::Range { min: 3, max: 4 },
            params: &["String", "Bytes", "Any", "Function"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "stream_delete",
            arity: NativeAritySpec::Range { min: 2, max: 3 },
            params: &["String", "Any", "Function"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "stream_options",
            arity: NativeAritySpec::Range { min: 2, max: 3 },
            params: &["String", "Any", "Function"],
            variadic: None,
            return_type: "Map<String, Any>",
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
    match name {
        "get" => {
            expect_arity_range(&args, 1, 2, span)?;
            let url = expect_string(&args[0], span)?;
            let headers = optional_headers(&args, 1, span)?;
            http_client_request(runtime.permissions(), "GET", &url, "", &headers, span)
        }
        "get_bytes" => {
            expect_arity_range(&args, 1, 2, span)?;
            let url = expect_string(&args[0], span)?;
            let headers = optional_headers(&args, 1, span)?;
            http_client_request_bytes(runtime.permissions(), "GET", &url, &[], &headers, span)
        }
        "post" | "put" => {
            expect_arity_range(&args, 2, 3, span)?;
            let url = expect_string(&args[0], span)?;
            let body = expect_string(&args[1], span)?;
            let headers = optional_headers(&args, 2, span)?;
            http_client_request(
                runtime.permissions(),
                &name.to_ascii_uppercase(),
                &url,
                &body,
                &headers,
                span,
            )
        }
        "post_bytes" | "put_bytes" => {
            expect_arity_range(&args, 2, 3, span)?;
            let url = expect_string(&args[0], span)?;
            let body = expect_bytes(&args[1], span)?;
            let headers = optional_headers(&args, 2, span)?;
            let method = if name == "post_bytes" { "POST" } else { "PUT" };
            http_client_request_bytes(runtime.permissions(), method, &url, &body, &headers, span)
        }
        "delete" | "options" => {
            expect_arity_range(&args, 1, 2, span)?;
            let url = expect_string(&args[0], span)?;
            let headers = optional_headers(&args, 1, span)?;
            http_client_request(
                runtime.permissions(),
                &name.to_ascii_uppercase(),
                &url,
                "",
                &headers,
                span,
            )
        }
        "delete_bytes" | "options_bytes" => {
            expect_arity_range(&args, 1, 2, span)?;
            let url = expect_string(&args[0], span)?;
            let headers = optional_headers(&args, 1, span)?;
            let method = if name == "delete_bytes" {
                "DELETE"
            } else {
                "OPTIONS"
            };
            http_client_request_bytes(runtime.permissions(), method, &url, &[], &headers, span)
        }
        "stream_get" => {
            expect_arity_range(&args, 2, 3, span)?;
            let url = expect_string(&args[0], span)?;
            let (headers, handler) = stream_headers_and_handler(&args, 1, span)?;
            runtime.http_client_stream_request("GET", &url, "", &headers, handler, span)
        }
        "stream_get_bytes" => {
            expect_arity_range(&args, 2, 3, span)?;
            let url = expect_string(&args[0], span)?;
            let (headers, handler) = stream_headers_and_handler(&args, 1, span)?;
            runtime.http_client_stream_request_bytes("GET", &url, &[], &headers, handler, span)
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
        "stream_post_bytes" | "stream_put_bytes" => {
            expect_arity_range(&args, 3, 4, span)?;
            let url = expect_string(&args[0], span)?;
            let body = expect_bytes(&args[1], span)?;
            let (headers, handler) = stream_headers_and_handler(&args, 2, span)?;
            runtime.http_client_stream_request_bytes(
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
