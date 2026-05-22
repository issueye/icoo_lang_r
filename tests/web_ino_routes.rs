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

fn response_body(response: &str) -> &str {
    response.split("\r\n\r\n").nth(1).unwrap_or("")
}

#[test]
fn supports_std_web_ino_route_params_and_query_params() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_routes");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn user(req: Map<String, Any>, res: WebInoResponse):
    let params = req.get("params")
    let query = req.get("query_params")
    res.send("id=" + params.get("id") + ";name=" + query.get("name") + ";city=" + query.get("city"))

fn exact(req: Map<String, Any>, res: WebInoResponse):
    res.send("exact")

app.get("/users/:id", user)
app.get("/users/me", exact)
app.listen("127.0.0.1", {}, 2)
"#,
            port
        ),
    )
    .unwrap();

    let server_handle =
        thread::spawn(move || icoo_lang_r::run_file(server_path).map_err(|err| err.to_string()));
    thread::sleep(Duration::from_millis(150));

    let parameterized = http_get(
        port,
        "/users/alice%20smith?name=Ada+Lovelace&city=Shenzhen%20CN",
    )
    .unwrap();
    let exact = http_get(port, "/users/me").unwrap();

    assert!(
        parameterized.starts_with("HTTP/1.1 200 OK"),
        "{parameterized}"
    );
    assert_eq!(
        response_body(&parameterized),
        "id=alice smith;name=Ada Lovelace;city=Shenzhen CN"
    );
    assert!(exact.starts_with("HTTP/1.1 200 OK"), "{exact}");
    assert_eq!(response_body(&exact), "exact");

    server_handle.join().unwrap().unwrap();
}

#[test]
fn returns_not_found_for_unmatched_web_ino_routes() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_routes_404");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn home(req: Map<String, Any>, res: WebInoResponse):
    res.send("home")

app.get("/", home)
app.listen_once("127.0.0.1", {})
"#,
            port
        ),
    )
    .unwrap();

    let server_handle =
        thread::spawn(move || icoo_lang_r::run_file(server_path).map_err(|err| err.to_string()));
    thread::sleep(Duration::from_millis(150));

    let missing = http_get(port, "/missing").unwrap();

    assert!(missing.starts_with("HTTP/1.1 404 Not Found"), "{missing}");
    assert_eq!(response_body(&missing), "Not Found");

    server_handle.join().unwrap().unwrap();
}
