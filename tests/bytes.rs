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
fn supports_bytes_methods_and_string_encoding() {
    let output = run(r#"
let data: Bytes = "hello".to_bytes()
print(data.type_name())
print(data.len().to_string())
print(data.is_empty().to_string())
print(data.to_string())
print(data.to_string("hex"))
print(data.to_hex())
print(data.slice(1, 4).to_string())
print(data.concat("!".to_bytes()).to_string())
print(data.equals("hello".to_bytes()).to_string())
print(data.equals("HELLO".to_bytes()).to_string())
print(data.to_string("lossy"))
"#)
    .unwrap();

    assert_eq!(
        output,
        vec![
            "Bytes",
            "5",
            "false",
            "hello",
            "68656c6c6f",
            "68656c6c6f",
            "ell",
            "hello!",
            "true",
            "false",
            "hello",
        ]
    );
}

#[test]
fn supports_std_io_fs_binary_round_trip() {
    fs::create_dir_all("target/icoo_bytes").unwrap();
    let path = "target/icoo_bytes/round_trip.bin";
    let output = run(r#"
import "std.io.fs" as fs

let path = "target/icoo_bytes/round_trip.bin"
fs.write_bytes(path, "ab".to_bytes())
fs.append_bytes(path, "cd".to_bytes())
let data: Bytes = fs.read_bytes(path)
print(data.len().to_string())
print(data.to_string())
print(data.to_hex())
print(fs.read_text(path))
"#)
    .unwrap();

    assert_eq!(output, vec!["4", "abcd", "61626364", "abcd"]);
    assert_eq!(fs::read(path).unwrap(), b"abcd");
}

#[test]
fn bytes_display_is_safe_for_printing() {
    let output = run(r#"
print("abc".to_bytes())
"#)
    .unwrap();

    assert_eq!(output, vec!["<bytes len=3 hex=616263>"]);
}

#[test]
fn rejects_invalid_bytes_usage() {
    let err = run(r#"
let data = "abc".to_bytes()
data.slice(-1)
"#)
    .unwrap_err();
    assert!(err.contains("byte index must be non-negative"));

    let err = run(r#"
let data = "abc".to_bytes()
data.concat("not bytes")
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Bytes for argument 1 but got String"));

    let err = run(r#"
import "std.io.fs" as fs
fs.write_bytes("target/icoo_bytes/bad.bin", "not bytes")
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Bytes for argument 2 but got String"));
}

#[test]
fn rejects_implicit_json_encoding_for_bytes() {
    let err = run(r#"
import "std.json" as json
json.stringify({"data": "abc".to_bytes()})
"#)
    .unwrap_err();

    assert!(err.contains("Bytes cannot be represented as JSON"));
}
