use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

fn run(source: &str) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    icoo_lang_r::run_source_with_output(source, move |line| {
        captured.borrow_mut().push(line);
    })
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
}

fn test_dir(name: &str) -> String {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = format!("target/icoo_io_fs_production/{}_{}", name, unique);
    let _ = fs::remove_dir_all(&path);
    path
}

#[test]
fn supports_directory_copy_rename_metadata_and_removal() {
    let base = test_dir("paths");

    let output = run(&format!(
        r#"
import "std.io.fs" as fs

let base = "{base}"
let nested = base + "/a/b"
let file = nested + "/file.txt"
let copied = base + "/copy.txt"
let moved = base + "/moved.txt"

fs.mkdir_all(nested)
fs.write_text_atomic(file, "hello")
let meta = fs.metadata(file)
print(fs.exists(file).to_string())
print(fs.is_dir(nested).to_string())
print(meta.get("type"))
print(meta.get("len").to_string())
print(meta.get("readonly").to_string())
print(fs.copy(file, copied).to_string())
fs.rename(copied, moved)
print(fs.read_text(moved))
print(fs.canonicalize(moved).contains("moved.txt").to_string())
fs.remove_file(file)
print(fs.exists(file).to_string())
fs.remove_dir_all(base + "/a")
print(fs.exists(base + "/a").to_string())
"#
    ))
    .unwrap();

    assert_eq!(
        output,
        vec!["true", "true", "file", "5", "false", "5", "hello", "true", "false", "false",]
    );
}

#[test]
fn supports_temp_files_atomic_bytes_and_chunked_io() {
    let base = test_dir("chunks");

    let output = run(&format!(
        r#"
import "std.io.fs" as fs

let base = "{base}"
fs.mkdir_all(base)
let temp = fs.create_temp_file(base, "tmp")
print(fs.exists(temp).to_string())
fs.write_bytes(temp, "abcdef".to_bytes())
fs.write_bytes_at(temp, 2, "XY".to_bytes())
let chunk: Bytes = fs.read_bytes_range(temp, 1, 3)
print(chunk.to_string())
let atomic = base + "/atomic.bin"
fs.write_bytes(atomic, "old".to_bytes())
fs.write_bytes_atomic(atomic, "zz".to_bytes())
print(fs.read_bytes(atomic).to_string())
let atomic_text = base + "/atomic.txt"
fs.write_text(atomic_text, "old")
fs.write_text_atomic(atomic_text, "new")
print(fs.read_text(atomic_text))
"#
    ))
    .unwrap();

    assert_eq!(output, vec!["true", "bXY", "zz", "new"]);
}

#[test]
fn supports_symlink_metadata_when_platform_allows_links() {
    let base = test_dir("links");
    fs::create_dir_all(&base).unwrap();

    let result = run(&format!(
        r#"
import "std.io.fs" as fs

let base = "{base}"
let target = base + "/target.txt"
let link = base + "/link.txt"
fs.write_text(target, "linked")
let target_abs = fs.canonicalize(target)
fs.create_symlink_file(target_abs, link)
let link_meta = fs.symlink_metadata(link)
let target_meta = fs.metadata(link)
print(fs.read_link(link).contains("target.txt").to_string())
print(link_meta.get("type"))
print(link_meta.get("is_symlink").to_string())
print(target_meta.get("type"))
print(fs.read_text(link))
"#
    ));

    match result {
        Ok(output) => assert_eq!(output, vec!["true", "symlink", "true", "file", "linked"]),
        Err(err) if cfg!(windows) && err.contains("create_symlink_file") => {}
        Err(err) => panic!("{err}"),
    }
}
