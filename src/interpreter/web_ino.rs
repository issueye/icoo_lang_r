use super::http_common::{find_http_body_start, http_content_length, http_status_text};
use super::{
    expect_arity, expect_bytes, expect_int, expect_string, is_callable, value_to_json, Interpreter,
};
use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::runtime::value::{Value, WebInoApp, WebInoResponse, WebInoRoute};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

#[derive(Debug)]
struct ParsedWebInoRequest {
    method: String,
    path: String,
    query: String,
    query_params: HashMap<String, Value>,
    params: HashMap<String, Value>,
    headers: HashMap<String, Value>,
    body: String,
    body_bytes: Vec<u8>,
    form: HashMap<String, Value>,
    files: HashMap<String, Value>,
}

enum WebInoAccepted {
    Request {
        request: Result<Vec<u8>, String>,
        stream: std::net::TcpStream,
    },
    AcceptError(String),
}

impl Interpreter {
    pub(super) fn web_ino_app_method(
        &mut self,
        app: Rc<RefCell<WebInoApp>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "get" | "post" | "put" | "delete" | "options" => {
                expect_arity(&args, 2, span)?;
                let path = expect_string(&args[0], span)?;
                if !path.starts_with('/') {
                    return Err(IcooError::runtime(
                        "route path must start with '/'",
                        Some(span),
                    ));
                }
                if !is_callable(&args[1]) {
                    return Err(IcooError::runtime(
                        "route handler must be callable",
                        Some(span),
                    ));
                }
                let method = name.to_ascii_uppercase();
                app.borrow_mut().routes.insert(
                    web_ino_route_key(&method, &path),
                    WebInoRoute {
                        method,
                        parameterized: web_ino_route_is_parameterized(&path),
                        path,
                        handler: args[1].clone(),
                    },
                );
                Ok(Value::WebInoApp(app))
            }
            "listen_once" => {
                expect_arity(&args, 2, span)?;
                let host = expect_string(&args[0], span)?;
                let port = expect_int(&args[1], span)?;
                if !(1..=65535).contains(&port) {
                    return Err(IcooError::runtime(
                        "server port must be between 1 and 65535",
                        Some(span),
                    ));
                }
                self.web_ino_listen_once(app, &host, port as u16, span)?;
                Ok(Value::Nil)
            }
            "listen" => {
                expect_arity(&args, 3, span)?;
                let host = expect_string(&args[0], span)?;
                let port = expect_int(&args[1], span)?;
                let max_requests = expect_int(&args[2], span)?;
                if !(1..=65535).contains(&port) {
                    return Err(IcooError::runtime(
                        "server port must be between 1 and 65535",
                        Some(span),
                    ));
                }
                if max_requests <= 0 {
                    return Err(IcooError::runtime(
                        "max_requests must be positive",
                        Some(span),
                    ));
                }
                let workers = std::thread::available_parallelism()
                    .map(|count| count.get())
                    .unwrap_or(1);
                self.web_ino_listen(
                    app,
                    &host,
                    port as u16,
                    max_requests as usize,
                    workers,
                    span,
                )?;
                Ok(Value::Nil)
            }
            "listen_with_workers" => {
                expect_arity(&args, 4, span)?;
                let host = expect_string(&args[0], span)?;
                let port = expect_int(&args[1], span)?;
                let max_requests = expect_int(&args[2], span)?;
                let workers = expect_int(&args[3], span)?;
                if !(1..=65535).contains(&port) {
                    return Err(IcooError::runtime(
                        "server port must be between 1 and 65535",
                        Some(span),
                    ));
                }
                if max_requests <= 0 {
                    return Err(IcooError::runtime(
                        "max_requests must be positive",
                        Some(span),
                    ));
                }
                if workers <= 0 {
                    return Err(IcooError::runtime("workers must be positive", Some(span)));
                }
                self.web_ino_listen(
                    app,
                    &host,
                    port as u16,
                    max_requests as usize,
                    workers as usize,
                    span,
                )?;
                Ok(Value::Nil)
            }
            _ => Err(IcooError::runtime("unknown WebInoApp method", Some(span))),
        }
    }

    pub(super) fn web_ino_response_method(
        &mut self,
        response: Rc<RefCell<WebInoResponse>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "status" => {
                expect_arity(&args, 1, span)?;
                let status = expect_int(&args[0], span)?;
                if !(100..=999).contains(&status) {
                    return Err(IcooError::runtime(
                        "HTTP status must be between 100 and 999",
                        Some(span),
                    ));
                }
                {
                    let mut response_ref = response.borrow_mut();
                    if response_ref.headers_sent {
                        return Err(IcooError::runtime(
                            "cannot change HTTP status after streaming has started",
                            Some(span),
                        ));
                    }
                    response_ref.status = status;
                }
                Ok(Value::WebInoResponse(response))
            }
            "header" => {
                expect_arity(&args, 2, span)?;
                let header_name = expect_string(&args[0], span)?;
                let header_value = expect_string(&args[1], span)?;
                web_ino_validate_header_name(&header_name, span)?;
                web_ino_validate_header_value(&header_value, span)?;
                {
                    let mut response_ref = response.borrow_mut();
                    if response_ref.headers_sent {
                        return Err(IcooError::runtime(
                            "cannot change HTTP headers after streaming has started",
                            Some(span),
                        ));
                    }
                    response_ref.headers.insert(header_name, header_value);
                }
                Ok(Value::WebInoResponse(response))
            }
            "content_type" => {
                expect_arity(&args, 1, span)?;
                let content_type = expect_string(&args[0], span)?;
                web_ino_validate_header_value(&content_type, span)?;
                {
                    let mut response_ref = response.borrow_mut();
                    if response_ref.headers_sent {
                        return Err(IcooError::runtime(
                            "cannot change HTTP content type after streaming has started",
                            Some(span),
                        ));
                    }
                    response_ref.content_type = content_type;
                    response_ref.content_type_overridden = true;
                }
                Ok(Value::WebInoResponse(response))
            }
            "send" => {
                expect_arity(&args, 1, span)?;
                let mut response_ref = response.borrow_mut();
                if response_ref.headers_sent {
                    return Err(IcooError::runtime(
                        "cannot send response after streaming has started",
                        Some(span),
                    ));
                }
                response_ref.body = args[0].display();
                response_ref.body_bytes = None;
                response_ref.chunks.clear();
                response_ref.headers.remove("Content-Disposition");
                if !response_ref.content_type_overridden {
                    response_ref.content_type = "text/plain; charset=utf-8".to_string();
                }
                response_ref.sent = true;
                response_ref.streaming = false;
                Ok(Value::Nil)
            }
            "send_bytes" => {
                if !(1..=2).contains(&args.len()) {
                    return Err(IcooError::runtime(
                        "send_bytes() expects 1..2 arguments",
                        Some(span),
                    ));
                }
                let bytes = expect_bytes(&args[0], span)?;
                let content_type = if args.len() == 2 {
                    Some(expect_string(&args[1], span)?)
                } else {
                    None
                };
                let mut response_ref = response.borrow_mut();
                if response_ref.headers_sent {
                    return Err(IcooError::runtime(
                        "cannot send bytes response after streaming has started",
                        Some(span),
                    ));
                }
                response_ref.body = String::new();
                response_ref.body_bytes = Some(bytes.as_ref().clone());
                response_ref.chunks.clear();
                response_ref.headers.remove("Content-Disposition");
                if let Some(content_type) = content_type {
                    web_ino_validate_header_value(&content_type, span)?;
                    response_ref.content_type = content_type;
                    response_ref.content_type_overridden = true;
                } else if !response_ref.content_type_overridden {
                    response_ref.content_type = "application/octet-stream".to_string();
                }
                response_ref.sent = true;
                response_ref.streaming = false;
                Ok(Value::Nil)
            }
            "json" => {
                expect_arity(&args, 1, span)?;
                let body =
                    serde_json::to_string(&value_to_json(&args[0], span)?).map_err(|err| {
                        IcooError::runtime(
                            format!("web response json() failed: {}", err),
                            Some(span),
                        )
                    })?;
                let mut response_ref = response.borrow_mut();
                if response_ref.headers_sent {
                    return Err(IcooError::runtime(
                        "cannot send JSON response after streaming has started",
                        Some(span),
                    ));
                }
                response_ref.body = body;
                response_ref.body_bytes = None;
                response_ref.chunks.clear();
                response_ref.headers.remove("Content-Disposition");
                if !response_ref.content_type_overridden {
                    response_ref.content_type = "application/json; charset=utf-8".to_string();
                }
                response_ref.sent = true;
                response_ref.streaming = false;
                Ok(Value::Nil)
            }
            "download" => {
                if !(1..=2).contains(&args.len()) {
                    return Err(IcooError::runtime(
                        "download() expects 1..2 arguments",
                        Some(span),
                    ));
                }
                let path = expect_string(&args[0], span)?;
                let filename = if args.len() == 2 {
                    expect_string(&args[1], span)?
                } else {
                    web_ino_download_filename(&path)
                };
                self.permissions().check_fs_read(span)?;
                let bytes = std::fs::read(&path).map_err(|err| {
                    IcooError::runtime(
                        format!("web response download() failed: {}", err),
                        Some(span),
                    )
                })?;
                let mut response_ref = response.borrow_mut();
                if response_ref.headers_sent {
                    return Err(IcooError::runtime(
                        "cannot download file after streaming has started",
                        Some(span),
                    ));
                }
                response_ref.body = String::new();
                response_ref.body_bytes = Some(bytes);
                response_ref.chunks.clear();
                if !response_ref.content_type_overridden {
                    response_ref.content_type = web_ino_download_content_type(&path).to_string();
                }
                response_ref.headers.insert(
                    "Content-Disposition".to_string(),
                    format!(
                        "attachment; filename=\"{}\"",
                        web_ino_escape_header_value(&filename)
                    ),
                );
                response_ref.sent = true;
                response_ref.streaming = false;
                Ok(Value::Nil)
            }
            "write" => {
                expect_arity(&args, 1, span)?;
                {
                    let mut response_ref = response.borrow_mut();
                    let chunk = args[0].display();
                    web_ino_write_stream_chunk(&mut response_ref, chunk.as_bytes(), span)?;
                }
                Ok(Value::WebInoResponse(response))
            }
            "write_bytes" => {
                expect_arity(&args, 1, span)?;
                let bytes = expect_bytes(&args[0], span)?;
                {
                    let mut response_ref = response.borrow_mut();
                    web_ino_write_stream_chunk(&mut response_ref, bytes.as_slice(), span)?;
                }
                Ok(Value::WebInoResponse(response))
            }
            "end" => {
                expect_arity(&args, 0, span)?;
                let mut response_ref = response.borrow_mut();
                if response_ref.streaming {
                    web_ino_end_stream(&mut response_ref, span)?;
                } else {
                    response_ref.sent = true;
                }
                Ok(Value::Nil)
            }
            _ => Err(IcooError::runtime(
                "unknown WebInoResponse method",
                Some(span),
            )),
        }
    }

    fn web_ino_listen_once(
        &mut self,
        app: Rc<RefCell<WebInoApp>>,
        host: &str,
        port: u16,
        span: Span,
    ) -> IcooResult<()> {
        self.permissions().check_net_listen(span)?;
        let listener = std::net::TcpListener::bind((host, port)).map_err(|err| {
            IcooError::runtime(
                format!("web.ino listen_once bind failed: {}", err),
                Some(span),
            )
        })?;
        let (mut stream, _) = listener.accept().map_err(|err| {
            IcooError::runtime(
                format!("web.ino listen_once accept failed: {}", err),
                Some(span),
            )
        })?;
        let request = read_web_ino_request_bytes(&mut stream)
            .map_err(|message| IcooError::runtime(message, Some(span)))?;
        self.web_ino_handle_request(app, &request, &mut stream, span)
    }

    fn web_ino_listen(
        &mut self,
        app: Rc<RefCell<WebInoApp>>,
        host: &str,
        port: u16,
        max_requests: usize,
        workers: usize,
        span: Span,
    ) -> IcooResult<()> {
        self.permissions().check_net_listen(span)?;
        let listener = std::net::TcpListener::bind((host, port)).map_err(|err| {
            IcooError::runtime(format!("web.ino listen bind failed: {}", err), Some(span))
        })?;
        let workers = workers.max(1).min(max_requests);
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let (stream_tx, stream_rx) = std::sync::mpsc::channel();
        let stream_rx = std::sync::Arc::new(std::sync::Mutex::new(stream_rx));
        let accept_result_tx = result_tx.clone();
        let accept_handle = std::thread::spawn(move || {
            for _ in 0..max_requests {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if stream_tx.send(stream).is_err() {
                            let _ = accept_result_tx.send(WebInoAccepted::AcceptError(
                                "web.ino listen worker queue closed".to_string(),
                            ));
                            break;
                        }
                    }
                    Err(err) => {
                        let _ = accept_result_tx.send(WebInoAccepted::AcceptError(format!(
                            "web.ino listen accept failed: {}",
                            err
                        )));
                        break;
                    }
                }
            }
        });
        let mut worker_handles = Vec::new();
        for _ in 0..workers {
            let stream_rx = stream_rx.clone();
            let result_tx = result_tx.clone();
            worker_handles.push(std::thread::spawn(move || loop {
                let stream = {
                    let stream_rx = stream_rx.lock().expect("web.ino worker queue poisoned");
                    stream_rx.recv()
                };
                let Ok(mut stream) = stream else {
                    break;
                };
                let request = stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .map_err(|err| format!("web.ino request read failed: {}", err))
                    .and_then(|_| read_web_ino_request_bytes(&mut stream));
                let _ = result_tx.send(WebInoAccepted::Request { request, stream });
            }));
        }
        drop(result_tx);

        for _ in 0..max_requests {
            match result_rx.recv().map_err(|err| {
                IcooError::runtime(format!("web.ino listen failed: {}", err), Some(span))
            })? {
                WebInoAccepted::Request {
                    request: Ok(request),
                    mut stream,
                } => self.web_ino_handle_request(app.clone(), &request, &mut stream, span)?,
                WebInoAccepted::Request {
                    request: Err(message),
                    mut stream,
                } => {
                    let response = WebInoResponse {
                        status: 400,
                        body: message,
                        body_bytes: None,
                        chunks: Vec::new(),
                        content_type: "text/plain; charset=utf-8".to_string(),
                        content_type_overridden: false,
                        headers: HashMap::new(),
                        sent: true,
                        streaming: false,
                        headers_sent: false,
                        stream_ended: false,
                        writer: None,
                    };
                    let response_bytes = web_ino_http_response(&response);
                    std::io::Write::write_all(&mut stream, &response_bytes).map_err(|err| {
                        IcooError::runtime(
                            format!("web.ino response write failed: {}", err),
                            Some(span),
                        )
                    })?;
                }
                WebInoAccepted::AcceptError(message) => {
                    let _ = accept_handle.join();
                    return Err(IcooError::runtime(message, Some(span)));
                }
            }
        }
        accept_handle
            .join()
            .map_err(|_| IcooError::runtime("web.ino listen accept thread panicked", Some(span)))?;
        for worker_handle in worker_handles {
            worker_handle.join().map_err(|_| {
                IcooError::runtime("web.ino listen worker thread panicked", Some(span))
            })?;
        }
        Ok(())
    }

    fn web_ino_handle_request(
        &mut self,
        app: Rc<RefCell<WebInoApp>>,
        request_bytes: &[u8],
        stream: &mut std::net::TcpStream,
        span: Span,
    ) -> IcooResult<()> {
        let mut request = parse_web_ino_request(request_bytes, span)?;
        let route = {
            let app_ref = app.borrow();
            web_ino_match_route(&app_ref, &mut request)
        };
        let writer = stream.try_clone().map_err(|err| {
            IcooError::runtime(
                format!("web.ino response stream clone failed: {}", err),
                Some(span),
            )
        })?;
        let response = Rc::new(RefCell::new(WebInoResponse {
            status: 200,
            body: String::new(),
            body_bytes: None,
            chunks: Vec::new(),
            content_type: "text/plain; charset=utf-8".to_string(),
            content_type_overridden: false,
            headers: HashMap::new(),
            sent: false,
            streaming: false,
            headers_sent: false,
            stream_ended: false,
            writer: Some(Rc::new(RefCell::new(writer))),
        }));
        if let Some(route) = route {
            let result = self.call_value(
                route.handler,
                vec![
                    web_ino_request_value(&request),
                    Value::WebInoResponse(response.clone()),
                ],
                span,
            )?;
            if !response.borrow().sent && !matches!(result, Value::Nil) {
                let mut response_ref = response.borrow_mut();
                response_ref.body = result.display();
                response_ref.sent = true;
            }
        } else {
            let mut response_ref = response.borrow_mut();
            response_ref.status = 404;
            response_ref.body = "Not Found".to_string();
            response_ref.sent = true;
        }
        if response.borrow().streaming && response.borrow().headers_sent {
            let mut response_ref = response.borrow_mut();
            web_ino_end_stream(&mut response_ref, span)?;
            return Ok(());
        }
        let response_bytes = web_ino_http_response(&response.borrow());
        std::io::Write::write_all(stream, &response_bytes).map_err(|err| {
            IcooError::runtime(
                format!("web.ino response write failed: {}", err),
                Some(span),
            )
        })
    }
}

fn read_web_ino_request_bytes(stream: &mut std::net::TcpStream) -> Result<Vec<u8>, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| format!("web.ino request read failed: {}", err))?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let size = std::io::Read::read(stream, &mut buffer)
            .map_err(|err| format!("web.ino request read failed: {}", err))?;
        if size == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..size]);
        let Some(body_start) = find_http_body_start(&bytes) else {
            continue;
        };
        let head = String::from_utf8_lossy(&bytes[..body_start]);
        let content_length = http_content_length(&head);
        if content_length
            .map(|length| bytes.len() >= body_start + length)
            .unwrap_or(true)
        {
            break;
        }
    }
    Ok(bytes)
}

fn parse_web_ino_request(request: &[u8], span: Span) -> IcooResult<ParsedWebInoRequest> {
    let body_start = find_http_body_start(request).unwrap_or(request.len());
    let head = String::from_utf8_lossy(&request[..body_start]);
    let body_bytes = if body_start < request.len() {
        request[body_start..].to_vec()
    } else {
        Vec::new()
    };
    let body = String::from_utf8_lossy(&body_bytes).into_owned();
    let mut lines = head.lines();
    let request_line = lines.next().ok_or_else(|| {
        IcooError::runtime("invalid HTTP request: missing request line", Some(span))
    })?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| IcooError::runtime("invalid HTTP request method", Some(span)))?
        .to_ascii_uppercase();
    let target = parts
        .next()
        .ok_or_else(|| IcooError::runtime("invalid HTTP request path", Some(span)))?;
    let (path, query) = target
        .split_once('?')
        .map(|(path, query)| (path.to_string(), query.to_string()))
        .unwrap_or((target.to_string(), String::new()));
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(
                name.trim().to_ascii_lowercase(),
                Value::String(value.trim().to_string()),
            );
        }
    }
    let (form, files) = parse_web_ino_multipart(&headers, &body_bytes);
    Ok(ParsedWebInoRequest {
        method,
        path,
        query_params: parse_web_ino_query_params(&query),
        query,
        params: HashMap::new(),
        headers,
        body,
        body_bytes,
        form,
        files,
    })
}

fn web_ino_request_value(request: &ParsedWebInoRequest) -> Value {
    let mut map = HashMap::new();
    map.insert("method".to_string(), Value::String(request.method.clone()));
    map.insert("path".to_string(), Value::String(request.path.clone()));
    map.insert("query".to_string(), Value::String(request.query.clone()));
    map.insert(
        "query_params".to_string(),
        Value::Map(Rc::new(RefCell::new(request.query_params.clone()))),
    );
    map.insert(
        "params".to_string(),
        Value::Map(Rc::new(RefCell::new(request.params.clone()))),
    );
    map.insert(
        "headers".to_string(),
        Value::Map(Rc::new(RefCell::new(request.headers.clone()))),
    );
    map.insert("body".to_string(), Value::String(request.body.clone()));
    map.insert(
        "body_bytes".to_string(),
        Value::Bytes(Rc::new(request.body_bytes.clone())),
    );
    map.insert(
        "form".to_string(),
        Value::Map(Rc::new(RefCell::new(request.form.clone()))),
    );
    map.insert(
        "files".to_string(),
        Value::Map(Rc::new(RefCell::new(request.files.clone()))),
    );
    Value::Map(Rc::new(RefCell::new(map)))
}

fn parse_web_ino_multipart(
    headers: &HashMap<String, Value>,
    body: &[u8],
) -> (HashMap<String, Value>, HashMap<String, Value>) {
    let mut form = HashMap::new();
    let mut files = HashMap::new();
    let Some(Value::String(content_type)) = headers.get("content-type") else {
        return (form, files);
    };
    let Some(boundary) = multipart_boundary(content_type) else {
        return (form, files);
    };
    let marker = format!("--{}", boundary).into_bytes();
    for part in split_bytes(body, &marker).into_iter().skip(1) {
        let part = strip_prefix_bytes(part, b"\r\n");
        if part.starts_with(b"--") {
            break;
        }
        let Some(part_body_start) = find_http_body_start(part) else {
            continue;
        };
        let part_head = String::from_utf8_lossy(&part[..part_body_start]);
        let part_body = &part[part_body_start..];
        let mut disposition = HashMap::new();
        let mut content_type = "application/octet-stream".to_string();
        for line in part_head.lines() {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("content-disposition") {
                disposition = parse_header_parameters(value);
            } else if name.trim().eq_ignore_ascii_case("content-type") {
                content_type = value.trim().to_string();
            }
        }
        let Some(field_name) = disposition.get("name").cloned() else {
            continue;
        };
        let content_bytes = strip_suffix_bytes(part_body, b"\r\n").to_vec();
        let content = String::from_utf8_lossy(&content_bytes).into_owned();
        if let Some(filename) = disposition.get("filename").cloned() {
            let mut file = HashMap::new();
            file.insert("field".to_string(), Value::String(field_name.clone()));
            file.insert("filename".to_string(), Value::String(filename));
            file.insert("content_type".to_string(), Value::String(content_type));
            file.insert("content".to_string(), Value::String(content.clone()));
            file.insert(
                "content_bytes".to_string(),
                Value::Bytes(Rc::new(content_bytes.clone())),
            );
            file.insert("size".to_string(), Value::Int(content_bytes.len() as i64));
            files.insert(field_name, Value::Map(Rc::new(RefCell::new(file))));
        } else {
            form.insert(field_name, Value::String(content));
        }
    }
    (form, files)
}

fn multipart_boundary(content_type: &str) -> Option<String> {
    let mut parts = content_type.split(';');
    let media_type = parts.next()?.trim();
    if !media_type.eq_ignore_ascii_case("multipart/form-data") {
        return None;
    }
    parts.find_map(|part| {
        let (name, value) = part.split_once('=')?;
        if name.trim().eq_ignore_ascii_case("boundary") {
            Some(trim_header_quotes(value.trim()).to_string())
        } else {
            None
        }
    })
}

fn parse_header_parameters(value: &str) -> HashMap<String, String> {
    value
        .split(';')
        .filter_map(|part| {
            let (name, value) = part.split_once('=')?;
            Some((
                name.trim().to_ascii_lowercase(),
                trim_header_quotes(value.trim()).to_string(),
            ))
        })
        .collect()
}

fn trim_header_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn split_bytes<'a>(value: &'a [u8], separator: &[u8]) -> Vec<&'a [u8]> {
    if separator.is_empty() {
        return vec![value];
    }
    let mut parts = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index + separator.len() <= value.len() {
        if &value[index..index + separator.len()] == separator {
            parts.push(&value[start..index]);
            index += separator.len();
            start = index;
        } else {
            index += 1;
        }
    }
    parts.push(&value[start..]);
    parts
}

fn strip_prefix_bytes<'a>(value: &'a [u8], prefix: &[u8]) -> &'a [u8] {
    value.strip_prefix(prefix).unwrap_or(value)
}

fn strip_suffix_bytes<'a>(value: &'a [u8], suffix: &[u8]) -> &'a [u8] {
    value.strip_suffix(suffix).unwrap_or(value)
}

fn parse_web_ino_query_params(query: &str) -> HashMap<String, Value> {
    let mut params = HashMap::new();
    if query.is_empty() {
        return params;
    }
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        params.insert(
            percent_decode_form_component(name),
            Value::String(percent_decode_form_component(value)),
        );
    }
    params
}

fn percent_decode_form_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &value[index + 1..index + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    decoded.push(byte);
                    index += 3;
                } else {
                    decoded.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn web_ino_route_key(method: &str, path: &str) -> String {
    let mut key = String::with_capacity(method.len() + path.len() + 1);
    key.push_str(method);
    key.push(' ');
    key.push_str(path);
    key
}

fn web_ino_route_is_parameterized(path: &str) -> bool {
    web_ino_path_segments(path)
        .iter()
        .any(|segment| segment.starts_with(':') && segment.len() > 1)
}

fn web_ino_match_route(app: &WebInoApp, request: &mut ParsedWebInoRequest) -> Option<WebInoRoute> {
    if let Some(route) = app
        .routes
        .get(&web_ino_route_key(&request.method, &request.path))
        .cloned()
    {
        request.params.clear();
        return Some(route);
    }
    app.routes
        .values()
        .filter(|route| route.method == request.method && route.parameterized)
        .find_map(|route| {
            let params = web_ino_route_params(&route.path, &request.path)?;
            request.params = params;
            Some(route.clone())
        })
}

fn web_ino_route_params(route_path: &str, request_path: &str) -> Option<HashMap<String, Value>> {
    let route_segments = web_ino_path_segments(route_path);
    let request_segments = web_ino_path_segments(request_path);
    if route_segments.len() != request_segments.len() {
        return None;
    }
    let mut params = HashMap::new();
    for (route_segment, request_segment) in route_segments.iter().zip(request_segments.iter()) {
        if let Some(name) = route_segment.strip_prefix(':') {
            if name.is_empty() {
                return None;
            }
            params.insert(
                name.to_string(),
                Value::String(percent_decode_form_component(request_segment)),
            );
        } else if route_segment != request_segment {
            return None;
        }
    }
    Some(params)
}

fn web_ino_path_segments(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn web_ino_write_stream_chunk(
    response: &mut WebInoResponse,
    chunk: &[u8],
    span: Span,
) -> IcooResult<()> {
    if response.stream_ended {
        return Err(IcooError::runtime(
            "cannot write after response stream ended",
            Some(span),
        ));
    }
    response.streaming = true;
    response.sent = true;
    if !response.headers_sent {
        web_ino_write_stream_headers(response, span)?;
    }
    if let Some(writer) = response.writer.clone() {
        let mut writer = writer.borrow_mut();
        let header = format!("{:X}\r\n", chunk.len());
        std::io::Write::write_all(&mut *writer, header.as_bytes())
            .and_then(|_| std::io::Write::write_all(&mut *writer, chunk))
            .and_then(|_| std::io::Write::write_all(&mut *writer, b"\r\n"))
            .map_err(|err| {
                IcooError::runtime(
                    format!("web.ino stream response write failed: {}", err),
                    Some(span),
                )
            })?;
    } else {
        response.chunks.push(chunk.to_vec());
    }
    Ok(())
}

fn web_ino_write_stream_headers(response: &mut WebInoResponse, span: Span) -> IcooResult<()> {
    let status_text = http_status_text(response.status);
    let mut headers = format!(
        "HTTP/1.1 {} {}\r\nTransfer-Encoding: chunked\r\nContent-Type: {}\r\nConnection: close\r\n",
        response.status, status_text, response.content_type
    );
    for (name, value) in &response.headers {
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }
    headers.push_str("\r\n");
    if let Some(writer) = response.writer.clone() {
        std::io::Write::write_all(&mut *writer.borrow_mut(), headers.as_bytes()).map_err(
            |err| {
                IcooError::runtime(
                    format!("web.ino stream response header write failed: {}", err),
                    Some(span),
                )
            },
        )?;
    }
    response.headers_sent = true;
    Ok(())
}

fn web_ino_end_stream(response: &mut WebInoResponse, span: Span) -> IcooResult<()> {
    if response.stream_ended {
        return Ok(());
    }
    response.streaming = true;
    response.sent = true;
    if !response.headers_sent {
        web_ino_write_stream_headers(response, span)?;
    }
    if let Some(writer) = response.writer.clone() {
        std::io::Write::write_all(&mut *writer.borrow_mut(), b"0\r\n\r\n").map_err(|err| {
            IcooError::runtime(
                format!("web.ino stream response end failed: {}", err),
                Some(span),
            )
        })?;
    }
    response.stream_ended = true;
    Ok(())
}

fn web_ino_http_response(response: &WebInoResponse) -> Vec<u8> {
    let body = if let Some(bytes) = &response.body_bytes {
        bytes.clone()
    } else if response.streaming {
        response.chunks.concat()
    } else {
        response.body.clone().into_bytes()
    };
    let status_text = http_status_text(response.status);
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n",
        response.status,
        status_text,
        body.len(),
        response.content_type
    );
    for (name, value) in &response.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(&body);
    bytes
}

fn web_ino_download_filename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("download")
        .to_string()
}

fn web_ino_download_content_type(path: &str) -> &'static str {
    match std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "txt" | "log" | "md" => "text/plain; charset=utf-8",
        "html" | "htm" => "text/html; charset=utf-8",
        "json" => "application/json",
        "csv" => "text/csv; charset=utf-8",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

fn web_ino_escape_header_value(value: &str) -> String {
    value
        .replace(['\r', '\n'], "_")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn web_ino_validate_header_name(name: &str, span: Span) -> IcooResult<()> {
    if name.contains(['\r', '\n']) {
        return Err(IcooError::runtime(
            "HTTP header name cannot contain CR or LF",
            Some(span),
        ));
    }
    Ok(())
}

fn web_ino_validate_header_value(value: &str, span: Span) -> IcooResult<()> {
    if value.contains(['\r', '\n']) {
        return Err(IcooError::runtime(
            "HTTP header value cannot contain CR or LF",
            Some(span),
        ));
    }
    Ok(())
}
