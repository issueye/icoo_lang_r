use icoo_lang_r::runtime::limits::{MAX_HTTP_BODY_BYTES, MAX_HTTP_STREAM_CHUNK_BYTES};
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

fn spawn_http_server(response: Vec<u8>) -> (u16, thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
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
        stream.write_all(&response).unwrap();
        request
    });
    (port, handle)
}

#[test]
fn http_client_get_bytes_preserves_binary_response_body() {
    let body = [0_u8, 255, b'A', b'B'];
    let mut response =
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n".to_vec();
    response.extend_from_slice(&body);
    let (port, server) = spawn_http_server(response);

    let output = run(&format!(
        r#"
import "std.net.http.client" as client

let response = client.get_bytes("http://127.0.0.1:{}/bin")
let body: Bytes = response.get("body")
print(response.get("status").to_string())
print(body.type_name())
print(body.len().to_string())
print(body.to_hex())
print(body.to_string("lossy").len().to_string())
"#,
        port
    ))
    .unwrap();
    let request = server.join().unwrap();
    let request_text = String::from_utf8_lossy(&request);

    assert_eq!(output, vec!["200", "Bytes", "4", "00ff4142", "4"]);
    assert!(request_text.contains("GET /bin HTTP/1.1"), "{request_text}");
}

#[test]
fn http_client_post_bytes_sends_binary_request_body() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec();
    let (port, server) = spawn_http_server(response);

    let output = run(&format!(
        r#"
import "std.net.http.client" as client

let payload = "AZ".to_bytes().concat("!".to_bytes())
let response = client.post_bytes("http://127.0.0.1:{}/upload", payload, {{"X-Mode": "bytes"}})
let body: Bytes = response.get("body")
print(response.get("status").to_string())
print(body.to_string())
"#,
        port
    ))
    .unwrap();
    let request = server.join().unwrap();
    let request_text = String::from_utf8_lossy(&request);

    assert_eq!(output, vec!["200", "ok"]);
    assert!(
        request_text.contains("POST /upload HTTP/1.1"),
        "{request_text}"
    );
    assert!(request_text.contains("Content-Length: 3"), "{request_text}");
    assert!(
        request_text.contains("Content-Type: application/octet-stream"),
        "{request_text}"
    );
    assert!(request_text.contains("X-Mode: bytes"), "{request_text}");
    assert!(request.ends_with(b"AZ!"), "{request_text}");
}

#[test]
fn http_client_stream_get_bytes_delivers_bytes_chunks() {
    let response =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nA\xffB\r\n2\r\nCD\r\n0\r\n\r\n"
            .to_vec();
    let (port, server) = spawn_http_server(response);

    let output = run(&format!(
        r#"
import "std.net.http.client" as client

let chunks = []

fn on_chunk(chunk: Bytes) {{
    chunks.push(chunk.to_hex())
}}

let response = client.stream_get_bytes("http://127.0.0.1:{}/bin", on_chunk)
print(response.get("status").to_string())
print(response.get("chunks").to_string())
print(chunks.join("|"))
"#,
        port
    ))
    .unwrap();
    let request = server.join().unwrap();
    let request_text = String::from_utf8_lossy(&request);

    assert_eq!(output, vec!["200", "2", "41ff42|4344"]);
    assert!(request_text.contains("GET /bin HTTP/1.1"), "{request_text}");
}

#[test]
fn http_client_stream_post_bytes_sends_binary_body_and_receives_bytes_chunks() {
    let response =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\nok\r\n0\r\n\r\n"
            .to_vec();
    let (port, server) = spawn_http_server(response);

    let output = run(&format!(
        r#"
import "std.net.http.client" as client

let chunks = []

fn on_chunk(chunk: Bytes) {{
    chunks.push(chunk.to_string())
}}

let response = client.stream_post_bytes("http://127.0.0.1:{}/upload", Bytes.from_hex("00ff41"), {{"X-Stream": "bytes"}}, on_chunk)
print(response.get("status").to_string())
print(chunks.join(""))
"#,
        port
    ))
    .unwrap();
    let request = server.join().unwrap();
    let request_text = String::from_utf8_lossy(&request);

    assert_eq!(output, vec!["200", "ok"]);
    assert!(
        request_text.contains("POST /upload HTTP/1.1"),
        "{request_text}"
    );
    assert!(request_text.contains("Content-Length: 3"), "{request_text}");
    assert!(request_text.contains("X-Stream: bytes"), "{request_text}");
    assert!(request.ends_with(&[0, 255, b'A']), "{request_text}");
}

#[test]
fn http_client_bytes_methods_are_type_checked() {
    let err = run(r#"
import "std.net.http.client" as client
client.post_bytes("http://127.0.0.1:1/", "text")
"#)
    .unwrap_err();

    assert!(err.contains("type error: expected Bytes for argument 2 but got String"));

    let err = run(r#"
import "std.net.http.client" as client

fn on_chunk(chunk: Bytes) {
    print(chunk.len().to_string())
}

client.stream_post_bytes("http://127.0.0.1:1/", "text", on_chunk)
"#)
    .unwrap_err();

    assert!(err.contains("type error: expected Bytes for argument 2 but got String"));
}

#[test]
fn http_client_rejects_oversized_response_body() {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        MAX_HTTP_BODY_BYTES + 1
    )
    .into_bytes();
    response.extend(std::iter::repeat_n(b'x', MAX_HTTP_BODY_BYTES + 1));
    let (port, server) = spawn_http_server(response);

    let err = run(&format!(
        r#"
import "std.net.http.client" as client
client.get_bytes("http://127.0.0.1:{}/big")
"#,
        port
    ))
    .unwrap_err();
    let _request = server.join().unwrap();

    assert!(
        err.contains("http response body exceeds maximum size"),
        "{err}"
    );
}

#[test]
fn http_client_rejects_oversized_stream_chunk() {
    let chunk_len = MAX_HTTP_STREAM_CHUNK_BYTES + 1;
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
        chunk_len
    )
    .into_bytes();
    response.extend(std::iter::repeat_n(b'x', chunk_len));
    response.extend_from_slice(b"\r\n0\r\n\r\n");
    let (port, server) = spawn_http_server(response);

    let err = run(&format!(
        r#"
import "std.net.http.client" as client

fn on_chunk(chunk: String) {{
    print(chunk)
}}

client.stream_get("http://127.0.0.1:{}/stream", on_chunk)
"#,
        port
    ))
    .unwrap_err();
    let _request = server.join().unwrap();

    assert!(
        err.contains("http stream chunk exceeds maximum size"),
        "{err}"
    );
}
