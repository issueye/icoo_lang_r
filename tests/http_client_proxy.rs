use icoo_lang_r::{HttpProxyConfig, RuntimeHttpConfig};
use rcgen::{generate_simple_self_signed, CertifiedKey};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn run_with_http_config(
    source: &str,
    http_config: RuntimeHttpConfig,
) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    icoo_lang_r::run_source_with_output_and_http_config(
        source,
        move |line| {
            captured.borrow_mut().push(line);
        },
        http_config,
    )
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
}

fn run_with_http_config_and_roots(
    source: &str,
    http_config: RuntimeHttpConfig,
    roots: rustls::RootCertStore,
) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    icoo_lang_r::run_source_with_output_http_config_and_tls_roots(
        source,
        move |line| {
            captured.borrow_mut().push(line);
        },
        http_config,
        roots,
    )
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
}

fn spawn_proxy(response: &'static str) -> (u16, thread::JoinHandle<String>) {
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
        stream.write_all(response.as_bytes()).unwrap();
        String::from_utf8_lossy(&request).into_owned()
    });
    (port, handle)
}

fn https_material() -> (rustls::RootCertStore, Arc<rustls::ServerConfig>) {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(["localhost".to_string()]).unwrap();
    let cert_der: CertificateDer<'static> = cert.der().clone();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der.clone()], key)
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert_der).unwrap();
    (roots, Arc::new(server_config))
}

fn spawn_https_tunnel_proxy(
    response: &'static [u8],
    server_config: Arc<rustls::ServerConfig>,
) -> (u16, thread::JoinHandle<(String, String)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut tcp, _) = listener.accept().unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(5)))
            .and_then(|_| tcp.set_write_timeout(Some(Duration::from_secs(5))))
            .unwrap();
        let connect_request = read_plain_http_request(&mut tcp);
        tcp.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .unwrap();
        tcp.flush().unwrap();

        let connection = rustls::ServerConnection::new(server_config).unwrap();
        let mut tls = rustls::StreamOwned::new(connection, tcp);
        let tunneled_request = read_tls_http_request(&mut tls);
        tls.write_all(response).unwrap();
        (
            String::from_utf8_lossy(&connect_request).into_owned(),
            String::from_utf8_lossy(&tunneled_request).into_owned(),
        )
    });
    (port, handle)
}

fn read_plain_http_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 512];
    loop {
        let size = stream.read(&mut buffer).unwrap();
        request.extend_from_slice(&buffer[..size]);
        if size == 0 || request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    request
}

fn read_tls_http_request(
    stream: &mut rustls::StreamOwned<rustls::ServerConnection, std::net::TcpStream>,
) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 512];
    loop {
        let size = stream.read(&mut buffer).unwrap();
        request.extend_from_slice(&buffer[..size]);
        if size == 0 || request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    request
}

#[test]
fn http_client_get_uses_configured_http_proxy_for_plain_http() {
    let (proxy_port, proxy) =
        spawn_proxy("HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nproxied");
    let proxy_config = HttpProxyConfig::new("127.0.0.1", proxy_port)
        .unwrap()
        .with_authorization("Basic dXNlcjpwYXNz");
    let http_config = RuntimeHttpConfig::default()
        .with_timeouts(
            Duration::from_secs(2),
            Duration::from_secs(2),
            Duration::from_secs(2),
        )
        .with_proxy(proxy_config)
        .unwrap();

    let output = run_with_http_config(
        r#"
import "std.net.http.client" as client

let response = client.get("http://example.test/path?q=1")
print(response.get("status").to_string())
print(response.get("body"))
"#,
        http_config,
    )
    .unwrap();
    let request = proxy.join().unwrap();

    assert_eq!(output, vec!["200", "proxied"]);
    assert!(
        request.starts_with("GET http://example.test:80/path?q=1 HTTP/1.1\r\n"),
        "{request}"
    );
    assert!(request.contains("\r\nHost: example.test\r\n"), "{request}");
    assert!(
        request.contains("\r\nProxy-Authorization: Basic dXNlcjpwYXNz\r\n"),
        "{request}"
    );
}

#[test]
fn https_client_uses_configured_http_proxy_connect_tunnel() {
    let (roots, server_config) = https_material();
    let (proxy_port, proxy) = spawn_https_tunnel_proxy(
        b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\ntunneled",
        server_config,
    );
    let proxy_config = HttpProxyConfig::new("127.0.0.1", proxy_port)
        .unwrap()
        .with_authorization("Basic dXNlcjpwYXNz");
    let http_config = RuntimeHttpConfig::default()
        .with_timeouts(
            Duration::from_secs(2),
            Duration::from_secs(2),
            Duration::from_secs(2),
        )
        .with_proxy(proxy_config)
        .unwrap();

    let output = run_with_http_config_and_roots(
        &format!(
            r#"
import "std.net.http.client" as client

let response = client.get("https://localhost:{}/secure?x=1")
print(response.get("status").to_string())
print(response.get("body"))
"#,
            proxy_port + 1
        ),
        http_config,
        roots,
    )
    .unwrap();
    let (connect_request, tunneled_request) = proxy.join().unwrap();

    assert_eq!(output, vec!["200", "tunneled"]);
    assert!(
        connect_request.starts_with(&format!(
            "CONNECT localhost:{} HTTP/1.1\r\n",
            proxy_port + 1
        )),
        "{connect_request}"
    );
    assert!(
        connect_request.contains("\r\nProxy-Authorization: Basic dXNlcjpwYXNz\r\n"),
        "{connect_request}"
    );
    assert!(
        tunneled_request.starts_with("GET /secure?x=1 HTTP/1.1\r\n"),
        "{tunneled_request}"
    );
    assert!(
        tunneled_request.contains(&format!("\r\nHost: localhost:{}\r\n", proxy_port + 1)),
        "{tunneled_request}"
    );
    assert!(
        !tunneled_request.contains("Proxy-Authorization"),
        "{tunneled_request}"
    );
}
