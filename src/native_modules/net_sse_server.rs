use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_int, expect_string, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::io::{Read, Write};

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.net.sse.server",
    kind: "net.sse.server",
    type_name: "NetSseServer",
    methods: &[NativeMethodSpec {
        name: "serve_once",
        arity: NativeAritySpec::Exact(3),
        params: &["String", "Int", "Any"],
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
                    "SSE server port must be between 1 and 65535",
                    Some(span),
                ));
            }
            serve_once(runtime, &host, port as u16, &args[2], span)?;
            Ok(Value::Nil)
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}

fn serve_once(
    runtime: &Interpreter,
    host: &str,
    port: u16,
    events: &Value,
    span: Span,
) -> IcooResult<()> {
    runtime
        .permissions()
        .check_net_listen_target(host, port, span)?;
    let listener = std::net::TcpListener::bind((host, port)).map_err(|err| {
        IcooError::runtime(format!("sse server bind failed: {}", err), Some(span))
    })?;
    let (mut stream, _) = listener.accept().map_err(|err| {
        IcooError::runtime(format!("sse server accept failed: {}", err), Some(span))
    })?;
    read_request_head(&mut stream, span)?;
    let body = events_to_sse(events, span)?;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|err| IcooError::runtime(format!("sse server write failed: {}", err), Some(span)))
}

fn read_request_head(stream: &mut std::net::TcpStream, span: Span) -> IcooResult<()> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let size = stream.read(&mut buffer).map_err(|err| {
            IcooError::runtime(format!("sse server read failed: {}", err), Some(span))
        })?;
        if size == 0 {
            return Err(IcooError::runtime(
                "invalid HTTP request: missing header terminator",
                Some(span),
            ));
        }
        bytes.extend_from_slice(&buffer[..size]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(());
        }
        if bytes.len() > 16 * 1024 {
            return Err(IcooError::runtime(
                "HTTP request headers exceed maximum size: 16384 bytes",
                Some(span),
            ));
        }
    }
}

fn events_to_sse(events: &Value, span: Span) -> IcooResult<String> {
    match events {
        Value::Array(values) => {
            let values = values.borrow();
            let mut body = String::new();
            for event in values.iter() {
                body.push_str(&event_to_sse(event, span)?);
            }
            Ok(body)
        }
        _ => event_to_sse(events, span),
    }
}

fn event_to_sse(event: &Value, span: Span) -> IcooResult<String> {
    match event {
        Value::String(data) => Ok(sse_frame(None, None, None, Some(data), span)?),
        Value::Map(map) => {
            let map = map.borrow();
            let event_name = optional_string_field(&map, "event", span)?;
            let id = optional_string_field(&map, "id", span)?;
            let retry = optional_retry_field(&map, span)?;
            let data = map.get("data").map(value_to_sse_text);
            sse_frame(
                event_name.as_deref(),
                id.as_deref(),
                retry,
                data.as_deref(),
                span,
            )
        }
        _ => Err(IcooError::runtime(
            "SSE server events must be a String, Map, or Array",
            Some(span),
        )),
    }
}

fn optional_string_field(
    map: &std::collections::HashMap<String, Value>,
    name: &str,
    span: Span,
) -> IcooResult<Option<String>> {
    match map.get(name) {
        Some(value) => Ok(Some(expect_string(value, span)?)),
        None => Ok(None),
    }
}

fn optional_retry_field(
    map: &std::collections::HashMap<String, Value>,
    span: Span,
) -> IcooResult<Option<i64>> {
    match map.get("retry") {
        Some(Value::Int(value)) if *value >= 0 => Ok(Some(*value)),
        Some(Value::String(value)) => {
            let retry = value.parse::<i64>().map_err(|_| {
                IcooError::runtime("SSE retry field must be a non-negative Int", Some(span))
            })?;
            if retry < 0 {
                return Err(IcooError::runtime(
                    "SSE retry field must be a non-negative Int",
                    Some(span),
                ));
            }
            Ok(Some(retry))
        }
        Some(_) => Err(IcooError::runtime(
            "SSE retry field must be a non-negative Int",
            Some(span),
        )),
        None => Ok(None),
    }
}

fn sse_frame(
    event: Option<&str>,
    id: Option<&str>,
    retry: Option<i64>,
    data: Option<&str>,
    span: Span,
) -> IcooResult<String> {
    let mut frame = String::new();
    if let Some(event) = event {
        ensure_sse_field_no_crlf("event", event, span)?;
        frame.push_str("event: ");
        frame.push_str(event);
        frame.push('\n');
    }
    if let Some(id) = id {
        ensure_sse_field_no_crlf("id", id, span)?;
        frame.push_str("id: ");
        frame.push_str(id);
        frame.push('\n');
    }
    if let Some(retry) = retry {
        frame.push_str("retry: ");
        frame.push_str(&retry.to_string());
        frame.push('\n');
    }
    if let Some(data) = data {
        if data.is_empty() {
            frame.push_str("data: ");
            frame.push('\n');
        } else {
            for line in data.lines() {
                frame.push_str("data: ");
                frame.push_str(line);
                frame.push('\n');
            }
            if data.ends_with('\n') {
                frame.push_str("data: \n");
            }
        }
    }
    frame.push('\n');
    Ok(frame)
}

fn ensure_sse_field_no_crlf(name: &str, value: &str, span: Span) -> IcooResult<()> {
    if value.contains(['\r', '\n']) {
        Err(IcooError::runtime(
            format!("SSE {} field cannot contain CR or LF", name),
            Some(span),
        ))
    } else {
        Ok(())
    }
}

fn value_to_sse_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.display(),
    }
}
