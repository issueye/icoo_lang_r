use icoo_lang_r::{PermissionRule, RuntimePermissions};
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
