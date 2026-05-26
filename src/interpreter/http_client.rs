use super::http_common::{ensure_http_header_name_value_no_crlf, find_http_body_start};
use super::Interpreter;
use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::runtime::permissions::RuntimePermissions;
use crate::runtime::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

const HTTP_CLIENT_MAX_RESPONSE_BODY_BYTES: usize = 16 * 1024 * 1024;
const HTTP_CLIENT_MAX_STREAM_CHUNK_BYTES: usize = 64 * 1024;

struct ParsedHttpUrl {
    host: String,
    port: u16,
    path: String,
}

struct ParsedHttpResponseHead {
    status: i64,
    headers: HashMap<String, Value>,
    body_prefix: Vec<u8>,
}

pub(crate) type HttpClientHeaders = Vec<(String, String)>;

fn parse_http_url(url: &str, span: Span) -> IcooResult<ParsedHttpUrl> {
    let Some(rest) = url.strip_prefix("http://") else {
        return Err(IcooError::runtime(
            "only http:// URLs are supported",
            Some(span),
        ));
    };
    let (host_port, path) = rest
        .split_once('/')
        .map(|(host, path)| (host, format!("/{}", path)))
        .unwrap_or((rest, "/".to_string()));
    if host_port.is_empty() {
        return Err(IcooError::runtime("URL host is required", Some(span)));
    }
    let (host, port) = if let Some((host, port)) = host_port.rsplit_once(':') {
        if host.is_empty() {
            return Err(IcooError::runtime("URL host is required", Some(span)));
        }
        let port = port
            .parse::<u16>()
            .map_err(|_| IcooError::runtime("URL port must be between 1 and 65535", Some(span)))?;
        (host.to_string(), port)
    } else {
        (host_port.to_string(), 80)
    };
    Ok(ParsedHttpUrl { host, port, path })
}

fn open_http_client_request(
    permissions: &RuntimePermissions,
    method: &str,
    url: &str,
    body: &[u8],
    content_type: Option<&str>,
    headers: &HttpClientHeaders,
    span: Span,
) -> IcooResult<std::net::TcpStream> {
    let parsed = parse_http_url(url, span)?;
    let custom_headers = http_client_headers_text(headers, span)?;
    permissions.check_net_connect_target(&parsed.host, parsed.port, span)?;
    let mut stream =
        std::net::TcpStream::connect((parsed.host.as_str(), parsed.port)).map_err(|err| {
            IcooError::runtime(
                format!("http client connection failed: {}", err),
                Some(span),
            )
        })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| IcooError::runtime(format!("http client failed: {}", err), Some(span)))?;
    if http_method_has_request_body(method) {
        let content_type = content_type.unwrap_or("application/octet-stream");
        let head = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: {}\r\n{}\r\n",
            method,
            parsed.path,
            parsed.host,
            body.len(),
            content_type,
            custom_headers,
        );
        std::io::Write::write_all(&mut stream, head.as_bytes()).map_err(|err| {
            IcooError::runtime(format!("http client write failed: {}", err), Some(span))
        })?;
        std::io::Write::write_all(&mut stream, body).map_err(|err| {
            IcooError::runtime(format!("http client write failed: {}", err), Some(span))
        })?;
    } else {
        let request = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n{}\r\n",
            method, parsed.path, parsed.host, custom_headers
        );
        std::io::Write::write_all(&mut stream, request.as_bytes()).map_err(|err| {
            IcooError::runtime(format!("http client write failed: {}", err), Some(span))
        })?;
    };
    Ok(stream)
}

fn http_client_headers_text(headers: &HttpClientHeaders, span: Span) -> IcooResult<String> {
    let mut text = String::new();
    for (name, value) in headers {
        ensure_http_header_name_value_no_crlf(name, value, span)?;
        text.push_str(name);
        text.push_str(": ");
        text.push_str(value);
        text.push_str("\r\n");
    }
    Ok(text)
}

fn http_method_has_request_body(method: &str) -> bool {
    matches!(method, "POST" | "PUT")
}

pub(crate) fn http_stream_method_name(method_name: &str) -> &'static str {
    match method_name {
        "stream_get" | "stream_get_bytes" => "GET",
        "stream_post" | "stream_post_bytes" => "POST",
        "stream_put" | "stream_put_bytes" => "PUT",
        "stream_delete" | "stream_delete_bytes" => "DELETE",
        "stream_options" | "stream_options_bytes" => "OPTIONS",
        _ => "GET",
    }
}

pub(crate) fn http_client_request(
    permissions: &RuntimePermissions,
    method: &str,
    url: &str,
    body: &str,
    headers: &HttpClientHeaders,
    span: Span,
) -> IcooResult<Value> {
    let mut stream = open_http_client_request(
        permissions,
        method,
        url,
        body.as_bytes(),
        Some("text/plain; charset=utf-8"),
        headers,
        span,
    )?;
    let response = read_http_response(&mut stream, span)?;
    Ok(http_client_response_value(
        response.status,
        response.headers,
        response.body,
        false,
    ))
}

pub(crate) fn http_client_request_bytes(
    permissions: &RuntimePermissions,
    method: &str,
    url: &str,
    body: &[u8],
    headers: &HttpClientHeaders,
    span: Span,
) -> IcooResult<Value> {
    let mut stream = open_http_client_request(
        permissions,
        method,
        url,
        body,
        Some("application/octet-stream"),
        headers,
        span,
    )?;
    let response = read_http_response(&mut stream, span)?;
    Ok(http_client_response_value(
        response.status,
        response.headers,
        response.body,
        true,
    ))
}

impl Interpreter {
    pub(crate) fn http_client_stream_request(
        &mut self,
        method: &str,
        url: &str,
        body: &str,
        headers: &HttpClientHeaders,
        handler: Value,
        span: Span,
    ) -> IcooResult<Value> {
        self.http_client_stream_request_inner(
            method,
            url,
            body.as_bytes(),
            Some("text/plain; charset=utf-8"),
            headers,
            handler,
            false,
            span,
        )
    }

    pub(crate) fn http_client_stream_request_bytes(
        &mut self,
        method: &str,
        url: &str,
        body: &[u8],
        headers: &HttpClientHeaders,
        handler: Value,
        span: Span,
    ) -> IcooResult<Value> {
        self.http_client_stream_request_inner(
            method,
            url,
            body,
            Some("application/octet-stream"),
            headers,
            handler,
            true,
            span,
        )
    }

    fn http_client_stream_request_inner(
        &mut self,
        method: &str,
        url: &str,
        body: &[u8],
        content_type: Option<&str>,
        headers: &HttpClientHeaders,
        handler: Value,
        bytes_chunks: bool,
        span: Span,
    ) -> IcooResult<Value> {
        let mut stream = open_http_client_request(
            &self.permissions,
            method,
            url,
            body,
            content_type,
            headers,
            span,
        )?;
        let response = read_http_response_head(&mut stream, span)?;
        let chunk_count = if http_headers_transfer_chunked(&response.headers) {
            http_client_stream_chunked(&mut stream, response.body_prefix, span, |chunk| {
                self.call_http_stream_handler(&handler, chunk, bytes_chunks, span)
            })?
        } else if let Some(content_length) = http_headers_content_length(&response.headers) {
            http_client_stream_content_length(
                &mut stream,
                response.body_prefix,
                content_length,
                span,
                |chunk| self.call_http_stream_handler(&handler, chunk, bytes_chunks, span),
            )?
        } else {
            http_client_stream_until_close(&mut stream, response.body_prefix, span, |chunk| {
                self.call_http_stream_handler(&handler, chunk, bytes_chunks, span)
            })?
        };
        Ok(http_client_stream_response_value(
            response.status,
            response.headers,
            chunk_count,
        ))
    }

    fn call_http_stream_handler(
        &mut self,
        handler: &Value,
        chunk: Vec<u8>,
        bytes_chunks: bool,
        span: Span,
    ) -> IcooResult<()> {
        let value = if bytes_chunks {
            Value::Bytes(Rc::new(chunk))
        } else {
            Value::String(String::from_utf8_lossy(&chunk).into_owned())
        };
        self.call_value(handler.clone(), vec![value], span)
            .map(|_| ())
    }
}

fn read_http_response_head(
    stream: &mut std::net::TcpStream,
    span: Span,
) -> IcooResult<ParsedHttpResponseHead> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let body_start = loop {
        let size = std::io::Read::read(stream, &mut buffer).map_err(|err| {
            IcooError::runtime(
                format!("http client stream read failed: {}", err),
                Some(span),
            )
        })?;
        if size == 0 {
            return Err(IcooError::runtime(
                "invalid HTTP response: missing header terminator",
                Some(span),
            ));
        }
        bytes.extend_from_slice(&buffer[..size]);
        if let Some(body_start) = find_http_body_start(&bytes) {
            break body_start;
        }
    };
    let head = String::from_utf8_lossy(&bytes[..body_start - 4]).into_owned();
    let (status, headers) = parse_http_response_head(&head, span)?;
    Ok(ParsedHttpResponseHead {
        status,
        headers,
        body_prefix: bytes[body_start..].to_vec(),
    })
}

struct ParsedHttpResponse {
    status: i64,
    headers: HashMap<String, Value>,
    body: Vec<u8>,
}

fn read_http_response(
    stream: &mut std::net::TcpStream,
    span: Span,
) -> IcooResult<ParsedHttpResponse> {
    let mut response = Vec::new();
    std::io::Read::read_to_end(stream, &mut response).map_err(|err| {
        IcooError::runtime(format!("http client read failed: {}", err), Some(span))
    })?;
    let body_start = find_http_body_start(&response).ok_or_else(|| {
        IcooError::runtime(
            "invalid HTTP response: missing header terminator",
            Some(span),
        )
    })?;
    let head = String::from_utf8_lossy(&response[..body_start - 4]).into_owned();
    let (status, headers) = parse_http_response_head(&head, span)?;
    let body = if http_headers_transfer_chunked(&headers) {
        decode_chunked_bytes(&response[body_start..], span)?
    } else {
        response[body_start..].to_vec()
    };
    ensure_http_client_body_size(body.len(), span)?;
    Ok(ParsedHttpResponse {
        status,
        headers,
        body,
    })
}

fn http_client_response_value(
    status: i64,
    headers: HashMap<String, Value>,
    body: Vec<u8>,
    bytes_body: bool,
) -> Value {
    let mut result = HashMap::new();
    result.insert("status".to_string(), Value::Int(status));
    let body = if bytes_body {
        Value::Bytes(Rc::new(body))
    } else {
        Value::String(String::from_utf8_lossy(&body).into_owned())
    };
    result.insert("body".to_string(), body);
    result.insert(
        "headers".to_string(),
        Value::Map(Rc::new(RefCell::new(headers))),
    );
    Value::Map(Rc::new(RefCell::new(result)))
}

fn parse_http_response_head(head: &str, span: Span) -> IcooResult<(i64, HashMap<String, Value>)> {
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
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(
                name.trim().to_ascii_lowercase(),
                Value::String(value.trim().to_string()),
            );
        }
    }
    Ok((status, headers))
}

fn http_headers_transfer_chunked(headers: &HashMap<String, Value>) -> bool {
    matches!(
        headers.get("transfer-encoding"),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("chunked")
    )
}

fn http_headers_content_length(headers: &HashMap<String, Value>) -> Option<usize> {
    let Some(Value::String(value)) = headers.get("content-length") else {
        return None;
    };
    value.parse::<usize>().ok()
}

fn http_client_stream_response_value(
    status: i64,
    headers: HashMap<String, Value>,
    chunk_count: usize,
) -> Value {
    let mut result = HashMap::new();
    result.insert("status".to_string(), Value::Int(status));
    result.insert("body".to_string(), Value::String(String::new()));
    result.insert(
        "headers".to_string(),
        Value::Map(Rc::new(RefCell::new(headers))),
    );
    result.insert("streamed".to_string(), Value::Bool(true));
    result.insert("chunks".to_string(), Value::Int(chunk_count as i64));
    Value::Map(Rc::new(RefCell::new(result)))
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
        let size = usize::from_str_radix(size_text.trim(), 16)
            .map_err(|_| IcooError::runtime("invalid chunked response size", Some(span)))?;
        index = line_end + 2;
        if size == 0 {
            break;
        }
        ensure_http_client_body_size(decoded.len() + size, span)?;
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

fn http_client_stream_chunked<F>(
    stream: &mut std::net::TcpStream,
    mut buffer: Vec<u8>,
    span: Span,
    mut on_chunk: F,
) -> IcooResult<usize>
where
    F: FnMut(Vec<u8>) -> IcooResult<()>,
{
    let mut chunk_count = 0;
    loop {
        while find_crlf(&buffer, 0).is_none() {
            read_more_http_stream_bytes(stream, &mut buffer, span)?;
        }
        let line_end = find_crlf(&buffer, 0).expect("checked above");
        let size_text = std::str::from_utf8(&buffer[..line_end])
            .map_err(|_| IcooError::runtime("invalid chunked response size", Some(span)))?;
        let size_hex = size_text.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| IcooError::runtime("invalid chunked response size", Some(span)))?;
        ensure_http_client_stream_chunk_size(size, span)?;
        buffer.drain(..line_end + 2);
        if size == 0 {
            break;
        }
        while buffer.len() < size + 2 {
            read_more_http_stream_bytes(stream, &mut buffer, span)?;
        }
        if buffer.get(size..size + 2) != Some(b"\r\n") {
            return Err(IcooError::runtime(
                "invalid chunked response: missing chunk terminator",
                Some(span),
            ));
        }
        let chunk = buffer[..size].to_vec();
        buffer.drain(..size + 2);
        on_chunk(chunk)?;
        chunk_count += 1;
    }
    Ok(chunk_count)
}

fn http_client_stream_content_length<F>(
    stream: &mut std::net::TcpStream,
    buffer: Vec<u8>,
    content_length: usize,
    span: Span,
    mut on_chunk: F,
) -> IcooResult<usize>
where
    F: FnMut(Vec<u8>) -> IcooResult<()>,
{
    let mut chunk_count = 0;
    let mut delivered = 0;
    ensure_http_client_body_size(content_length, span)?;
    if !buffer.is_empty() && content_length > 0 {
        let size = buffer.len().min(content_length);
        ensure_http_client_stream_chunk_size(size, span)?;
        on_chunk(buffer[..size].to_vec())?;
        delivered += size;
        chunk_count += 1;
    }
    let mut read_buffer = [0_u8; 4096];
    while delivered < content_length {
        let max_read = (content_length - delivered).min(read_buffer.len());
        let size = std::io::Read::read(stream, &mut read_buffer[..max_read]).map_err(|err| {
            IcooError::runtime(
                format!("http client stream read failed: {}", err),
                Some(span),
            )
        })?;
        if size == 0 {
            return Err(IcooError::runtime(
                "invalid HTTP response: incomplete body",
                Some(span),
            ));
        }
        ensure_http_client_stream_chunk_size(size, span)?;
        on_chunk(read_buffer[..size].to_vec())?;
        delivered += size;
        chunk_count += 1;
    }
    Ok(chunk_count)
}

fn http_client_stream_until_close<F>(
    stream: &mut std::net::TcpStream,
    buffer: Vec<u8>,
    span: Span,
    mut on_chunk: F,
) -> IcooResult<usize>
where
    F: FnMut(Vec<u8>) -> IcooResult<()>,
{
    let mut chunk_count = 0;
    if !buffer.is_empty() {
        ensure_http_client_stream_chunk_size(buffer.len(), span)?;
        on_chunk(buffer)?;
        chunk_count += 1;
    }
    let mut read_buffer = [0_u8; 4096];
    loop {
        let size = std::io::Read::read(stream, &mut read_buffer).map_err(|err| {
            IcooError::runtime(
                format!("http client stream read failed: {}", err),
                Some(span),
            )
        })?;
        if size == 0 {
            break;
        }
        ensure_http_client_stream_chunk_size(size, span)?;
        on_chunk(read_buffer[..size].to_vec())?;
        chunk_count += 1;
    }
    Ok(chunk_count)
}

fn read_more_http_stream_bytes(
    stream: &mut std::net::TcpStream,
    buffer: &mut Vec<u8>,
    span: Span,
) -> IcooResult<()> {
    let mut read_buffer = [0_u8; 4096];
    let size = std::io::Read::read(stream, &mut read_buffer).map_err(|err| {
        IcooError::runtime(
            format!("http client stream read failed: {}", err),
            Some(span),
        )
    })?;
    if size == 0 {
        return Err(IcooError::runtime(
            "invalid chunked response: incomplete chunk",
            Some(span),
        ));
    }
    buffer.extend_from_slice(&read_buffer[..size]);
    Ok(())
}

fn ensure_http_client_body_size(size: usize, span: Span) -> IcooResult<()> {
    if size > HTTP_CLIENT_MAX_RESPONSE_BODY_BYTES {
        Err(IcooError::runtime(
            format!(
                "http response body exceeds maximum size: {} bytes",
                HTTP_CLIENT_MAX_RESPONSE_BODY_BYTES
            ),
            Some(span),
        ))
    } else {
        Ok(())
    }
}

fn ensure_http_client_stream_chunk_size(size: usize, span: Span) -> IcooResult<()> {
    if size > HTTP_CLIENT_MAX_STREAM_CHUNK_BYTES {
        Err(IcooError::runtime(
            format!(
                "http response stream chunk exceeds maximum size: {} bytes",
                HTTP_CLIENT_MAX_STREAM_CHUNK_BYTES
            ),
            Some(span),
        ))
    } else {
        Ok(())
    }
}
