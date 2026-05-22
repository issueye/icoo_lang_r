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

fn response_head(response: &[u8]) -> String {
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    String::from_utf8_lossy(&response[..split]).into_owned()
}

fn response_body(response: &[u8]) -> String {
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    String::from_utf8_lossy(&response[split + 4..]).into_owned()
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

fn custom(req: Map<String, Any>, res: WebInoResponse):
    res.status(202)
    res.header("X-Trace-Id", "abc123")
    res.content_type("text/custom; charset=utf-8")
    res.send("ok")

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

fn stream(req: Map<String, Any>, res: WebInoResponse):
    res.header("X-Stream-Id", "stream-1")
    res.content_type("text/event-stream")
    res.write("a")
    res.write("b")
    res.end()

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

fn bad(req: Map<String, Any>, res: WebInoResponse):
    {}
    res.send("bad")

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
