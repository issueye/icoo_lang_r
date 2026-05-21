use super::NativeModuleSpec;
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_string};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.io.fs",
    kind: "io.fs",
    type_name: "IoFs",
    methods: &[
        "exists",
        "is_file",
        "is_dir",
        "read_text",
        "write_text",
        "append_text",
        "list_dir",
    ],
};

pub(crate) fn call(name: &str, args: Vec<Value>, span: Span) -> Option<IcooResult<Value>> {
    Some(dispatch(name, args, span))
}

fn dispatch(name: &str, args: Vec<Value>, span: Span) -> IcooResult<Value> {
    match name {
        "exists" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            Ok(Value::Bool(std::path::Path::new(&path).exists()))
        }
        "is_file" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            Ok(Value::Bool(std::path::Path::new(&path).is_file()))
        }
        "is_dir" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            Ok(Value::Bool(std::path::Path::new(&path).is_dir()))
        }
        "read_text" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            std::fs::read_to_string(&path)
                .map(Value::String)
                .map_err(|err| {
                    IcooError::runtime(format!("io.fs.read_text() failed: {}", err), Some(span))
                })
        }
        "write_text" => {
            expect_arity(&args, 2, span)?;
            let path = expect_string(&args[0], span)?;
            let content = expect_string(&args[1], span)?;
            std::fs::write(&path, content)
                .map(|_| Value::Nil)
                .map_err(|err| {
                    IcooError::runtime(format!("io.fs.write_text() failed: {}", err), Some(span))
                })
        }
        "append_text" => {
            expect_arity(&args, 2, span)?;
            let path = expect_string(&args[0], span)?;
            let content = expect_string(&args[1], span)?;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .and_then(|mut file| file.write_all(content.as_bytes()))
                .map(|_| Value::Nil)
                .map_err(|err| {
                    IcooError::runtime(format!("io.fs.append_text() failed: {}", err), Some(span))
                })
        }
        "list_dir" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(&path).map_err(|err| {
                IcooError::runtime(format!("io.fs.list_dir() failed: {}", err), Some(span))
            })? {
                let entry = entry.map_err(|err| {
                    IcooError::runtime(format!("io.fs.list_dir() failed: {}", err), Some(span))
                })?;
                entries.push(Value::String(
                    entry.file_name().to_string_lossy().into_owned(),
                ));
            }
            entries.sort_by_key(Value::display);
            Ok(Value::Array(Rc::new(RefCell::new(entries))))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
