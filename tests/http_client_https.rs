use rcgen::{generate_simple_self_signed, CertifiedKey};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn run_with_roots(source: &str, roots: rustls::RootCertStore) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    icoo_lang_r::run_source_with_output_and_http_tls_roots(
        source,
        move |line| {
            captured.borrow_mut().push(line);
        },
        roots,
    )
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
}

fn run(source: &str) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    icoo_lang_r::run_source_with_output(source, move |line| {
        captured.borrow_mut().push(line);
    })
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
}

fn https_material() -> (
    rustls::RootCertStore,
    Arc<rustls::ServerConfig>,
    CertificateDer<'static>,
) {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(["localhost".to_string()]).unwrap();
    let cert_der = cert.der().clone();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key)
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert_der.clone()).unwrap();
    (roots, Arc::new(server_config), cert_der)
}

fn start_https_server(
    response: Vec<u8>,
) -> (
    u16,
    rustls::RootCertStore,
    thread::JoinHandle<Result<String, String>>,
) {
    let (roots, server_config, _cert_der) = https_material();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (tcp, _) = listener.accept().map_err(|err| err.to_string())?;
        tcp.set_read_timeout(Some(Duration::from_secs(5)))
            .and_then(|_| tcp.set_write_timeout(Some(Duration::from_secs(5))))
            .map_err(|err| err.to_string())?;
        let connection =
            rustls::ServerConnection::new(server_config).map_err(|err| err.to_string())?;
        let mut stream = rustls::StreamOwned::new(connection, tcp);
        let request = read_http_request(&mut stream)?;
        stream.write_all(&response).map_err(|err| err.to_string())?;
        Ok(String::from_utf8_lossy(&request).into_owned())
    });
    (port, roots, handle)
}

fn read_http_request(
    stream: &mut rustls::StreamOwned<rustls::ServerConnection, std::net::TcpStream>,
) -> Result<Vec<u8>, String> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 512];
    loop {
        let size = stream.read(&mut buffer).map_err(|err| err.to_string())?;
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
    let header_text = String::from_utf8_lossy(&request[..header_end]);
    let content_length = header_text
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while request.len() < header_end + content_length {
        let size = stream.read(&mut buffer).map_err(|err| err.to_string())?;
        if size == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..size]);
    }
    Ok(request)
}

#[test]
fn https_client_get_returns_text_body() {
    let (port, roots, server) = start_https_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello".to_vec(),
    );

    let output = run_with_roots(
        &format!(
            r#"import "std.net.http.client" as client

let response = client.get("https://localhost:{}/hello")
print(response.get("status").to_string())
print(response.get("body"))"#,
            port
        ),
        roots,
    )
    .unwrap();
    let request = server.join().unwrap().unwrap();

    assert_eq!(output, vec!["200", "hello"]);
    assert!(request.contains("GET /hello HTTP/1.1"), "{request}");
    assert!(
        request.contains(&format!("Host: localhost:{}", port)),
        "{request}"
    );
}

#[test]
fn https_client_get_bytes_preserves_binary_body() {
    let mut response =
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n".to_vec();
    response.extend_from_slice(&[0, 255, b'A', b'B']);
    let (port, roots, server) = start_https_server(response);

    let output = run_with_roots(
        &format!(
            r#"import "std.net.http.client" as client

let response = client.get_bytes("https://localhost:{}/bin")
let body: Bytes = response.get("body")
print(response.get("status").to_string())
print(body.to_hex())"#,
            port
        ),
        roots,
    )
    .unwrap();
    let _request = server.join().unwrap().unwrap();

    assert_eq!(output, vec!["200", "00ff4142"]);
}

#[test]
fn https_client_post_sends_body_and_headers() {
    let (port, roots, server) = start_https_server(
        b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec(),
    );

    let output = run_with_roots(
        &format!(
            r#"import "std.net.http.client" as client

let response = client.post("https://localhost:{}/submit", "payload", {{"X-Trace": "tls"}})
print(response.get("status").to_string())
print(response.get("body"))"#,
            port
        ),
        roots,
    )
    .unwrap();
    let request = server.join().unwrap().unwrap();

    assert_eq!(output, vec!["201", "ok"]);
    assert!(request.contains("POST /submit HTTP/1.1"), "{request}");
    assert!(request.contains("Content-Length: 7"), "{request}");
    assert!(request.contains("X-Trace: tls"), "{request}");
    assert!(request.ends_with("payload"), "{request}");
}

#[test]
fn https_client_stream_get_handles_chunked_response() {
    let (port, roots, server) = start_https_server(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n"
            .to_vec(),
    );

    let output = run_with_roots(
        &format!(
            r#"import "std.net.http.client" as client

let chunks = []

fn on_chunk(chunk: String) {{
    chunks.push(chunk)

}}
let response = client.stream_get("https://localhost:{}/stream", on_chunk)
print(response.get("status").to_string())
print(response.get("chunks").to_string())
print(chunks.join(""))"#,
            port
        ),
        roots,
    )
    .unwrap();
    let _request = server.join().unwrap().unwrap();

    assert_eq!(output, vec!["200", "1", "hello"]);
}

#[test]
fn https_client_rejects_untrusted_certificate() {
    let (port, _roots, server) = start_https_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec(),
    );

    let err = run(&format!(
        r#"import "std.net.http.client" as client
client.get("https://localhost:{}/untrusted")"#,
        port
    ))
    .unwrap_err();
    let _ = server.join().unwrap();

    assert!(err.contains("https client TLS handshake failed"), "{err}");
}

#[test]
fn unsupported_url_scheme_mentions_http_and_https() {
    let err = run(r#"import "std.net.http.client" as client
client.get("ftp://localhost/")"#)
    .unwrap_err();

    assert!(
        err.contains("only http:// and https:// URLs are supported"),
        "{err}"
    );
}
