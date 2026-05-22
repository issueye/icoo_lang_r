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
fn http_client_bytes_methods_are_type_checked() {
    let err = run(r#"
import "std.net.http.client" as client
client.post_bytes("http://127.0.0.1:1/", "text")
"#)
    .unwrap_err();

    assert!(err.contains("type error: expected Bytes for argument 2 but got String"));
}
