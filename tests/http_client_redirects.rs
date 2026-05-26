use icoo_lang_r::{run_source_with_output_and_http_config, RuntimeHttpConfig};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

fn run_with_config(source: &str, http_config: RuntimeHttpConfig) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    run_source_with_output_and_http_config(
        source,
        move |line| {
            captured.borrow_mut().push(line);
        },
        http_config,
    )
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
}

fn spawn_redirect_server(expected_requests: usize) -> (u16, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut requests = Vec::new();
        while requests.len() < expected_requests && Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(accepted) => accepted,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(err) => panic!("failed to accept test HTTP connection: {err}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();

            let request = read_http_request(&mut stream);
            let path = request_path(&request);
            requests.push(path.clone());

            let response = match path.as_str() {
                "/start" => "HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 8\r\nConnection: close\r\n\r\nredirect",
                "/loop-a" => "HTTP/1.1 302 Found\r\nLocation: /loop-b\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                "/loop-b" => "HTTP/1.1 302 Found\r\nLocation: /loop-c\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                "/final" => "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nfinal",
                _ => "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found",
            };
            stream.write_all(response.as_bytes()).unwrap();
        }
        requests
    });
    (port, handle)
}

fn read_http_request(stream: &mut std::net::TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 512];
    loop {
        let size = stream.read(&mut buffer).unwrap();
        request.extend_from_slice(&buffer[..size]);
        if size == 0 || request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&request).into_owned()
}

fn request_path(request: &str) -> String {
    request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("")
        .to_string()
}

#[test]
fn http_client_does_not_follow_302_by_default() {
    let (port, server) = spawn_redirect_server(1);

    let result = run_with_config(
        &format!(
            r#"
import "std.net.http.client" as client

let response = client.get("http://127.0.0.1:{}/start")
print(response.get("status").to_string())
print(response.get("body"))
"#,
            port
        ),
        RuntimeHttpConfig::default(),
    );
    let requests = server.join().unwrap();
    let output = result.unwrap();

    assert_eq!(output, vec!["302", "redirect"]);
    assert_eq!(requests, vec!["/start"]);
}

#[test]
fn http_client_follows_single_redirect_when_configured() {
    let (port, server) = spawn_redirect_server(2);

    let result = run_with_config(
        &format!(
            r#"
import "std.net.http.client" as client

let response = client.get("http://127.0.0.1:{}/start")
print(response.get("status").to_string())
print(response.get("body"))
"#,
            port
        ),
        RuntimeHttpConfig::default().with_max_redirects(1),
    );
    let requests = server.join().unwrap();
    let output = result.unwrap();

    assert_eq!(output, vec!["200", "final"]);
    assert_eq!(requests, vec!["/start", "/final"]);
}

#[test]
fn http_client_errors_when_redirects_exceed_configured_limit() {
    let (port, server) = spawn_redirect_server(2);

    let result = run_with_config(
        &format!(
            r#"
import "std.net.http.client" as client
client.get("http://127.0.0.1:{}/loop-a")
"#,
            port
        ),
        RuntimeHttpConfig::default().with_max_redirects(1),
    );
    let requests = server.join().unwrap();
    let err = result.unwrap_err();

    assert!(err.contains("maximum redirect count exceeded"), "{err}");
    assert_eq!(requests, vec!["/loop-a", "/loop-b"]);
}
