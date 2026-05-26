use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

fn run(source: &str) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    icoo_lang_r::run_source_with_output(source, move |line| {
        captured.borrow_mut().push(line);
    })
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
}

fn spawn_http_server(
    listener: TcpListener,
    response: &'static str,
) -> (u16, thread::JoinHandle<String>) {
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 512];
        loop {
            let size = stream.read(&mut buffer).unwrap();
            request.extend_from_slice(&buffer[..size]);
            if size == 0 || request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let header_end = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|index| index + 4)
            .unwrap_or(request.len());
        let header_text = String::from_utf8_lossy(&request[..header_end]).into_owned();
        let content_length = header_text
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while request.len() < header_end + content_length {
            let size = stream.read(&mut buffer).unwrap();
            if size == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..size]);
        }
        stream.write_all(response.as_bytes()).unwrap();
        String::from_utf8_lossy(&request).into_owned()
    });
    (port, handle)
}

fn start_http_server(response: &'static str) -> (u16, thread::JoinHandle<String>) {
    spawn_http_server(TcpListener::bind("127.0.0.1:0").unwrap(), response)
}

#[test]
fn http_client_uses_custom_port_from_url() {
    let (port, server) = start_http_server(
        "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\ncustom",
    );

    let output = run(&format!(
        r#"
import "std.net.http.client" as client

let response = client.get("http://127.0.0.1:{}/custom-port")
print(response.get("status").to_string())
print(response.get("body"))
"#,
        port
    ))
    .unwrap();
    let request = server.join().unwrap();

    assert_eq!(output, vec!["200", "custom"]);
    assert!(request.contains("GET /custom-port HTTP/1.1"), "{request}");
}

#[test]
fn http_client_uses_default_port_when_url_omits_port() {
    let Ok(listener) = TcpListener::bind("127.0.0.1:80") else {
        return;
    };
    let (_port, server) = spawn_http_server(
        listener,
        "HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\ndefault",
    );

    let output = run(r#"
import "std.net.http.client" as client

let response = client.get("http://127.0.0.1/default-port")
print(response.get("status").to_string())
print(response.get("body"))
"#)
    .unwrap();
    let request = server.join().unwrap();

    assert_eq!(output, vec!["200", "default"]);
    assert!(request.contains("GET /default-port HTTP/1.1"), "{request}");
}

#[test]
fn http_client_sends_custom_headers_on_regular_requests() {
    let (port, server) =
        start_http_server("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");

    let output = run(&format!(r#"
import "std.net.http.client" as client

let response = client.get("http://127.0.0.1:{}/headers", {{"X-Trace-Id": "abc123", "X-Mode": "regular"}})
print(response.get("status").to_string())
print(response.get("body"))
"#,
        port
    ))
    .unwrap();
    let request = server.join().unwrap();

    assert_eq!(output, vec!["200", "ok"]);
    assert!(request.contains("X-Trace-Id: abc123"), "{request}");
    assert!(request.contains("X-Mode: regular"), "{request}");
}

#[test]
fn http_client_sends_custom_headers_on_body_requests() {
    let (port, server) = start_http_server(
        "HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nupdated",
    );

    let output = run(&format!(
        r#"
import "std.net.http.client" as client

let response = client.put("http://127.0.0.1:{}/items/1", "payload", {{"X-Trace-Id": "body123"}})
print(response.get("status").to_string())
print(response.get("body"))
"#,
        port
    ))
    .unwrap();
    let request = server.join().unwrap();

    assert_eq!(output, vec!["200", "updated"]);
    assert!(request.contains("PUT /items/1 HTTP/1.1"), "{request}");
    assert!(request.contains("X-Trace-Id: body123"), "{request}");
    assert!(request.ends_with("payload"), "{request}");
}

#[test]
fn http_client_sends_custom_headers_on_streaming_requests() {
    let (port, server) = start_http_server(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
    );

    let output = run(&format!(
        r#"
import "std.net.http.client" as client

let chunks = []

fn on_chunk(chunk: String) {{
    chunks.push(chunk)

}}
let response = client.stream_get("http://127.0.0.1:{}/stream", {{"X-Mode": "stream"}}, on_chunk)
print(response.get("status").to_string())
print(response.get("chunks").to_string())
print(chunks.join(""))
"#,
        port
    ))
    .unwrap();
    let request = server.join().unwrap();

    assert_eq!(output, vec!["200", "1", "hello"]);
    assert!(request.contains("X-Mode: stream"), "{request}");
}

#[test]
fn http_client_keeps_old_stream_signature_without_headers() {
    let (port, server) = start_http_server(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\nok\r\n0\r\n\r\n",
    );

    let output = run(&format!(
        r#"
import "std.net.http.client" as client

let chunks = []

fn on_chunk(chunk: String) {{
    chunks.push(chunk)

}}
let response = client.stream_get("http://127.0.0.1:{}/stream", on_chunk)
print(response.get("status").to_string())
print(chunks.join(""))
"#,
        port
    ))
    .unwrap();
    let _request = server.join().unwrap();

    assert_eq!(output, vec!["200", "ok"]);
}

#[test]
fn http_client_reports_malformed_chunked_response() {
    let (port, server) = start_http_server(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhi\r\n0\r\n\r\n",
    );

    let err = run(&format!(
        r#"
import "std.net.http.client" as client
client.get("http://127.0.0.1:{}/bad-chunk")
"#,
        port
    ))
    .unwrap_err();
    let _request = server.join().unwrap();

    assert!(err.contains("invalid chunked response"), "{err}");
}

#[test]
fn http_client_streams_content_length_response() {
    let (port, server) = start_http_server(
        "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello world",
    );

    let output = run(&format!(
        r#"
import "std.net.http.client" as client

let chunks = []

fn on_chunk(chunk: String) {{
    chunks.push(chunk)

}}
let response = client.stream_get("http://127.0.0.1:{}/length", on_chunk)
print(response.get("status").to_string())
print(response.get("headers").get("content-length"))
print(response.get("chunks").to_string())
print(chunks.join(""))
"#,
        port
    ))
    .unwrap();
    let request = server.join().unwrap();

    assert_eq!(output, vec!["200", "11", "1", "hello world"]);
    assert!(request.contains("GET /length HTTP/1.1"), "{request}");
}

#[test]
fn http_client_rejects_header_injection() {
    let err = run(r#"
import "std.net.http.client" as client
client.get("http://127.0.0.1:1/", {"X-Good": "ok\nX-Bad: yes"})
"#)
    .unwrap_err();

    assert!(err.contains("HTTP header names and values cannot contain CR or LF"));
}
