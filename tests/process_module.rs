use std::cell::RefCell;
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
fn process_exec_runs_shell_command_and_returns_structured_result() {
    let command = if cfg!(windows) {
        "echo icoo-process"
    } else {
        "printf icoo-process"
    };
    let source = format!(
        r#"
import "std.process" as process

let result = process.exec("{}")
print(result.get("success").to_string())
print(result.get("exit_code").to_string())
print(result.get("stdout").contains("icoo-process").to_string())
print(result.get("stderr").to_string())
print(result.get("timed_out").to_string())
"#,
        command
    );

    let output = run(&source).unwrap();

    assert_eq!(output, vec!["true", "0", "true", "", "false"]);
}

#[test]
fn process_exec_reports_nonzero_exit_code() {
    let command = if cfg!(windows) { "exit /B 7" } else { "exit 7" };
    let source = format!(
        r#"
import "std.process" as process

let result = process.exec("{}")
print(result.get("success").to_string())
print(result.get("exit_code").to_string())
"#,
        command
    );

    let output = run(&source).unwrap();

    assert_eq!(output, vec!["false", "7"]);
}

#[test]
fn process_exec_supports_options_map() {
    let command = if cfg!(windows) {
        "echo %ICOO_PROCESS_TEST%"
    } else {
        "printf $ICOO_PROCESS_TEST"
    };
    let source = format!(
        r#"
import "std.process" as process

let result = process.exec("{}", {{
    "env": {{"ICOO_PROCESS_TEST": "from-env"}},
    "max_output_bytes": 4
}})
print(result.get("stdout"))
print(result.get("stdout_truncated").to_string())
"#,
        command
    );

    let output = run(&source).unwrap();

    assert_eq!(output, vec!["from", "true"]);
}
