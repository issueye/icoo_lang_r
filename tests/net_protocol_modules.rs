use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
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

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for _ in 0..40 {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => return stream,
            Err(_) => thread::sleep(Duration::from_millis(25)),
        }
    }
    panic!("server did not accept connections on port {}", port);
}

#[test]
fn socket_client_send_round_trips_text() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        stream.read_to_end(&mut request).unwrap();
        assert_eq!(String::from_utf8_lossy(&request), "ping");
        stream.write_all(b"pong").unwrap();
    });

    let output = run(&format!(
        r#"
import "std.net.socket.client" as socket

let response = socket.send("127.0.0.1", {}, "ping")
print(response)
"#,
        port
    ))
    .unwrap();
    server.join().unwrap();
    assert_eq!(output, vec!["pong"]);
}

#[test]
fn socket_server_serve_once_invokes_handler() {
    let port = free_port();
    let source = format!(
        r#"
import "std.net.socket.server" as socket

fn handle(payload: Bytes):
    return "socket-reply"

socket.serve_once("127.0.0.1", {}, handle)
"#,
        port
    );
    let server = thread::spawn(move || run(&source).unwrap());

    let mut stream = connect_with_retry(port);
    stream.write_all(b"client-payload").unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();

    server.join().unwrap();
    assert_eq!(response, "socket-reply");
}

#[test]
fn sse_client_get_delivers_events_to_handler() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 256];
        loop {
            let size = stream.read(&mut buffer).unwrap();
            request.extend_from_slice(&buffer[..size]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let body = "event: greeting\ndata: hello\ndata: world\nid: 7\n\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let output = run(&format!(
        r#"
import "std.net.sse.client" as sse

let events = []

fn on_event(event: Map<String, Any>):
    events.push(event.get("event") + ":" + event.get("data") + ":" + event.get("id"))

let response = sse.get("http://127.0.0.1:{}/events", on_event)
print(response.get("status").to_string())
print(response.get("events").to_string())
print(events.at(0))
"#,
        port
    ))
    .unwrap();
    server.join().unwrap();
    assert_eq!(output, vec!["200", "1", "greeting:hello\nworld:7"]);
}

#[test]
fn sse_server_serve_once_emits_event_stream() {
    let port = free_port();
    let source = format!(
        r#"
import "std.net.sse.server" as sse

sse.serve_once("127.0.0.1", {}, "hello")
"#,
        port
    );
    let server = thread::spawn(move || run(&source).unwrap());

    let mut stream = connect_with_retry(port);
    stream
        .write_all(b"GET /events HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();

    server.join().unwrap();
    assert!(
        response.contains("Content-Type: text/event-stream"),
        "{response}"
    );
    assert!(response.contains("data: hello"), "{response}");
}

#[test]
fn ws_client_send_text_round_trips_one_frame() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_http_head(&mut stream);
        stream
            .write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: test\r\n\r\n",
            )
            .unwrap();
        let (opcode, payload) = read_ws_frame(&mut stream, true);
        assert_eq!(opcode, 0x1);
        assert_eq!(String::from_utf8_lossy(&payload), "hello");
        write_ws_frame(&mut stream, 0x1, b"world", false);
    });

    let output = run(&format!(
        r#"
import "std.net.ws.client" as ws

print(ws.send_text("ws://127.0.0.1:{}/chat", "hello"))
"#,
        port
    ))
    .unwrap();
    server.join().unwrap();
    assert_eq!(output, vec!["world"]);
}

#[test]
fn ws_server_serve_once_invokes_handler() {
    let port = free_port();
    let source = format!(
        r#"
import "std.net.ws.server" as ws

fn handle(payload: Bytes):
    return "ws-reply"

ws.serve_once("127.0.0.1", {}, handle)
"#,
        port
    );
    let server = thread::spawn(move || run(&source).unwrap());

    let mut stream = connect_with_retry(port);
    stream
        .write_all(
            b"GET /chat HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
        )
        .unwrap();
    read_http_head(&mut stream);
    write_ws_frame(&mut stream, 0x1, b"client", true);
    let (opcode, payload) = read_ws_frame(&mut stream, false);

    server.join().unwrap();
    assert_eq!(opcode, 0x1);
    assert_eq!(String::from_utf8_lossy(&payload), "ws-reply");
}

fn read_http_head(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 256];
    loop {
        let size = stream.read(&mut buffer).unwrap();
        bytes.extend_from_slice(&buffer[..size]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return bytes;
        }
    }
}

fn read_ws_frame(stream: &mut TcpStream, expect_masked: bool) -> (u8, Vec<u8>) {
    let mut header = [0_u8; 2];
    stream.read_exact(&mut header).unwrap();
    let opcode = header[0] & 0x0f;
    let masked = header[1] & 0x80 != 0;
    assert_eq!(masked, expect_masked);
    let len_code = header[1] & 0x7f;
    let len = match len_code {
        0..=125 => len_code as usize,
        126 => {
            let mut bytes = [0_u8; 2];
            stream.read_exact(&mut bytes).unwrap();
            u16::from_be_bytes(bytes) as usize
        }
        _ => panic!("unexpected websocket payload size"),
    };
    let mut mask = [0_u8; 4];
    if masked {
        stream.read_exact(&mut mask).unwrap();
    }
    let mut payload = vec![0_u8; len];
    stream.read_exact(&mut payload).unwrap();
    if masked {
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % 4];
        }
    }
    (opcode, payload)
}

fn write_ws_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8], masked: bool) {
    let mut frame = Vec::new();
    frame.push(0x80 | opcode);
    let mask_bit = if masked { 0x80 } else { 0 };
    frame.push(mask_bit | payload.len() as u8);
    if masked {
        let mask = [1_u8, 2, 3, 4];
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
    } else {
        frame.extend_from_slice(payload);
    }
    stream.write_all(&frame).unwrap();
}
