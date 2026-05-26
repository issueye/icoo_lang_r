use icoo_lang_r::{PermissionRule, RuntimePermissions};
use std::cell::RefCell;
use std::path::MAIN_SEPARATOR;
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

fn quoted(value: &str) -> String {
    format!("{value:?}")
}

#[test]
fn std_path_runs_stable_host_path_operations() {
    let sep = MAIN_SEPARATOR;
    let file_path = format!("alpha{sep}beta{sep}file.txt");
    let normalize_input = format!("alpha{sep}.{sep}beta{sep}..{sep}gamma");
    let absolute_path = if cfg!(windows) {
        "C:\\alpha\\beta"
    } else {
        "/alpha/beta"
    };
    let source = format!(
        r#"
import "std.path" as path

print(path.join("alpha", "beta"))
print(path.normalize({normalize_input}))
print(path.dirname({file_path}))
print(path.basename({file_path}))
print(path.extension({file_path}))
print(path.is_absolute({absolute_path}).to_string())
print(path.is_absolute("alpha").to_string())
"#,
        normalize_input = quoted(&normalize_input),
        file_path = quoted(&file_path),
        absolute_path = quoted(absolute_path),
    );

    let output = run(&source).unwrap();

    assert_eq!(
        output,
        vec![
            format!("alpha{sep}beta"),
            format!("alpha{sep}gamma"),
            format!("alpha{sep}beta"),
            "file.txt".to_string(),
            "txt".to_string(),
            "true".to_string(),
            "false".to_string(),
        ]
    );
}

#[test]
fn std_path_normalize_preserves_relative_parent_segments() {
    let sep = MAIN_SEPARATOR;
    let input = format!("..{sep}..{sep}alpha{sep}.{sep}beta{sep}..");
    let source = format!(
        r#"
import "std.path" as path

print(path.normalize({input}))
"#,
        input = quoted(&input),
    );

    let output = run(&source).unwrap();

    assert_eq!(output, vec![format!("..{sep}..{sep}alpha")]);
}

#[cfg(windows)]
#[test]
fn std_path_handles_windows_native_paths() {
    let source = r#"
import "std.path" as path

print(path.join("C:\\tmp", "folder\\file.txt"))
print(path.normalize("C:\\tmp\\.\\folder\\..\\file.txt"))
print(path.dirname("C:\\tmp\\folder\\file.txt"))
print(path.basename("C:\\tmp\\folder\\file.txt"))
print(path.extension("C:\\tmp\\folder\\file.txt"))
print(path.is_absolute("C:\\tmp\\folder\\file.txt").to_string())
"#;

    let output = run(source).unwrap();

    assert_eq!(
        output,
        vec![
            "C:\\tmp\\folder\\file.txt",
            "C:\\tmp\\file.txt",
            "C:\\tmp\\folder",
            "file.txt",
            "txt",
            "true",
        ]
    );
}

#[test]
fn std_path_is_pure_and_runs_with_denied_permissions() {
    let permissions = RuntimePermissions {
        fs_read: PermissionRule::DenyAll,
        fs_write: PermissionRule::DenyAll,
        fs_list: PermissionRule::DenyAll,
        env_read: PermissionRule::DenyAll,
        os_info: PermissionRule::DenyAll,
        net_connect: PermissionRule::DenyAll,
        net_listen: PermissionRule::DenyAll,
        process_exec: PermissionRule::DenyAll,
    };

    icoo_lang_r::run_source_with_permissions(
        r#"
import "std.path" as path

let joined = path.join("alpha", "beta")
let normalized = path.normalize("alpha/./beta/..")
let parent = path.dirname("alpha/beta/file.txt")
let name = path.basename("alpha/beta/file.txt")
let ext = path.extension("alpha/beta/file.txt")
let absolute = path.is_absolute("alpha/beta")
"#,
        permissions,
    )
    .unwrap();
}

#[test]
fn typechecker_uses_std_path_metadata_for_argument_checks() {
    let err = run(r#"
import "std.path" as path
path.join(1, "child")
"#)
    .unwrap_err();

    assert!(err.contains("type error: expected String for argument 1 but got Int"));
}
