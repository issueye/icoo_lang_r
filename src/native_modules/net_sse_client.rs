use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_string, is_callable, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::rc::Rc;
use std::time::Duration;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.net.sse.client",
    kind: "net.sse.client",
    type_name: "NetSseClient",
    methods: &[NativeMethodSpec {
        name: "get",
        arity: NativeAritySpec::Exact(2),
        params: &["String", "Function"],
        variadic: None,
        return_type: "Map<String, Any>",
    }],
};

struct ParsedHttpUrl {
    host: String,
    port: u16,
    path: String,
}

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
            expect_arity(&args, 2, span)?;
            let url = expect_string(&args[0], span)?;
            sse_client_get(runtime, &url, args[1].clone(), span)
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}

fn sse_client_get(
    runtime: &mut Interpreter,
    url: &str,
    handler: Value,
    span: Span,
) -> IcooResult<Value> {
    if !is_callable(&handler) {
        return Err(IcooError::runtime(
            "SSE client handler must be a Function",
            Some(span),
        ));
    }
    let parsed = parse_http_url(url, span)?;
    runtime
        .permissions()
        .check_net_connect_target(&parsed.host, parsed.port, span)?;

    let mut stream =
        std::net::TcpStream::connect((parsed.host.as_str(), parsed.port)).map_err(|err| {
            IcooError::runtime(format!("sse client connection failed: {}", err), Some(span))
        })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|err| IcooError::runtime(format!("sse client failed: {}", err), Some(span)))?;

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nAccept: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        parsed.path, parsed.host
    );
    stream.write_all(request.as_bytes()).map_err(|err| {
        IcooError::runtime(format!("sse client write failed: {}", err), Some(span))
    })?;

    let response = read_http_response(&mut stream, span)?;
    let body = if response.transfer_chunked {
        decode_chunked_bytes(&response.body, span)?
    } else {
        response.body
    };
    let events = parse_sse_events(&String::from_utf8_lossy(&body));
    let event_count = events.len();
    for event in events {
        call_handler(runtime, &handler, event_to_value(event), span)?;
    }
    Ok(result_value(response.status, event_count))
}

fn parse_http_url(url: &str, span: Span) -> IcooResult<ParsedHttpUrl> {
    let Some(rest) = url.strip_prefix("http://") else {
        return Err(IcooError::runtime(
            "SSE client only supports http:// URLs",
            Some(span),
        ));
    };
    let (host_port, path) = split_http_authority_and_path(rest);
    if host_port.is_empty() {
        return Err(IcooError::runtime("SSE URL host is required", Some(span)));
    }
    let (host, port) = if let Some((host, port)) = host_port.rsplit_once(':') {
        if host.is_empty() {
            return Err(IcooError::runtime("SSE URL host is required", Some(span)));
        }
        let port = port.parse::<u16>().map_err(|_| {
            IcooError::runtime("SSE URL port must be between 1 and 65535", Some(span))
        })?;
        if port == 0 {
            return Err(IcooError::runtime(
                "SSE URL port must be between 1 and 65535",
                Some(span),
            ));
        }
        (host.to_string(), port)
    } else {
        (host_port.to_string(), 80)
    };
    Ok(ParsedHttpUrl { host, port, path })
}

fn split_http_authority_and_path(rest: &str) -> (&str, String) {
    let slash = rest.find('/');
    let query = rest.find('?');
    let split_at = match (slash, query) {
        (Some(slash), Some(query)) => slash.min(query),
        (Some(index), None) | (None, Some(index)) => index,
        (None, None) => return (rest, "/".to_string()),
    };
    let (host, path) = rest.split_at(split_at);
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };
    (host, path)
}

struct HttpResponse {
    status: i64,
    transfer_chunked: bool,
    body: Vec<u8>,
}

fn read_http_response(stream: &mut std::net::TcpStream, span: Span) -> IcooResult<HttpResponse> {
    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(|err| {
        IcooError::runtime(format!("sse client read failed: {}", err), Some(span))
    })?;
    let body_start = find_http_body_start(&response).ok_or_else(|| {
        IcooError::runtime(
            "invalid HTTP response: missing header terminator",
            Some(span),
        )
    })?;
    let head = String::from_utf8_lossy(&response[..body_start - 4]);
    let (status, transfer_chunked) = parse_response_head(&head, span)?;
    Ok(HttpResponse {
        status,
        transfer_chunked,
        body: response[body_start..].to_vec(),
    })
}

fn find_http_body_start(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn parse_response_head(head: &str, span: Span) -> IcooResult<(i64, bool)> {
    let mut lines = head.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| IcooError::runtime("invalid HTTP response: missing status", Some(span)))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| IcooError::runtime("invalid HTTP response status", Some(span)))?
        .parse::<i64>()
        .map_err(|_| IcooError::runtime("invalid HTTP response status", Some(span)))?;
    let transfer_chunked = lines.any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.trim().eq_ignore_ascii_case("transfer-encoding")
            && value.trim().eq_ignore_ascii_case("chunked")
    });
    Ok((status, transfer_chunked))
}

fn decode_chunked_bytes(bytes: &[u8], span: Span) -> IcooResult<Vec<u8>> {
    let mut index = 0;
    let mut decoded = Vec::new();
    loop {
        let Some(line_end) = find_crlf(bytes, index) else {
            return Err(IcooError::runtime(
                "invalid chunked response: missing chunk size",
                Some(span),
            ));
        };
        let size_text = std::str::from_utf8(&bytes[index..line_end])
            .map_err(|_| IcooError::runtime("invalid chunked response size", Some(span)))?;
        let size_hex = size_text.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| IcooError::runtime("invalid chunked response size", Some(span)))?;
        index = line_end + 2;
        if size == 0 {
            break;
        }
        if bytes.len() < index + size + 2 {
            return Err(IcooError::runtime(
                "invalid chunked response: incomplete chunk",
                Some(span),
            ));
        }
        decoded.extend_from_slice(&bytes[index..index + size]);
        index += size;
        if bytes.get(index..index + 2) != Some(b"\r\n") {
            return Err(IcooError::runtime(
                "invalid chunked response: missing chunk terminator",
                Some(span),
            ));
        }
        index += 2;
    }
    Ok(decoded)
}

fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|offset| start + offset)
}

#[derive(Default)]
struct SseEvent {
    data: Vec<String>,
    event: Option<String>,
    id: Option<String>,
    retry: Option<i64>,
    has_field: bool,
}

fn parse_sse_events(text: &str) -> Vec<SseEvent> {
    let mut events = Vec::new();
    let mut current = SseEvent::default();
    for raw_line in text.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() {
            push_event(&mut events, &mut current);
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        let (field, value) = if let Some((field, value)) = line.split_once(':') {
            (field, value.strip_prefix(' ').unwrap_or(value))
        } else {
            (line, "")
        };
        match field {
            "data" => {
                current.data.push(value.to_string());
                current.has_field = true;
            }
            "event" => {
                current.event = Some(value.to_string());
                current.has_field = true;
            }
            "id" => {
                current.id = Some(value.to_string());
                current.has_field = true;
            }
            "retry" => {
                if let Ok(retry) = value.parse::<i64>() {
                    current.retry = Some(retry);
                    current.has_field = true;
                }
            }
            _ => {}
        }
    }
    push_event(&mut events, &mut current);
    events
}

fn push_event(events: &mut Vec<SseEvent>, current: &mut SseEvent) {
    if current.has_field {
        events.push(std::mem::take(current));
    }
}

fn event_to_value(event: SseEvent) -> Value {
    let mut map = HashMap::new();
    map.insert("data".to_string(), Value::String(event.data.join("\n")));
    if let Some(event_name) = event.event {
        map.insert("event".to_string(), Value::String(event_name));
    }
    if let Some(id) = event.id {
        map.insert("id".to_string(), Value::String(id));
    }
    if let Some(retry) = event.retry {
        map.insert("retry".to_string(), Value::Int(retry));
    }
    Value::Map(Rc::new(RefCell::new(map)))
}

fn call_handler(
    runtime: &mut Interpreter,
    handler: &Value,
    event: Value,
    span: Span,
) -> IcooResult<()> {
    runtime
        .call_value(handler.clone(), vec![event], span)
        .map(|_| ())
}

fn result_value(status: i64, event_count: usize) -> Value {
    let mut map = HashMap::new();
    map.insert("status".to_string(), Value::Int(status));
    map.insert("events".to_string(), Value::Int(event_count as i64));
    Value::Map(Rc::new(RefCell::new(map)))
}
