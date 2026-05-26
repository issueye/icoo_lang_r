use std::cell::RefCell;
use std::fs;
use std::rc::Rc;

fn run(source: &str) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    icoo_lang_r::run_source_with_output(source, move |line| {
        captured.borrow_mut().push(line);
    })
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
}

#[test]
fn native_module_registry_matches_current_standard_library_surface() {
    let modules: Vec<(&str, &str, &str, Vec<&str>)> = icoo_lang_r::native_modules::SPECS
        .iter()
        .map(|spec| {
            (
                spec.import_path,
                spec.kind,
                spec.type_name,
                spec.methods.iter().map(|method| method.name).collect(),
            )
        })
        .collect();

    assert_eq!(
        modules.iter().map(|module| module.0).collect::<Vec<_>>(),
        vec![
            "Bytes",
            "Buffer",
            "std.math",
            "std.time",
            "std.json",
            "std.yaml",
            "std.toml",
            "std.env",
            "std.io",
            "std.io.fs",
            "std.os",
            "std.net.http.client",
            "std.net.http.server",
            "std.net.ws.client",
            "std.net.ws.server",
            "std.net.sse.client",
            "std.net.sse.server",
            "std.net.socket.client",
            "std.net.socket.server",
            "std.web.ino",
        ]
    );

    for (import_path, kind, type_name, methods) in modules {
        assert!(import_path.starts_with("std.") || matches!(import_path, "Bytes" | "Buffer"));
        assert!(!kind.is_empty(), "{import_path} missing kind");
        assert!(!type_name.is_empty(), "{import_path} missing type name");
        assert!(!methods.is_empty(), "{import_path} missing methods");
    }
}

#[test]
fn native_module_method_specs_cover_registry_and_lookup() {
    use icoo_lang_r::native_modules::{NativeAritySpec, SPECS};

    for spec in SPECS {
        for method in spec.methods {
            assert!(
                icoo_lang_r::native_modules::has_method(spec.import_path, method.name),
                "{}.{} missing import-path lookup",
                spec.import_path,
                method.name
            );
            assert!(
                icoo_lang_r::native_modules::has_method(spec.kind, method.name),
                "{}.{} missing kind lookup",
                spec.kind,
                method.name
            );
            assert!(
                icoo_lang_r::native_modules::method_spec_for_type(spec.type_name, method.name)
                    .is_some(),
                "{}.{} missing type-name lookup",
                spec.type_name,
                method.name
            );

            let fixed_capacity = match method.arity {
                NativeAritySpec::Exact(expected) => expected,
                NativeAritySpec::Range { max, .. } => max,
                NativeAritySpec::AtLeast(_) => method.params.len(),
            };
            assert!(
                method.params.len() >= fixed_capacity,
                "{}.{} has fewer param specs than arity",
                spec.import_path,
                method.name
            );
            assert!(
                !method.return_type.is_empty(),
                "{}.{} missing return type",
                spec.import_path,
                method.name
            );
        }
    }

    assert!(icoo_lang_r::native_modules::has_method(
        "std.io.fs",
        "read_text"
    ));
    assert!(icoo_lang_r::native_modules::has_method(
        "std.net.http.client",
        "stream_get"
    ));
    assert!(icoo_lang_r::native_modules::has_method(
        "std.net.http.client",
        "get_bytes"
    ));
    assert!(icoo_lang_r::native_modules::has_method(
        "std.net.ws.client",
        "send_text"
    ));
    assert!(icoo_lang_r::native_modules::has_method(
        "std.net.sse.server",
        "serve_once"
    ));
    assert!(icoo_lang_r::native_modules::has_method(
        "std.net.socket.client",
        "send_bytes"
    ));
    assert!(icoo_lang_r::native_modules::has_method(
        "std.web.ino",
        "create"
    ));
    assert!(icoo_lang_r::native_modules::has_method("std.os", "pid"));
    assert!(!icoo_lang_r::native_modules::has_method(
        "std.io.fs",
        "missing"
    ));
}

#[test]
fn typechecker_uses_native_module_metadata_for_argument_checks() {
    let err = run(r#"
import "std.io.fs" as fs
fs.read_text(1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));

    let err = run(r#"
import "std.net.http.client" as client
client.get(1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));

    let err = run(r#"
import "std.net.ws.client" as ws
ws.send_text(1, "hello")
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));

    let err = run(r#"
import "std.os" as os
os.has_env(1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));

    let err = run(r#"
import "std.web.ino" as ino
ino.create(1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: method expected 0 arguments but got 1"));
}

#[test]
fn imports_global_native_modules_and_runs_basic_methods() {
    let output = run(r#"
import "std.math" as std_math
import "std.time" as std_time
import "std.json" as std_json
import "std.env" as std_env

print(std_math.max(4, 7).to_string())
print(std_math.floor(3.8).to_string())
print((std_time.now_ms() > 0).to_string())

let encoded = std_json.stringify({"name": "Icoo", "items": [1, 2]})
let decoded = std_json.parse(encoded)
print(decoded.get("name"))
print(decoded.get("items").at(1).to_string())

print(std_env.has("__ICOO_NATIVE_MATRIX_MISSING__").to_string())
print(std_env.get("__ICOO_NATIVE_MATRIX_MISSING__").to_string())
"#)
    .unwrap();

    assert_eq!(output, vec!["7", "3", "true", "Icoo", "2", "false", "nil"]);
}

#[test]
fn imports_data_format_modules_and_round_trips_simple_values() {
    let output = run(r#"
import "std.yaml" as yaml
import "std.toml" as toml

let yaml_data = yaml.parse("name: Icoo\nactive: true\n")
print(yaml_data.get("name"))
print(yaml.stringify({"ok": true}).contains("ok: true").to_string())

let toml_data = toml.parse("name = \"Icoo\"\nactive = true\n")
print(toml_data.get("active").to_string())
print(toml.stringify({"name": "Icoo"}).contains("name = \"Icoo\"").to_string())
"#)
    .unwrap();

    assert_eq!(output, vec!["Icoo", "true", "true", "true"]);
}

#[test]
fn imports_io_fs_and_os_modules_and_runs_local_methods() {
    fs::create_dir_all("target/icoo_native_modules_matrix").unwrap();

    let output = run(r#"
import "std.io" as io
import "std.io.fs" as fs
import "std.os" as os

let path = "target/icoo_native_modules_matrix/fs.txt"
io.print("io-ok")
fs.write_text(path, "hello")
fs.append_text(path, " fs")
let bytes_path = "target/icoo_native_modules_matrix/fs.bin"
fs.write_bytes(bytes_path, "io".to_bytes())
fs.append_bytes(bytes_path, "-bytes".to_bytes())

print(fs.exists(path).to_string())
print(fs.is_file(path).to_string())
print(fs.is_dir("target/icoo_native_modules_matrix").to_string())
print(fs.read_text(path))
print(fs.read_bytes(bytes_path).to_string())
print(fs.list_dir("target/icoo_native_modules_matrix").includes("fs.txt").to_string())

print(os.name().len() > 0)
print(os.family().len() > 0)
print(os.arch().len() > 0)
print(os.pid() > 0)
print(os.has_env("__ICOO_NATIVE_MATRIX_MISSING__").to_string())
"#)
    .unwrap();

    assert_eq!(
        output,
        vec![
            "io-ok", "true", "true", "true", "hello fs", "io-bytes", "true", "true", "true",
            "true", "true", "false",
        ]
    );
}

#[test]
fn imports_http_modules_and_validates_lightweight_methods() {
    let output = run(r#"
import "std.net.http.client" as client
import "std.net.http.server" as server
import "std.net.ws.client" as ws_client
import "std.net.ws.server" as ws_server
import "std.net.sse.client" as sse_client
import "std.net.sse.server" as sse_server
import "std.net.socket.client" as socket_client
import "std.net.socket.server" as socket_server

print(client.to_string().contains("std.net.http.client").to_string())
print(server.to_string().contains("std.net.http.server").to_string())
print(ws_client.to_string().contains("std.net.ws.client").to_string())
print(ws_server.to_string().contains("std.net.ws.server").to_string())
print(sse_client.to_string().contains("std.net.sse.client").to_string())
print(sse_server.to_string().contains("std.net.sse.server").to_string())
print(socket_client.to_string().contains("std.net.socket.client").to_string())
print(socket_server.to_string().contains("std.net.socket.server").to_string())
"#)
    .unwrap();
    assert_eq!(
        output,
        vec!["true", "true", "true", "true", "true", "true", "true", "true",]
    );

    let err = run(r#"
import "std.net.http.client" as client
client.get("https://example.invalid/")
"#)
    .unwrap_err();
    assert!(err.contains("only http:// URLs are supported"));

    let err = run(r#"
import "std.net.http.server" as server
server.serve_once("127.0.0.1", 0, "body")
"#)
    .unwrap_err();
    assert!(err.contains("server port must be between 1 and 65535"));

    let err = run(r#"
import "std.net.socket.server" as server
fn handle(bytes: Bytes):
    "body"
server.serve_once("127.0.0.1", 0, handle)
"#)
    .unwrap_err();
    assert!(err.contains("server port must be between 1 and 65535"));
}

#[test]
fn imports_web_ino_and_registers_basic_routes_without_listening() {
    let output = run(r#"
import "std.web.ino" as ino

let app = ino.create()

fn handler(req: Map<String, Any>, res: WebInoResponse):
    res.send("ok")

print(app.type_name())
print(app.get("/health", handler).type_name())
print(ino.App().type_name())
"#)
    .unwrap();

    assert_eq!(output, vec!["WebInoApp", "WebInoApp", "WebInoApp"]);
}
