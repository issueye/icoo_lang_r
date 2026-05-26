use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http_get(port: u16, path: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|err| err.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| err.to_string())?;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        path
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| err.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|err| err.to_string())?;
    Ok(response)
}

#[test]
#[ignore = "manual performance test: run with `cargo test --test web_ino_perf -- --ignored --nocapture`"]
fn web_ino_handles_concurrent_requests_perf() {
    let request_count = 256;
    let workers = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let dir = PathBuf::from("target/icoo_perf_tests/web_ino");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn ping(req: Map<String, Any>, res: WebInoResponse) {{
    res.send("ok:" + req.get("path"))

}}
app.get("/ping", ping)
app.get("/route-0", ping)
app.get("/route-1", ping)
app.get("/route-2", ping)
app.get("/route-3", ping)
app.get("/route-4", ping)
app.get("/route-5", ping)
app.get("/route-6", ping)
app.get("/route-7", ping)
app.get("/route-8", ping)
app.get("/route-9", ping)
app.listen_with_workers("127.0.0.1", {}, {}, {})
"#,
            port, request_count, workers
        ),
    )
    .unwrap();

    let server_handle =
        thread::spawn(move || icoo_lang_r::run_file(server_path).map_err(|err| err.to_string()));
    thread::sleep(Duration::from_millis(150));

    let start_gate = Arc::new(Barrier::new(request_count));
    let started_at = Instant::now();
    let mut clients = Vec::new();
    for _ in 0..request_count {
        let start_gate = start_gate.clone();
        clients.push(thread::spawn(move || {
            start_gate.wait();
            http_get(port, "/ping")
        }));
    }

    let mut successes = 0;
    for client in clients {
        let response = client.join().unwrap().unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.ends_with("ok:/ping"), "{response}");
        successes += 1;
    }
    let elapsed = started_at.elapsed();
    server_handle.join().unwrap().unwrap();

    let throughput = successes as f64 / elapsed.as_secs_f64();
    eprintln!(
        "web_ino_perf: requests={}, workers={}, elapsed_ms={}, throughput_rps={:.2}",
        successes,
        workers,
        elapsed.as_millis(),
        throughput
    );
    assert_eq!(successes, request_count);
}
