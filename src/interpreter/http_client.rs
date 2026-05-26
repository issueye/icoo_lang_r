use super::http_alpn::{negotiated_http_protocol, HttpAlpnPolicy, NegotiatedHttpProtocol};
use super::http_common::{ensure_http_header_name_value_no_crlf, find_http_body_start};
use super::http_proxy::{HttpProxyConfig as ProxyRequestConfig, HttpProxyTarget};
use super::http_redirect;
use super::http_url::{HttpScheme, ParsedHttpUrl};
use super::Interpreter;
use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::runtime::limits::{
    check_http_body_len, check_http_stream_chunk_len, MAX_HTTP_BODY_BYTES,
};
use crate::runtime::value::Value;
use rustls::pki_types::ServerName;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::rc::Rc;
use std::sync::Arc;

enum HttpClientStream {
    Plain(std::net::TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>>),
}

impl Read for HttpClientStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buf),
            Self::Tls(stream) => stream.read(buf),
        }
    }
}

impl Write for HttpClientStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.write(buf),
            Self::Tls(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

struct ParsedHttpResponseHead {
    status: i64,
    headers: HashMap<String, Value>,
    body_prefix: Vec<u8>,
}

pub(crate) type HttpClientHeaders = Vec<(String, String)>;

impl Interpreter {
    pub(crate) fn http_tls_client_config(
        &self,
        span: Span,
    ) -> IcooResult<Arc<rustls::ClientConfig>> {
        if let Some(config) = self.http_tls_config.borrow().clone() {
            return Ok(config);
        }
        let roots = match &self.http_tls_roots {
            Some(roots) => roots.as_ref().clone(),
            None => native_root_store(span)?,
        };
        let mut config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = HttpAlpnPolicy::default().rustls_protocols();
        let config = Arc::new(config);
        *self.http_tls_config.borrow_mut() = Some(config.clone());
        Ok(config)
    }
}

fn open_http_client_request(
    runtime: &Interpreter,
    method: &str,
    url: &str,
    body: &[u8],
    content_type: Option<&str>,
    headers: &HttpClientHeaders,
    span: Span,
) -> IcooResult<HttpClientStream> {
    let parsed = ParsedHttpUrl::parse(url, span)?;
    let custom_headers = http_client_headers_text(headers, span)?;
    runtime
        .permissions()
        .check_net_connect_endpoint(&parsed.host, parsed.port, span)?;
    let proxy = runtime_http_proxy_config(runtime, span)?;
    if let Some(proxy) = &proxy {
        runtime
            .permissions()
            .check_net_connect_endpoint(proxy.host(), proxy.port(), span)?;
    }
    let request_target = http_request_target(&parsed, proxy.as_ref(), span)?;
    let proxy_authorization = if parsed.scheme == HttpScheme::Http {
        proxy
            .as_ref()
            .and_then(|proxy| proxy.authorization())
            .map(str::to_string)
    } else {
        None
    };
    let mut stream = open_http_client_stream(runtime, &parsed, proxy.as_ref(), span)?;
    write_http_request(
        &mut stream,
        &parsed,
        &request_target,
        method,
        body,
        content_type,
        &custom_headers,
        proxy_authorization.as_deref(),
        span,
    )?;
    Ok(stream)
}

fn open_http_client_stream(
    runtime: &Interpreter,
    parsed: &ParsedHttpUrl,
    proxy: Option<&ProxyRequestConfig>,
    span: Span,
) -> IcooResult<HttpClientStream> {
    let mut tcp = if let Some(proxy) = proxy {
        connect_http_tcp(runtime, proxy.host(), proxy.port(), span)?
    } else {
        connect_http_tcp_authority(runtime, &parsed.connect_host(), span)?
    };

    if let Some(proxy) = proxy {
        if parsed.scheme == HttpScheme::Https {
            establish_https_proxy_tunnel(&mut tcp, parsed, proxy, span)?;
        }
    }

    if parsed.scheme == HttpScheme::Http {
        return Ok(HttpClientStream::Plain(tcp));
    }

    let config = runtime.http_tls_client_config(span)?;
    let server_name = ServerName::try_from(parsed.host.clone())
        .map_err(|_| IcooError::runtime("invalid HTTPS server name", Some(span)))?;
    let mut connection = rustls::ClientConnection::new(config, server_name).map_err(|err| {
        IcooError::runtime(
            format!("https client TLS handshake failed: {}", err),
            Some(span),
        )
    })?;
    connection.complete_io(&mut tcp).map_err(|err| {
        IcooError::runtime(
            format!("https client TLS handshake failed: {}", err),
            Some(span),
        )
    })?;
    match negotiated_http_protocol(connection.alpn_protocol()) {
        NegotiatedHttpProtocol::Http11 => {}
        NegotiatedHttpProtocol::H2 => {
            return Err(IcooError::runtime(
                "https client negotiated HTTP/2 via ALPN, but HTTP/2 is not supported by this blocking client yet",
                Some(span),
            ))
        }
        NegotiatedHttpProtocol::Unknown => {
            return Err(IcooError::runtime(
                "https client negotiated unsupported ALPN protocol",
                Some(span),
            ))
        }
    }
    Ok(HttpClientStream::Tls(Box::new(rustls::StreamOwned::new(
        connection, tcp,
    ))))
}

fn connect_http_tcp(
    runtime: &Interpreter,
    host: &str,
    port: u16,
    span: Span,
) -> IcooResult<TcpStream> {
    let authority = socket_authority(host, port);
    connect_http_tcp_authority(runtime, &authority, span)
}

fn connect_http_tcp_authority(
    runtime: &Interpreter,
    authority: &str,
    span: Span,
) -> IcooResult<TcpStream> {
    let addrs = authority.to_socket_addrs().map_err(|err| {
        IcooError::runtime(
            format!("http client connection failed: {}", err),
            Some(span),
        )
    })?;
    let mut last_error = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, runtime.http_config().connect_timeout) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(runtime.http_config().read_timeout))
                    .and_then(|_| {
                        stream.set_write_timeout(Some(runtime.http_config().write_timeout))
                    })
                    .map_err(|err| {
                        IcooError::runtime(format!("http client failed: {}", err), Some(span))
                    })?;
                return Ok(stream);
            }
            Err(err) => last_error = Some(err),
        }
    }
    let message = last_error
        .map(|err| err.to_string())
        .unwrap_or_else(|| "no socket address resolved".to_string());
    Err(IcooError::runtime(
        format!("http client connection failed: {}", message),
        Some(span),
    ))
}

fn socket_authority(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}

fn runtime_http_proxy_config(
    runtime: &Interpreter,
    span: Span,
) -> IcooResult<Option<ProxyRequestConfig>> {
    runtime
        .http_config()
        .proxy
        .as_ref()
        .map(|proxy| {
            ProxyRequestConfig::new(proxy.host.clone(), proxy.port, proxy.authorization.clone())
                .map_err(|err| IcooError::runtime(err, Some(span)))
        })
        .transpose()
}

fn http_request_target(
    parsed: &ParsedHttpUrl,
    proxy: Option<&ProxyRequestConfig>,
    span: Span,
) -> IcooResult<String> {
    if parsed.scheme == HttpScheme::Http && proxy.is_some() {
        let target = HttpProxyTarget::new(parsed.host.clone(), parsed.port)
            .map_err(|err| IcooError::runtime(err, Some(span)))?;
        Ok(target.absolute_form_http_target(&parsed.path))
    } else {
        Ok(parsed.path.clone())
    }
}

fn establish_https_proxy_tunnel(
    tcp: &mut TcpStream,
    parsed: &ParsedHttpUrl,
    proxy: &ProxyRequestConfig,
    span: Span,
) -> IcooResult<()> {
    let target = HttpProxyTarget::new(parsed.host.clone(), parsed.port)
        .map_err(|err| IcooError::runtime(err, Some(span)))?;
    let request = target.connect_request(proxy);
    tcp.write_all(request.as_bytes()).map_err(|err| {
        IcooError::runtime(
            format!("http proxy CONNECT write failed: {}", err),
            Some(span),
        )
    })?;
    tcp.flush().map_err(|err| {
        IcooError::runtime(
            format!("http proxy CONNECT write failed: {}", err),
            Some(span),
        )
    })?;
    let status = read_proxy_connect_status(tcp, span)?;
    if !(200..300).contains(&status) {
        return Err(IcooError::runtime(
            format!("http proxy CONNECT failed with status {}", status),
            Some(span),
        ));
    }
    Ok(())
}

fn read_proxy_connect_status(tcp: &mut TcpStream, span: Span) -> IcooResult<i64> {
    let mut response = Vec::new();
    let mut buffer = [0_u8; 512];
    loop {
        let size = tcp.read(&mut buffer).map_err(|err| {
            IcooError::runtime(
                format!("http proxy CONNECT read failed: {}", err),
                Some(span),
            )
        })?;
        if size == 0 {
            return Err(IcooError::runtime(
                "invalid HTTP proxy CONNECT response: missing header terminator",
                Some(span),
            ));
        }
        response.extend_from_slice(&buffer[..size]);
        if find_http_body_start(&response).is_some() {
            break;
        }
    }
    let head_end = find_http_body_start(&response).expect("checked above") - 4;
    let head = String::from_utf8_lossy(&response[..head_end]);
    let status_line = head.lines().next().ok_or_else(|| {
        IcooError::runtime(
            "invalid HTTP proxy CONNECT response: missing status",
            Some(span),
        )
    })?;
    status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| {
            IcooError::runtime("invalid HTTP proxy CONNECT response status", Some(span))
        })?
        .parse::<i64>()
        .map_err(|_| IcooError::runtime("invalid HTTP proxy CONNECT response status", Some(span)))
}

fn native_root_store(span: Span) -> IcooResult<rustls::RootCertStore> {
    let loaded = rustls_native_certs::load_native_certs();
    if loaded.certs.is_empty() {
        let reason = if loaded.errors.is_empty() {
            "no native certificate roots found".to_string()
        } else {
            loaded
                .errors
                .iter()
                .map(|err| err.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        };
        return Err(IcooError::runtime(
            format!(
                "https client failed to load native certificate roots: {}",
                reason
            ),
            Some(span),
        ));
    }
    let mut roots = rustls::RootCertStore::empty();
    let (added, _ignored) = roots.add_parsable_certificates(loaded.certs);
    if added == 0 {
        return Err(IcooError::runtime(
            "https client failed to load native certificate roots: no usable native certificate roots found",
            Some(span),
        ));
    }
    Ok(roots)
}

fn write_http_request(
    stream: &mut HttpClientStream,
    parsed: &ParsedHttpUrl,
    request_target: &str,
    method: &str,
    body: &[u8],
    content_type: Option<&str>,
    custom_headers: &str,
    proxy_authorization: Option<&str>,
    span: Span,
) -> IcooResult<()> {
    let host = parsed.host_header();
    let proxy_authorization = proxy_authorization_header(proxy_authorization, span)?;
    if http_method_has_request_body(method) {
        let content_type = content_type.unwrap_or("application/octet-stream");
        let head = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: {}\r\n{}{}\r\n",
            method,
            request_target,
            host,
            body.len(),
            content_type,
            custom_headers,
            proxy_authorization,
        );
        stream.write_all(head.as_bytes()).map_err(|err| {
            IcooError::runtime(format!("http client write failed: {}", err), Some(span))
        })?;
        stream.write_all(body).map_err(|err| {
            IcooError::runtime(format!("http client write failed: {}", err), Some(span))
        })?;
    } else {
        let request = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n{}{}\r\n",
            method, request_target, host, custom_headers, proxy_authorization
        );
        stream.write_all(request.as_bytes()).map_err(|err| {
            IcooError::runtime(format!("http client write failed: {}", err), Some(span))
        })?;
    };
    stream
        .flush()
        .map_err(|err| IcooError::runtime(format!("http client write failed: {}", err), Some(span)))
}

fn proxy_authorization_header(authorization: Option<&str>, span: Span) -> IcooResult<String> {
    let Some(authorization) = authorization else {
        return Ok(String::new());
    };
    ensure_http_header_name_value_no_crlf("Proxy-Authorization", authorization, span)?;
    Ok(format!("Proxy-Authorization: {}\r\n", authorization))
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
        "stream_delete" => "DELETE",
        "stream_options" => "OPTIONS",
        _ => "GET",
    }
}

pub(crate) fn http_client_request(
    runtime: &Interpreter,
    method: &str,
    url: &str,
    body: &str,
    headers: &HttpClientHeaders,
    span: Span,
) -> IcooResult<Value> {
    http_client_request_raw(
        runtime,
        method,
        url,
        body.as_bytes(),
        Some("text/plain; charset=utf-8"),
        headers,
        false,
        span,
    )
}

pub(crate) fn http_client_request_bytes(
    runtime: &Interpreter,
    method: &str,
    url: &str,
    body: &[u8],
    headers: &HttpClientHeaders,
    span: Span,
) -> IcooResult<Value> {
    http_client_request_raw(
        runtime,
        method,
        url,
        body,
        Some("application/octet-stream"),
        headers,
        true,
        span,
    )
}

fn http_client_request_raw(
    runtime: &Interpreter,
    method: &str,
    url: &str,
    body: &[u8],
    content_type: Option<&str>,
    headers: &HttpClientHeaders,
    bytes_body: bool,
    span: Span,
) -> IcooResult<Value> {
    let mut current_method = method.to_string();
    let mut current_url = url.to_string();
    let mut current_body = body.to_vec();
    let mut current_content_type = content_type.map(str::to_string);
    let mut redirect_count = 0;

    loop {
        let mut stream = open_http_client_request(
            runtime,
            &current_method,
            &current_url,
            &current_body,
            current_content_type.as_deref(),
            headers,
            span,
        )?;
        let response = read_http_response(&mut stream, span)?;
        let max_redirects = runtime.http_config().max_redirects;
        if max_redirects == 0 || !http_redirect::is_redirect_status(response.status) {
            return Ok(http_client_response_value(
                response.status,
                response.headers,
                response.body,
                bytes_body,
            ));
        }

        let redirect_headers = http_redirect_headers(&response.headers);
        let redirect = http_redirect::redirect_request(
            response.status,
            &redirect_headers,
            &current_url,
            &current_method,
            &current_body,
            redirect_count,
            max_redirects,
        )
        .map_err(|err| {
            IcooError::runtime(format!("http client redirect failed: {}", err), Some(span))
        })?;
        let Some(redirect) = redirect else {
            return Ok(http_client_response_value(
                response.status,
                response.headers,
                response.body,
                bytes_body,
            ));
        };
        redirect_count += 1;
        current_url = redirect.url;
        current_method = redirect.method;
        current_body = redirect.body;
        if !http_method_has_request_body(&current_method) {
            current_content_type = None;
        }
    }
}

fn http_redirect_headers(headers: &HashMap<String, Value>) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| match value {
            Value::String(value) => Some((name.clone(), value.clone())),
            _ => None,
        })
        .collect()
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
        let mut stream = open_http_client_request(
            self,
            method,
            url,
            body.as_bytes(),
            Some("text/plain; charset=utf-8"),
            headers,
            span,
        )?;
        let response = read_http_response_head(&mut stream, span)?;
        let chunk_count = if http_headers_transfer_chunked(&response.headers) {
            http_client_stream_chunked(&mut stream, response.body_prefix, span, |chunk| {
                self.call_http_stream_handler(&handler, chunk, span)
            })?
        } else if let Some(content_length) = http_headers_content_length(&response.headers) {
            http_client_stream_content_length(
                &mut stream,
                response.body_prefix,
                content_length,
                span,
                |chunk| self.call_http_stream_handler(&handler, chunk, span),
            )?
        } else {
            http_client_stream_until_close(&mut stream, response.body_prefix, span, |chunk| {
                self.call_http_stream_handler(&handler, chunk, span)
            })?
        };
        Ok(http_client_stream_response_value(
            response.status,
            response.headers,
            chunk_count,
        ))
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
        let mut stream = open_http_client_request(
            self,
            method,
            url,
            body,
            Some("application/octet-stream"),
            headers,
            span,
        )?;
        let response = read_http_response_head(&mut stream, span)?;
        let chunk_count = if http_headers_transfer_chunked(&response.headers) {
            http_client_stream_chunked(&mut stream, response.body_prefix, span, |chunk| {
                self.call_http_stream_bytes_handler(&handler, chunk, span)
            })?
        } else if let Some(content_length) = http_headers_content_length(&response.headers) {
            http_client_stream_content_length(
                &mut stream,
                response.body_prefix,
                content_length,
                span,
                |chunk| self.call_http_stream_bytes_handler(&handler, chunk, span),
            )?
        } else {
            http_client_stream_until_close(&mut stream, response.body_prefix, span, |chunk| {
                self.call_http_stream_bytes_handler(&handler, chunk, span)
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
        span: Span,
    ) -> IcooResult<()> {
        let text = String::from_utf8_lossy(&chunk).into_owned();
        self.call_value(handler.clone(), vec![Value::String(text)], span)
            .map(|_| ())
    }

    fn call_http_stream_bytes_handler(
        &mut self,
        handler: &Value,
        chunk: Vec<u8>,
        span: Span,
    ) -> IcooResult<()> {
        self.call_value(handler.clone(), vec![Value::Bytes(Rc::new(chunk))], span)
            .map(|_| ())
    }
}

fn read_http_response_head(
    stream: &mut HttpClientStream,
    span: Span,
) -> IcooResult<ParsedHttpResponseHead> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let body_start = loop {
        let size = stream.read(&mut buffer).map_err(|err| {
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

fn read_http_response(stream: &mut HttpClientStream, span: Span) -> IcooResult<ParsedHttpResponse> {
    let mut response = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let size = match stream.read(&mut buffer) {
            Ok(size) => size,
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(err) => {
                return Err(IcooError::runtime(
                    format!("http client read failed: {}", err),
                    Some(span),
                ))
            }
        };
        if size == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..size]);
        if let Some(body_start) = find_http_body_start(&response) {
            check_http_body_len(response.len().saturating_sub(body_start), span)?;
        }
    }
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
        let body = response[body_start..].to_vec();
        check_http_body_len(body.len(), span)?;
        body
    };
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
        if bytes.len() < index + size + 2 {
            return Err(IcooError::runtime(
                "invalid chunked response: incomplete chunk",
                Some(span),
            ));
        }
        let total = decoded.len().checked_add(size).ok_or_else(|| {
            IcooError::runtime(
                "http response body exceeds maximum size: overflow",
                Some(span),
            )
        })?;
        check_http_body_len(total, span)?;
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
    stream: &mut HttpClientStream,
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
        check_http_stream_chunk_len(chunk.len(), span)?;
        buffer.drain(..size + 2);
        on_chunk(chunk)?;
        chunk_count += 1;
    }
    Ok(chunk_count)
}

fn http_client_stream_content_length<F>(
    stream: &mut HttpClientStream,
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
    if !buffer.is_empty() && content_length > 0 {
        let size = buffer.len().min(content_length);
        check_http_stream_chunk_len(size, span)?;
        on_chunk(buffer[..size].to_vec())?;
        delivered += size;
        chunk_count += 1;
    }
    let mut read_buffer = [0_u8; 4096];
    while delivered < content_length {
        let max_read = (content_length - delivered).min(read_buffer.len());
        let size = stream.read(&mut read_buffer[..max_read]).map_err(|err| {
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
        check_http_stream_chunk_len(size, span)?;
        on_chunk(read_buffer[..size].to_vec())?;
        delivered += size;
        chunk_count += 1;
    }
    Ok(chunk_count)
}

fn http_client_stream_until_close<F>(
    stream: &mut HttpClientStream,
    buffer: Vec<u8>,
    span: Span,
    mut on_chunk: F,
) -> IcooResult<usize>
where
    F: FnMut(Vec<u8>) -> IcooResult<()>,
{
    let mut chunk_count = 0;
    if !buffer.is_empty() {
        check_http_stream_chunk_len(buffer.len(), span)?;
        on_chunk(buffer)?;
        chunk_count += 1;
    }
    let mut read_buffer = [0_u8; 4096];
    loop {
        let size = match stream.read(&mut read_buffer) {
            Ok(size) => size,
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(err) => {
                return Err(IcooError::runtime(
                    format!("http client stream read failed: {}", err),
                    Some(span),
                ))
            }
        };
        if size == 0 {
            break;
        }
        check_http_stream_chunk_len(size, span)?;
        on_chunk(read_buffer[..size].to_vec())?;
        chunk_count += 1;
    }
    Ok(chunk_count)
}

fn read_more_http_stream_bytes(
    stream: &mut HttpClientStream,
    buffer: &mut Vec<u8>,
    span: Span,
) -> IcooResult<()> {
    let mut read_buffer = [0_u8; 4096];
    let size = stream.read(&mut read_buffer).map_err(|err| {
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
    if buffer.len() > MAX_HTTP_BODY_BYTES + 4096 {
        return Err(IcooError::runtime(
            format!(
                "http response body exceeds maximum size: buffered stream data exceeded {} bytes",
                MAX_HTTP_BODY_BYTES
            ),
            Some(span),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::logging::RuntimeLogger;
    use crate::runtime::permissions::RuntimePermissions;

    #[test]
    fn interpreter_reuses_cached_http_tls_client_config() {
        let interpreter = Interpreter::with_output_permissions_logger_and_tls_roots(
            |_| {},
            RuntimePermissions::allow_all(),
            RuntimeLogger::default(),
            Some(Arc::new(rustls::RootCertStore::empty())),
        );
        let span = Span::new(1, 1, 0, 1);

        let first = interpreter.http_tls_client_config(span).unwrap();
        let second = interpreter.http_tls_client_config(span).unwrap();

        assert!(Arc::ptr_eq(&first, &second));
    }
}
