use icoo_lang_r::runtime::limits::MAX_WEB_INO_REQUEST_BYTES;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start_server(server_path: PathBuf) -> thread::JoinHandle<Result<(), String>> {
    let handle =
        thread::spawn(move || icoo_lang_r::run_file(server_path).map_err(|err| err.to_string()));
    thread::sleep(Duration::from_millis(150));
    handle
}

fn raw_get(port: u16, path: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        stream,
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        path
    )
    .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    response
}

fn raw_request(port: u16, request: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(request).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    response
}

fn response_head(response: &[u8]) -> String {
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    String::from_utf8_lossy(&response[..split]).into_owned()
}

fn response_body(response: &[u8]) -> String {
    String::from_utf8_lossy(response_body_bytes(response)).into_owned()
}

fn response_body_bytes(response: &[u8]) -> &[u8] {
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    &response[split + 4..]
}

fn icoo_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[test]
fn rejects_oversized_web_ino_request_body() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_oversized_request");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn echo(req: Map<String, Any>, res: WebInoResponse) {{
    res.send("unexpected")
}}
app.post("/upload", echo)
app.listen_once("127.0.0.1", {})
"#,
            port
        ),
    )
    .unwrap();

    let server_handle = start_server(server_path);
    let request = format!(
        "POST /upload HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        MAX_WEB_INO_REQUEST_BYTES + 1
    );
    let response = raw_request(port, request.as_bytes());

    assert!(response_head(&response).contains("400 Bad Request"));
    assert!(
        response_body(&response).contains("web.ino request body exceeds maximum size"),
        "{}",
        response_body(&response)
    );
    server_handle.join().unwrap().unwrap();
}

#[test]
fn supports_custom_headers_and_content_type_on_normal_responses() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_response_headers_normal");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn custom(req: Map<String, Any>, res: WebInoResponse) {{
    res.status(202)
    res.header("X-Trace-Id", "abc123")
    res.content_type("text/custom; charset=utf-8")
    res.send("ok")

}}
app.get("/custom", custom)
app.listen_once("127.0.0.1", {})
"#,
            port
        ),
    )
    .unwrap();

    let server_handle = start_server(server_path);
    let response = raw_get(port, "/custom");
    let head = response_head(&response);

    assert!(head.starts_with("HTTP/1.1 202"));
    assert!(head.contains("Content-Type: text/custom; charset=utf-8"));
    assert!(head.contains("X-Trace-Id: abc123"));
    assert_eq!(response_body(&response), "ok");
    server_handle.join().unwrap().unwrap();
}

#[test]
fn supports_custom_headers_and_content_type_on_chunked_streams() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_response_headers_stream");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn stream(req: Map<String, Any>, res: WebInoResponse) {{
    res.header("X-Stream-Id", "stream-1")
    res.content_type("text/event-stream")
    res.write("a")
    res.write("b")
    res.end()

}}
app.get("/stream", stream)
app.listen_once("127.0.0.1", {})
"#,
            port
        ),
    )
    .unwrap();

    let server_handle = start_server(server_path);
    let response = raw_get(port, "/stream");
    let head = response_head(&response);

    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert!(head.contains("Transfer-Encoding: chunked"));
    assert!(head.contains("Content-Type: text/event-stream"));
    assert!(head.contains("X-Stream-Id: stream-1"));
    assert_eq!(response_body(&response), "1\r\na\r\n1\r\nb\r\n0\r\n\r\n");
    server_handle.join().unwrap().unwrap();
}

#[test]
fn supports_web_ino_bytes_request_and_response_bodies() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_response_headers_bytes");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn echo(req: Map<String, Any>, res: WebInoResponse) {{
    res.send_bytes(req.get("body_bytes"), "application/octet-stream")

}}
app.post("/echo", echo)
app.listen_once("127.0.0.1", {})
"#,
            port
        ),
    )
    .unwrap();

    let server_handle = start_server(server_path);
    let body = [0x00, 0xff, b'A', b'Z'];
    let mut request = format!(
        "POST /echo HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(&body);
    let response = raw_request(port, &request);
    let head = response_head(&response);

    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert!(head.contains("Content-Type: application/octet-stream"));
    assert_eq!(response_body_bytes(&response), body);
    server_handle.join().unwrap().unwrap();
}

#[test]
fn supports_web_ino_multipart_content_bytes() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_response_headers_multipart_bytes");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn upload(req: Map<String, Any>, res: WebInoResponse) {{
    let files = req.get("files")
    let file = files.get("avatar")
    res.send_bytes(file.get("content_bytes"), file.get("content_type"))

}}
app.post("/upload", upload)
app.listen_once("127.0.0.1", {})
"#,
            port
        ),
    )
    .unwrap();

    let server_handle = start_server(server_path);
    let boundary = "icoo-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"avatar\"; filename=\"avatar.bin\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(&[0xff, 0x00, b'A']);
    body.extend_from_slice(format!("\r\n--{}--\r\n", boundary).as_bytes());
    let mut request = format!(
        "POST /upload HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: multipart/form-data; boundary={}\r\nContent-Length: {}\r\n\r\n",
        boundary,
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(&body);
    let response = raw_request(port, &request);
    let head = response_head(&response);

    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert!(head.contains("Content-Type: application/octet-stream"));
    assert_eq!(response_body_bytes(&response), [0xff, 0x00, b'A']);
    server_handle.join().unwrap().unwrap();
}

#[test]
fn supports_web_ino_chunked_write_bytes() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_response_headers_write_bytes");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn stream(req: Map<String, Any>, res: WebInoResponse) {{
    res.content_type("application/octet-stream")
    res.write_bytes(Bytes.from_hex("00ff"))
    res.write_bytes("AZ".to_bytes())
    res.end()

}}
app.get("/stream-bytes", stream)
app.listen_once("127.0.0.1", {})
"#,
            port
        ),
    )
    .unwrap();

    let server_handle = start_server(server_path);
    let response = raw_get(port, "/stream-bytes");
    let head = response_head(&response);

    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert!(head.contains("Transfer-Encoding: chunked"));
    assert!(head.contains("Content-Type: application/octet-stream"));
    assert_eq!(
        response_body_bytes(&response),
        &[
            b'2', b'\r', b'\n', 0x00, 0xff, b'\r', b'\n', b'2', b'\r', b'\n', b'A', b'Z', b'\r',
            b'\n', b'0', b'\r', b'\n', b'\r', b'\n'
        ]
    );
    server_handle.join().unwrap().unwrap();
}

#[test]
fn rejects_lf_header_injection_in_names_and_values() {
    for (case_name, bad_call, expected) in [
        (
            "name",
            r#"res.header("X-Good\nX-Injected", "value")"#,
            "HTTP header name cannot contain CR or LF",
        ),
        (
            "value",
            r#"res.header("X-Good", "value\nX-Injected: yes")"#,
            "HTTP header value cannot contain CR or LF",
        ),
    ] {
        let dir = PathBuf::from(format!(
            "target/icoo_module_tests/web_ino_response_headers_injection_{}",
            case_name
        ));
        fs::create_dir_all(&dir).unwrap();
        let port = free_port();
        let server_path = dir.join("server.icoo");
        fs::write(
            &server_path,
            format!(
                r#"
import "std.web.ino" as ino

let app = ino.App()

fn bad(req: Map<String, Any>, res: WebInoResponse) {{
    {}
    res.send("bad")

}}
app.get("/bad", bad)
app.listen_once("127.0.0.1", {})
"#,
                bad_call, port
            ),
        )
        .unwrap();

        let server_handle = start_server(server_path);
        let _ = TcpStream::connect(("127.0.0.1", port)).and_then(|mut stream| {
            stream
                .write_all(b"GET /bad HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")?;
            let mut response = Vec::new();
            let _ = stream.read_to_end(&mut response);
            Ok(())
        });
        let err = server_handle.join().unwrap().unwrap_err();
        assert!(err.contains(expected), "unexpected error: {}", err);
    }
}

#[test]
fn rejects_response_mutation_after_streaming_starts() {
    for (case_name, bad_call, expected) in [
        (
            "status",
            r#"res.status(201)"#,
            "cannot change HTTP status after streaming has started",
        ),
        (
            "header",
            r#"res.header("X-Late", "value")"#,
            "cannot change HTTP headers after streaming has started",
        ),
        (
            "content_type",
            r#"res.content_type("application/json")"#,
            "cannot change HTTP content type after streaming has started",
        ),
        (
            "send",
            r#"res.send("late")"#,
            "cannot send response after streaming has started",
        ),
        (
            "json",
            r#"res.json({"late": true})"#,
            "cannot send JSON response after streaming has started",
        ),
    ] {
        let dir = PathBuf::from(format!(
            "target/icoo_module_tests/web_ino_response_headers_stream_mutation_{}",
            case_name
        ));
        fs::create_dir_all(&dir).unwrap();
        let port = free_port();
        let server_path = dir.join("server.icoo");
        fs::write(
            &server_path,
            format!(
                r#"
import "std.web.ino" as ino

let app = ino.App()

fn bad(req: Map<String, Any>, res: WebInoResponse) {{
    res.write("a")
    {}

}}
app.get("/bad", bad)
app.listen_once("127.0.0.1", {})
"#,
                bad_call, port
            ),
        )
        .unwrap();

        let server_handle = start_server(server_path);
        let _ = raw_get(port, "/bad");
        let err = server_handle.join().unwrap().unwrap_err();
        assert!(err.contains(expected), "unexpected error: {}", err);
    }
}

#[test]
fn escapes_download_filename_quotes_in_content_disposition() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_response_headers_download_escape");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let download_path = dir.join("payload.txt");
    fs::write(&download_path, "payload").unwrap();
    let server_path = dir.join("server.icoo");
    let download_path = icoo_string(&download_path.display().to_string());
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn download(req: Map<String, Any>, res: WebInoResponse) {{
    res.download("{}", "report \"final\".txt")

}}
app.get("/download", download)
app.listen_once("127.0.0.1", {})
"#,
            download_path, port
        ),
    )
    .unwrap();

    let server_handle = start_server(server_path);
    let response = raw_get(port, "/download");
    let head = response_head(&response);

    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert!(head.contains(r#"Content-Disposition: attachment; filename="report \"final\".txt""#));
    assert_eq!(response_body(&response), "payload");
    server_handle.join().unwrap().unwrap();
}
