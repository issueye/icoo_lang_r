use icoo_lang_r::{NetTargetRule, PermissionRule, RuntimePermissions};
use std::fs;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn run_denied(source: &str) -> String {
    icoo_lang_r::run_source_with_permissions(source, RuntimePermissions::deny_all())
        .unwrap_err()
        .to_string()
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn send_get_with_retry(port: u16, path: &str) {
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        path
    );
    for _ in 0..20 {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut stream) => {
                let _ = stream.write_all(request.as_bytes());
                return;
            }
            Err(_) => thread::sleep(Duration::from_millis(25)),
        }
    }
    panic!("test server did not accept connections on port {}", port);
}

fn all_permissions_with(
    fs_read: PermissionRule,
    fs_write: PermissionRule,
    fs_list: PermissionRule,
    env_read: PermissionRule,
    net_connect: PermissionRule,
    net_listen: PermissionRule,
) -> RuntimePermissions {
    RuntimePermissions {
        fs_read,
        fs_write,
        fs_list,
        env_read,
        os_info: PermissionRule::AllowAll,
        net_connect,
        net_listen,
    }
}

fn run_with_permissions(source: &str, permissions: RuntimePermissions) -> Result<(), String> {
    icoo_lang_r::run_source_with_permissions(source, permissions).map_err(|err| err.to_string())
}

fn path_literal(path: impl AsRef<std::path::Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

#[test]
fn allow_all_enables_every_runtime_capability() {
    let permissions = RuntimePermissions::allow_all();

    assert!(permissions.can_read_fs());
    assert!(permissions.can_write_fs());
    assert!(permissions.can_list_fs());
    assert!(permissions.can_read_env());
    assert!(permissions.can_read_os_info());
    assert!(permissions.can_connect_net());
    assert!(permissions.can_listen_net());
}

#[test]
fn deny_all_disables_every_runtime_capability() {
    let permissions = RuntimePermissions::deny_all();

    assert!(!permissions.can_read_fs());
    assert!(!permissions.can_write_fs());
    assert!(!permissions.can_list_fs());
    assert!(!permissions.can_read_env());
    assert!(!permissions.can_read_os_info());
    assert!(!permissions.can_connect_net());
    assert!(!permissions.can_listen_net());
}

#[test]
fn individual_rules_drive_individual_capability_queries() {
    let permissions = RuntimePermissions {
        fs_read: PermissionRule::AllowAll,
        fs_write: PermissionRule::DenyAll,
        fs_list: PermissionRule::AllowAll,
        env_read: PermissionRule::DenyAll,
        os_info: PermissionRule::AllowAll,
        net_connect: PermissionRule::DenyAll,
        net_listen: PermissionRule::AllowAll,
    };

    assert!(permissions.can_read_fs());
    assert!(!permissions.can_write_fs());
    assert!(permissions.can_list_fs());
    assert!(!permissions.can_read_env());
    assert!(permissions.can_read_os_info());
    assert!(!permissions.can_connect_net());
    assert!(permissions.can_listen_net());
}

#[test]
fn default_permissions_preserve_current_allowing_behavior() {
    assert_eq!(
        RuntimePermissions::default(),
        RuntimePermissions::allow_all()
    );
}

#[test]
fn unrestricted_permission_entry_still_runs_existing_pipeline() {
    icoo_lang_r::run_source_with_permissions("let value = 1 + 1", RuntimePermissions::allow_all())
        .unwrap();
}

#[test]
fn restricted_permission_entry_still_runs_pure_script() {
    icoo_lang_r::run_source_with_permissions("let value = 1 + 1", RuntimePermissions::deny_all())
        .unwrap();
}

#[test]
fn deny_all_rejects_std_io_fs_capabilities() {
    fs::create_dir_all("target/icoo_permissions_matrix").unwrap();

    let cases = [
        (
            r#"import "std.io.fs" as fs
fs.exists("target/icoo_permissions_matrix")"#,
            "permission denied: fs.read",
        ),
        (
            r#"import "std.io.fs" as fs
fs.is_file("target/icoo_permissions_matrix/missing.txt")"#,
            "permission denied: fs.read",
        ),
        (
            r#"import "std.io.fs" as fs
fs.is_dir("target/icoo_permissions_matrix")"#,
            "permission denied: fs.read",
        ),
        (
            r#"import "std.io.fs" as fs
fs.read_text("target/icoo_permissions_matrix/missing.txt")"#,
            "permission denied: fs.read",
        ),
        (
            r#"import "std.io.fs" as fs
fs.read_bytes("target/icoo_permissions_matrix/missing.txt")"#,
            "permission denied: fs.read",
        ),
        (
            r#"import "std.io.fs" as fs
fs.write_text("target/icoo_permissions_matrix/denied.txt", "nope")"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.write_bytes("target/icoo_permissions_matrix/denied.txt", "nope".to_bytes())"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.append_text("target/icoo_permissions_matrix/denied.txt", "nope")"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.append_bytes("target/icoo_permissions_matrix/denied.txt", "nope".to_bytes())"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.list_dir("target/icoo_permissions_matrix")"#,
            "permission denied: fs.list",
        ),
    ];

    for (source, message) in cases {
        assert!(run_denied(source).contains(message), "{source}");
    }
}

#[test]
fn deny_all_rejects_std_env_and_os_capabilities() {
    let cases = [
        (
            r#"import "std.env" as env
env.cwd()"#,
            "permission denied: os.info",
        ),
        (
            r#"import "std.env" as env
env.args()"#,
            "permission denied: os.info",
        ),
        (
            r#"import "std.env" as env
env.get("PATH")"#,
            "permission denied: env.read",
        ),
        (
            r#"import "std.env" as env
env.has("PATH")"#,
            "permission denied: env.read",
        ),
        (
            r#"import "std.os" as os
os.name()"#,
            "permission denied: os.info",
        ),
        (
            r#"import "std.os" as os
os.get_env("PATH")"#,
            "permission denied: env.read",
        ),
        (
            r#"import "std.os" as os
os.has_env("PATH")"#,
            "permission denied: env.read",
        ),
    ];

    for (source, message) in cases {
        assert!(run_denied(source).contains(message), "{source}");
    }
}

#[test]
fn deny_all_rejects_net_connect_and_listen_capabilities() {
    let client_err = run_denied(
        r#"import "std.net.http.client" as client
client.get("http://127.0.0.1:1/")"#,
    );
    assert!(client_err.contains("permission denied: net.connect"));

    let server_err = run_denied(
        r#"import "std.net.http.server" as server
server.serve_once("127.0.0.1", 1, "body")"#,
    );
    assert!(server_err.contains("permission denied: net.listen"));

    let socket_client_err = run_denied(
        r#"import "std.net.socket.client" as socket
socket.send("127.0.0.1", 1, "body")"#,
    );
    assert!(socket_client_err.contains("permission denied: net.connect"));

    let socket_server_err = run_denied(
        r#"import "std.net.socket.server" as socket
fn handle(bytes: Bytes):
    "body"
socket.serve_once("127.0.0.1", 1, handle)"#,
    );
    assert!(socket_server_err.contains("permission denied: net.listen"));

    let web_err = run_denied(&format!(
        r#"import "std.web.ino" as ino
let app = ino.App()
app.listen_once("127.0.0.1", {})"#,
        free_port()
    ));
    assert!(web_err.contains("permission denied: net.listen"));
}

#[test]
fn web_ino_download_checks_fs_read_permission_inside_handler() {
    fs::create_dir_all("target/icoo_permissions_matrix").unwrap();
    let path = "target/icoo_permissions_matrix/download.txt";
    fs::write(path, "secret").unwrap();
    let port = free_port();
    let script_path = path.replace('\\', "/");
    let source = format!(
        r#"import "std.web.ino" as ino

let app = ino.App()

fn download(req: Map<String, Any>, res: WebInoResponse):
    res.download("{}")

app.get("/download", download)
app.listen_once("127.0.0.1", {})"#,
        script_path, port
    );
    let permissions = RuntimePermissions {
        fs_read: PermissionRule::DenyAll,
        fs_write: PermissionRule::AllowAll,
        fs_list: PermissionRule::AllowAll,
        env_read: PermissionRule::AllowAll,
        os_info: PermissionRule::AllowAll,
        net_connect: PermissionRule::AllowAll,
        net_listen: PermissionRule::AllowAll,
    };

    let handle = thread::spawn(move || {
        icoo_lang_r::run_source_with_permissions(&source, permissions)
            .unwrap_err()
            .to_string()
    });
    send_get_with_retry(port, "/download");
    let err = handle.join().unwrap();
    assert!(err.contains("permission denied: fs.read"));
}

#[test]
fn path_allow_list_permits_exact_read_target_and_denies_sibling() {
    let dir = std::path::PathBuf::from("target/icoo_permissions_matrix/path_read");
    fs::create_dir_all(&dir).unwrap();
    let allowed = dir.join("allowed.txt");
    let denied = dir.join("denied.txt");
    fs::write(&allowed, "allowed").unwrap();
    fs::write(&denied, "denied").unwrap();

    let permissions = all_permissions_with(
        PermissionRule::allow_paths([allowed.clone()]),
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
    );
    let source = format!(
        r#"import "std.io.fs" as fs
fs.read_text("{}")
fs.read_text("{}")"#,
        path_literal(&allowed),
        path_literal(&denied)
    );

    let err = run_with_permissions(&source, permissions).unwrap_err();
    assert!(err.contains("permission denied: fs.read"), "{err}");
    assert!(err.contains("denied.txt"), "{err}");
}

#[test]
fn path_allow_list_permits_writes_inside_allowed_directory_only() {
    let dir = std::path::PathBuf::from("target/icoo_permissions_matrix/path_write");
    fs::create_dir_all(&dir).unwrap();
    let allowed = dir.join("inside.txt");
    let denied = std::path::PathBuf::from("target/icoo_permissions_matrix/outside.txt");
    let _ = fs::remove_file(&allowed);
    let _ = fs::remove_file(&denied);

    let permissions = all_permissions_with(
        PermissionRule::AllowAll,
        PermissionRule::allow_paths([dir.clone()]),
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
    );
    let source = format!(
        r#"import "std.io.fs" as fs
fs.write_text("{}", "ok")
fs.write_text("{}", "nope")"#,
        path_literal(&allowed),
        path_literal(&denied)
    );

    let err = run_with_permissions(&source, permissions).unwrap_err();
    assert_eq!(fs::read_to_string(&allowed).unwrap(), "ok");
    assert!(!denied.exists());
    assert!(err.contains("permission denied: fs.write"), "{err}");
    assert!(err.contains("outside.txt"), "{err}");
}

#[test]
fn env_allow_list_permits_named_key_only() {
    let permissions = all_permissions_with(
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::allow_env_keys(["ICOO_ALLOWED"]),
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
    );
    let source = r#"import "std.env" as env
env.has("ICOO_ALLOWED")
env.has("ICOO_DENIED")"#;

    let err = run_with_permissions(source, permissions).unwrap_err();
    assert!(err.contains("permission denied: env.read"), "{err}");
    assert!(err.contains("ICOO_DENIED"), "{err}");
}

#[test]
fn net_connect_allow_list_permits_exact_host_port_only() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let allowed_port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0_u8; 512];
        let _ = std::io::Read::read(&mut stream, &mut buffer);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
    });
    let denied_port = free_port();
    let permissions = all_permissions_with(
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::allow_net_targets([NetTargetRule::host_port("127.0.0.1", allowed_port)]),
        PermissionRule::AllowAll,
    );
    let source = format!(
        r#"import "std.net.http.client" as client
client.get("http://127.0.0.1:{}/allowed")
client.get("http://127.0.0.1:{}/denied")"#,
        allowed_port, denied_port
    );

    let err = run_with_permissions(&source, permissions).unwrap_err();
    server.join().unwrap();
    assert!(err.contains("permission denied: net.connect"), "{err}");
    assert!(err.contains(&format!("127.0.0.1:{denied_port}")), "{err}");
}

#[test]
fn net_listen_allow_list_permits_exact_host_port_only() {
    let allowed_port = free_port();
    let permissions = all_permissions_with(
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::allow_net_targets([NetTargetRule::host_port("127.0.0.1", allowed_port)]),
    );
    let allowed_source = format!(
        r#"import "std.net.http.server" as server
server.serve_once("127.0.0.1", {}, "ok")"#,
        allowed_port
    );

    let handle = thread::spawn(move || run_with_permissions(&allowed_source, permissions));
    send_get_with_retry(allowed_port, "/allowed");
    handle.join().unwrap().unwrap();

    let denied_port = free_port();
    let denied_permissions = all_permissions_with(
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::AllowAll,
        PermissionRule::allow_net_targets([NetTargetRule::host_port("127.0.0.1", allowed_port)]),
    );
    let denied_source = format!(
        r#"import "std.net.http.server" as server
server.serve_once("127.0.0.1", {}, "blocked")"#,
        denied_port
    );

    let err = run_with_permissions(&denied_source, denied_permissions).unwrap_err();
    assert!(err.contains("permission denied: net.listen"), "{err}");
    assert!(err.contains(&format!("127.0.0.1:{denied_port}")), "{err}");
}
