use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_int, expect_string, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

const DEFAULT_MAX_OUTPUT_BYTES: usize = 1024 * 1024;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.process",
    kind: "process",
    type_name: "Process",
    methods: &[NativeMethodSpec {
        name: "exec",
        arity: NativeAritySpec::Range { min: 1, max: 2 },
        params: &["String", "Map"],
        variadic: None,
        return_type: "Map<String, Any>",
    }],
};

pub(crate) fn call(
    runtime: &mut Interpreter,
    name: &str,
    args: Vec<Value>,
    span: Span,
) -> Option<IcooResult<Value>> {
    Some(dispatch(runtime, name, args, span))
}

fn dispatch(
    runtime: &mut Interpreter,
    name: &str,
    args: Vec<Value>,
    span: Span,
) -> IcooResult<Value> {
    match name {
        "exec" => {
            expect_arity_range(&args, 1, 2, span)?;
            runtime.permissions().check_process_exec(span)?;

            let command = expect_string(&args[0], span)?;
            let options = ProcessExecOptions::from_value(args.get(1), span)?;
            exec_shell(command, options, span)
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}

#[derive(Debug)]
struct ProcessExecOptions {
    cwd: Option<String>,
    timeout: Option<Duration>,
    env: Vec<(String, String)>,
    max_output_bytes: usize,
}

impl ProcessExecOptions {
    fn from_value(value: Option<&Value>, span: Span) -> IcooResult<Self> {
        let mut options = Self {
            cwd: None,
            timeout: None,
            env: Vec::new(),
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
        };
        let Some(value) = value else {
            return Ok(options);
        };
        let Value::Map(map) = value else {
            return Err(IcooError::runtime(
                "process.exec() options must be a Map",
                Some(span),
            ));
        };
        let map = map.borrow();

        if let Some(value) = map.get("cwd") {
            options.cwd = Some(expect_string(value, span)?);
        }
        if let Some(value) = map.get("timeout_ms") {
            let timeout_ms = expect_non_negative_i64(value, "timeout_ms", span)?;
            options.timeout = Some(Duration::from_millis(timeout_ms as u64));
        }
        if let Some(value) = map.get("max_output_bytes") {
            options.max_output_bytes =
                expect_non_negative_i64(value, "max_output_bytes", span)? as usize;
        }
        if let Some(value) = map.get("env") {
            let Value::Map(env_map) = value else {
                return Err(IcooError::runtime(
                    "process.exec() env option must be a Map",
                    Some(span),
                ));
            };
            for (key, value) in env_map.borrow().iter() {
                options.env.push((key.clone(), expect_string(value, span)?));
            }
        }

        Ok(options)
    }
}

fn exec_shell(command: String, options: ProcessExecOptions, span: Span) -> IcooResult<Value> {
    let mut shell = shell_command(&command);
    shell.stdout(Stdio::piped()).stderr(Stdio::piped());

    if let Some(cwd) = options.cwd {
        shell.current_dir(cwd);
    }
    for (key, value) in options.env {
        shell.env(key, value);
    }

    let mut child = shell
        .spawn()
        .map_err(|err| IcooError::runtime(format!("process.exec() failed: {}", err), Some(span)))?;

    let mut timed_out = false;
    if let Some(timeout) = options.timeout {
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() >= timeout => {
                    timed_out = true;
                    let _ = child.kill();
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(err) => {
                    return Err(IcooError::runtime(
                        format!("process.exec() failed: {}", err),
                        Some(span),
                    ))
                }
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|err| IcooError::runtime(format!("process.exec() failed: {}", err), Some(span)))?;

    Ok(process_result_map(
        output.status.code(),
        output.status.success() && !timed_out,
        timed_out,
        output.stdout,
        output.stderr,
        options.max_output_bytes,
    ))
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut shell =
            Command::new(std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string()));
        shell.arg("/C").arg(command);
        shell
    }
    #[cfg(not(windows))]
    {
        let mut shell = Command::new(std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string()));
        shell.arg("-c").arg(command);
        shell
    }
}

fn process_result_map(
    exit_code: Option<i32>,
    success: bool,
    timed_out: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    max_output_bytes: usize,
) -> Value {
    let (stdout, stdout_truncated) = truncate_output(stdout, max_output_bytes);
    let (stderr, stderr_truncated) = truncate_output(stderr, max_output_bytes);
    let mut result = HashMap::new();
    result.insert(
        "exit_code".to_string(),
        exit_code
            .map(|code| Value::Int(code as i64))
            .unwrap_or(Value::Nil),
    );
    result.insert("success".to_string(), Value::Bool(success));
    result.insert("timed_out".to_string(), Value::Bool(timed_out));
    result.insert("stdout".to_string(), Value::String(stdout));
    result.insert("stderr".to_string(), Value::String(stderr));
    result.insert(
        "stdout_truncated".to_string(),
        Value::Bool(stdout_truncated),
    );
    result.insert(
        "stderr_truncated".to_string(),
        Value::Bool(stderr_truncated),
    );
    Value::Map(Rc::new(RefCell::new(result)))
}

fn truncate_output(mut output: Vec<u8>, max_output_bytes: usize) -> (String, bool) {
    let truncated = output.len() > max_output_bytes;
    if truncated {
        output.truncate(max_output_bytes);
    }
    (String::from_utf8_lossy(&output).into_owned(), truncated)
}

fn expect_arity_range(args: &[Value], min: usize, max: usize, span: Span) -> IcooResult<()> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else {
        Err(IcooError::runtime(
            format!(
                "expected {} to {} arguments but got {}",
                min,
                max,
                args.len()
            ),
            Some(span),
        ))
    }
}

fn expect_non_negative_i64(value: &Value, name: &str, span: Span) -> IcooResult<i64> {
    let value = expect_int(value, span)?;
    if value < 0 {
        Err(IcooError::runtime(
            format!("process.exec() option '{}' must be non-negative", name),
            Some(span),
        ))
    } else {
        Ok(value)
    }
}
