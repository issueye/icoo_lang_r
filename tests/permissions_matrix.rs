use icoo_lang_r::lexer::token::Span;
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

fn run_with_permissions(source: &str, permissions: RuntimePermissions) -> Result<(), String> {
    icoo_lang_r::run_source_with_permissions(source, permissions).map_err(|err| err.to_string())
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

fn test_span() -> Span {
    Span::new(1, 1, 0, 1)
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
fn allow_list_rules_do_not_grant_coarse_capability_queries() {
    let permissions = RuntimePermissions {
        fs_read: PermissionRule::allow_paths(["target/icoo_permissions_matrix/read"]),
        fs_write: PermissionRule::allow_paths(["target/icoo_permissions_matrix/write"]),
        fs_list: PermissionRule::allow_paths(["target/icoo_permissions_matrix/list"]),
        env_read: PermissionRule::allow_env_keys(["ICOO_ALLOWED"]),
        os_info: PermissionRule::AllowAll,
        net_connect: PermissionRule::allow_net_endpoints([("example.com", 80)]),
        net_listen: PermissionRule::allow_net_endpoints([("127.0.0.1", 8080)]),
    };

    assert!(!permissions.can_read_fs());
    assert!(!permissions.can_write_fs());
    assert!(!permissions.can_list_fs());
    assert!(!permissions.can_read_env());
    assert!(permissions.can_read_os_info());
    assert!(!permissions.can_connect_net());
    assert!(!permissions.can_listen_net());
}

#[test]
fn path_allow_lists_allow_matching_roots_and_report_denied_path() {
    let span = test_span();
    let mut permissions = RuntimePermissions::deny_all();
    permissions.fs_read = PermissionRule::allow_paths(["target/icoo_permissions_matrix/allowed"]);
    permissions.fs_write = PermissionRule::allow_paths(["target/icoo_permissions_matrix/allowed"]);
    permissions.fs_list = PermissionRule::allow_paths(["target/icoo_permissions_matrix/allowed"]);

    permissions
        .check_fs_read_path("target/icoo_permissions_matrix/allowed/file.txt", span)
        .unwrap();
    permissions
        .check_fs_write_path("target/icoo_permissions_matrix/allowed/file.txt", span)
        .unwrap();
    permissions
        .check_fs_list_path("target/icoo_permissions_matrix/allowed", span)
        .unwrap();

    let err = permissions
        .check_fs_read_path("target/icoo_permissions_matrix/blocked/file.txt", span)
        .unwrap_err()
        .to_string();
    assert!(err.contains(
        "permission denied: fs.read path 'target/icoo_permissions_matrix/blocked/file.txt'"
    ));

    let escaped_err = permissions
        .check_fs_write_path(
            "target/icoo_permissions_matrix/allowed/../blocked/file.txt",
            span,
        )
        .unwrap_err()
        .to_string();
    assert!(escaped_err.contains(
        "permission denied: fs.write path 'target/icoo_permissions_matrix/allowed/../blocked/file.txt'"
    ));
}

#[test]
fn env_key_allow_lists_allow_matching_keys_and_report_denied_key() {
    let span = test_span();
    let mut permissions = RuntimePermissions::deny_all();
    permissions.env_read = PermissionRule::allow_env_keys(["ICOO_ALLOWED_KEY"]);

    permissions
        .check_env_read_key("ICOO_ALLOWED_KEY", span)
        .unwrap();

    let err = permissions
        .check_env_read_key("ICOO_BLOCKED_KEY", span)
        .unwrap_err()
        .to_string();
    assert!(err.contains("permission denied: env.read key 'ICOO_BLOCKED_KEY'"));
}

#[test]
fn resource_allow_lists_are_enforced_by_native_modules() {
    fs::create_dir_all("target/icoo_permissions_matrix/allowed").unwrap();
    fs::create_dir_all("target/icoo_permissions_matrix/blocked").unwrap();
    fs::write("target/icoo_permissions_matrix/allowed/read.txt", "allowed").unwrap();
    fs::write("target/icoo_permissions_matrix/blocked/read.txt", "blocked").unwrap();

    let mut permissions = RuntimePermissions::deny_all();
    permissions.fs_read = PermissionRule::allow_paths(["target/icoo_permissions_matrix/allowed"]);
    permissions.fs_write = PermissionRule::allow_paths(["target/icoo_permissions_matrix/allowed"]);
    permissions.fs_list = PermissionRule::allow_paths(["target/icoo_permissions_matrix/allowed"]);
    permissions.env_read = PermissionRule::allow_env_keys(["ICOO_ALLOWED_KEY"]);
    permissions.os_info = PermissionRule::AllowAll;
    permissions.net_connect = PermissionRule::allow_net_endpoints([("127.0.0.1", 18080)]);
    permissions.net_listen = PermissionRule::allow_net_endpoints([("127.0.0.1", 18081)]);

    run_with_permissions(
        r#"import "std.io.fs" as fs
import "std.env" as env
import "std.os" as os

fs.exists("target/icoo_permissions_matrix/allowed/read.txt")
fs.is_file("target/icoo_permissions_matrix/allowed/read.txt")
fs.is_dir("target/icoo_permissions_matrix/allowed")
fs.read_text("target/icoo_permissions_matrix/allowed/read.txt")
fs.write_text("target/icoo_permissions_matrix/allowed/write.txt", "ok")
fs.append_text("target/icoo_permissions_matrix/allowed/write.txt", "!")
fs.list_dir("target/icoo_permissions_matrix/allowed")
env.has("ICOO_ALLOWED_KEY")
env.get("ICOO_ALLOWED_KEY")
os.has_env("ICOO_ALLOWED_KEY")
os.get_env("ICOO_ALLOWED_KEY")"#,
        permissions.clone(),
    )
    .unwrap();

    let fs_err = run_with_permissions(
        r#"import "std.io.fs" as fs
fs.read_text("target/icoo_permissions_matrix/blocked/read.txt")"#,
        permissions.clone(),
    )
    .unwrap_err();
    assert!(fs_err.contains(
        "permission denied: fs.read path 'target/icoo_permissions_matrix/blocked/read.txt'"
    ));

    let env_err = run_with_permissions(
        r#"import "std.env" as env
env.has("ICOO_BLOCKED_KEY")"#,
        permissions.clone(),
    )
    .unwrap_err();
    assert!(env_err.contains("permission denied: env.read key 'ICOO_BLOCKED_KEY'"));

    let os_env_err = run_with_permissions(
        r#"import "std.os" as os
os.get_env("ICOO_BLOCKED_KEY")"#,
        permissions.clone(),
    )
    .unwrap_err();
    assert!(os_env_err.contains("permission denied: env.read key 'ICOO_BLOCKED_KEY'"));

    let client_err = run_with_permissions(
        r#"import "std.net.http.client" as client
client.get("http://127.0.0.1:18082/")"#,
        permissions.clone(),
    )
    .unwrap_err();
    assert!(client_err.contains("permission denied: net.connect endpoint '127.0.0.1:18082'"));

    let https_default_port_err = run_with_permissions(
        r#"import "std.net.http.client" as client
client.get("https://example.invalid/")"#,
        permissions.clone(),
    )
    .unwrap_err();
    assert!(https_default_port_err
        .contains("permission denied: net.connect endpoint 'example.invalid:443'"));

    let https_explicit_port_err = run_with_permissions(
        r#"import "std.net.http.client" as client
client.get("https://example.invalid:8443/")"#,
        permissions.clone(),
    )
    .unwrap_err();
    assert!(https_explicit_port_err
        .contains("permission denied: net.connect endpoint 'example.invalid:8443'"));

    let server_err = run_with_permissions(
        r#"import "std.net.http.server" as server
server.serve_once("127.0.0.1", 18082, "body")"#,
        permissions.clone(),
    )
    .unwrap_err();
    assert!(server_err.contains("permission denied: net.listen endpoint '127.0.0.1:18082'"));

    let web_err = run_with_permissions(
        r#"import "std.web.ino" as ino
let app = ino.App()
app.listen_once("127.0.0.1", 18082)"#,
        permissions,
    )
    .unwrap_err();
    assert!(web_err.contains("permission denied: net.listen endpoint '127.0.0.1:18082'"));
}

#[test]
fn net_endpoint_allow_lists_allow_matching_endpoints_and_report_denied_endpoint() {
    let span = test_span();
    let mut permissions = RuntimePermissions::deny_all();
    permissions.net_connect = PermissionRule::allow_net_endpoints([("example.com", 80)]);
    permissions.net_listen = PermissionRule::allow_net_endpoints([("127.0.0.1", 8080)]);

    permissions
        .check_net_connect_endpoint("example.com", 80, span)
        .unwrap();
    permissions
        .check_net_listen_endpoint("127.0.0.1", 8080, span)
        .unwrap();

    let connect_err = permissions
        .check_net_connect_endpoint("example.com", 443, span)
        .unwrap_err()
        .to_string();
    assert!(connect_err.contains("permission denied: net.connect endpoint 'example.com:443'"));

    let listen_err = permissions
        .check_net_listen_endpoint("127.0.0.1", 9090, span)
        .unwrap_err()
        .to_string();
    assert!(listen_err.contains("permission denied: net.listen endpoint '127.0.0.1:9090'"));
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
fs.read_bytes_range("target/icoo_permissions_matrix/missing.txt", 0, 1)"#,
            "permission denied: fs.read",
        ),
        (
            r#"import "std.io.fs" as fs
fs.metadata("target/icoo_permissions_matrix/missing.txt")"#,
            "permission denied: fs.read",
        ),
        (
            r#"import "std.io.fs" as fs
fs.symlink_metadata("target/icoo_permissions_matrix/missing.txt")"#,
            "permission denied: fs.read",
        ),
        (
            r#"import "std.io.fs" as fs
fs.canonicalize("target/icoo_permissions_matrix/missing.txt")"#,
            "permission denied: fs.read",
        ),
        (
            r#"import "std.io.fs" as fs
fs.read_link("target/icoo_permissions_matrix/missing.txt")"#,
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
fs.write_text_atomic("target/icoo_permissions_matrix/denied.txt", "nope")"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.write_bytes_atomic("target/icoo_permissions_matrix/denied.txt", "nope".to_bytes())"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.write_bytes_at("target/icoo_permissions_matrix/denied.txt", 0, "nope".to_bytes())"#,
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
        (
            r#"import "std.io.fs" as fs
fs.mkdir("target/icoo_permissions_matrix/new")"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.mkdir_all("target/icoo_permissions_matrix/new/deep")"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.remove_file("target/icoo_permissions_matrix/denied.txt")"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.remove_dir("target/icoo_permissions_matrix/denied")"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.remove_dir_all("target/icoo_permissions_matrix/denied")"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.rename("target/icoo_permissions_matrix/a", "target/icoo_permissions_matrix/b")"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.copy("target/icoo_permissions_matrix/a", "target/icoo_permissions_matrix/b")"#,
            "permission denied: fs.read",
        ),
        (
            r#"import "std.io.fs" as fs
fs.create_temp_file("target/icoo_permissions_matrix", "tmp")"#,
            "permission denied: fs.write",
        ),
        (
            r#"import "std.io.fs" as fs
fs.create_symlink_file("target/icoo_permissions_matrix/a", "target/icoo_permissions_matrix/b")"#,
            "permission denied: fs.read",
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

fn download(req: Map<String, Any>, res: WebInoResponse) {{
    res.download("{}")

}}
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
