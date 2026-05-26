use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn icoo() -> Command {
    Command::new(env!("CARGO_BIN_EXE_icoo"))
}

fn temp_script(name: &str, source: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("icoo_cli_{}_{}", std::process::id(), unique));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, source).unwrap();
    path
}

fn temp_dir(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "icoo_cli_{}_{}_{}",
        name,
        std::process::id(),
        unique
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}

#[test]
fn run_executes_source_file() {
    let script = temp_script(
        "run.icoo",
        r#"
print("ran")
"#,
    );

    let output = icoo().arg("run").arg(script).output().unwrap();

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "ran\n");
}

#[test]
fn check_does_not_execute_source_file() {
    let script = temp_script(
        "check.icoo",
        r#"
print("should not run")
"#,
    );

    let output = icoo().arg("check").arg(script).output().unwrap();

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "");
}

#[test]
fn legacy_file_argument_runs_source_file() {
    let script = temp_script(
        "legacy.icoo",
        r#"
print("legacy")
"#,
    );

    let output = icoo().arg(script).output().unwrap();

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "legacy\n");
}

#[test]
fn init_generates_project_scaffold_and_default_config() {
    let dir = temp_dir("init").join("my_app");

    let init = icoo().arg("init").arg(&dir).output().unwrap();

    assert!(init.status.success(), "stderr: {}", stderr(&init));
    assert!(dir.join("pkg.toml").exists());
    assert!(dir.join("src").join("main.icoo").exists());
    let pkg = fs::read_to_string(dir.join("pkg.toml")).unwrap();
    assert!(pkg.contains("[package]"));
    assert!(pkg.contains("[run]"));
    assert!(pkg.contains("entry = \"src/main.icoo\""));

    let run = icoo().arg("run").arg(&dir).output().unwrap();

    assert!(run.status.success(), "stderr: {}", stderr(&run));
    assert_eq!(stdout(&run), "hello from Icoo\n");
}

#[test]
fn project_run_uses_pkg_toml_entry_and_calls_main() {
    let dir = temp_dir("project_run");
    fs::create_dir_all(dir.join("app")).unwrap();
    fs::write(
        dir.join("pkg.toml"),
        r#"[package]
name = "custom"
version = "0.1.0"

[run]
entry = "app/start.icoo"
"#,
    )
    .unwrap();
    fs::write(
        dir.join("app").join("start.icoo"),
        r#"
fn main() {
    print("from main")
}
"#,
    )
    .unwrap();

    let run_dir = icoo().arg("run").arg(&dir).output().unwrap();
    assert!(run_dir.status.success(), "stderr: {}", stderr(&run_dir));
    assert_eq!(stdout(&run_dir), "from main\n");

    let run_default = icoo().arg("run").current_dir(&dir).output().unwrap();
    assert!(
        run_default.status.success(),
        "stderr: {}",
        stderr(&run_default)
    );
    assert_eq!(stdout(&run_default), "from main\n");

    let run_legacy = icoo().arg(&dir).output().unwrap();
    assert!(
        run_legacy.status.success(),
        "stderr: {}",
        stderr(&run_legacy)
    );
    assert_eq!(stdout(&run_legacy), "from main\n");
}

#[test]
fn project_run_requires_main_function() {
    let dir = temp_dir("missing_main");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("pkg.toml"),
        r#"[package]
name = "missing-main"
version = "0.1.0"
"#,
    )
    .unwrap();
    fs::write(dir.join("src").join("main.icoo"), "print(\"top\")\n").unwrap();

    let output = icoo().arg("run").arg(&dir).output().unwrap();

    assert!(!output.status.success());
    assert_eq!(stdout(&output), "top\n");
    assert!(stderr(&output).contains("project entry must define fn main()"));
}

#[test]
fn help_and_version_succeed() {
    let help = icoo().arg("--help").output().unwrap();
    assert!(help.status.success(), "stderr: {}", stderr(&help));
    let help_text = stdout(&help);
    assert!(help_text.contains("Usage:"));
    assert!(help_text.contains("icoo init [dir]"));
    assert!(help_text.contains("icoo run [file.icoo|project_dir|pkg.toml]"));
    assert!(help_text.contains("icoo check <file.icoo>"));

    let version = icoo().arg("--version").output().unwrap();
    assert!(version.status.success(), "stderr: {}", stderr(&version));
    assert_eq!(
        stdout(&version),
        format!("icoo {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn invalid_arguments_fail_with_usage() {
    let missing_file = icoo().arg("check").output().unwrap();
    assert!(!missing_file.status.success());
    let missing_file_stderr = stderr(&missing_file);
    assert!(missing_file_stderr.contains("missing <file.icoo>"));
    assert!(missing_file_stderr.contains("usage:"));

    let extra_arg = icoo()
        .arg("run")
        .arg("one.icoo")
        .arg("two.icoo")
        .output()
        .unwrap();
    assert!(!extra_arg.status.success());
    let extra_arg_stderr = stderr(&extra_arg);
    assert!(extra_arg_stderr.contains("unexpected extra argument"));
    assert!(extra_arg_stderr.contains("usage:"));
}
