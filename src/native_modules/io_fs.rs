use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_bytes, expect_string, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::limits::check_bytes_len;
use crate::runtime::value::Value;
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.io.fs",
    kind: "io.fs",
    type_name: "IoFs",
    methods: &[
        NativeMethodSpec {
            name: "exists",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Bool",
        },
        NativeMethodSpec {
            name: "is_file",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Bool",
        },
        NativeMethodSpec {
            name: "is_dir",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Bool",
        },
        NativeMethodSpec {
            name: "read_text",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "String",
        },
        NativeMethodSpec {
            name: "read_bytes",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Bytes",
        },
        NativeMethodSpec {
            name: "write_text",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "write_bytes",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "Bytes"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "append_text",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "append_bytes",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "Bytes"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "list_dir",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Array<String>",
        },
    ],
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
        "exists" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            Ok(Value::Bool(std::path::Path::new(&path).exists()))
        }
        "is_file" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            Ok(Value::Bool(std::path::Path::new(&path).is_file()))
        }
        "is_dir" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            Ok(Value::Bool(std::path::Path::new(&path).is_dir()))
        }
        "read_text" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            std::fs::read_to_string(&path)
                .map(Value::String)
                .map_err(|err| {
                    IcooError::runtime(format!("io.fs.read_text() failed: {}", err), Some(span))
                })
        }
        "read_bytes" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            let bytes = std::fs::read(&path).map_err(|err| {
                IcooError::runtime(format!("io.fs.read_bytes() failed: {}", err), Some(span))
            })?;
            check_bytes_len(bytes.len(), span)?;
            Ok(Value::Bytes(Rc::new(bytes)))
        }
        "write_text" => {
            expect_arity(&args, 2, span)?;
            let path = expect_string(&args[0], span)?;
            let content = expect_string(&args[1], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            std::fs::write(&path, content)
                .map(|_| Value::Nil)
                .map_err(|err| {
                    IcooError::runtime(format!("io.fs.write_text() failed: {}", err), Some(span))
                })
        }
        "write_bytes" => {
            expect_arity(&args, 2, span)?;
            let path = expect_string(&args[0], span)?;
            let content = expect_bytes(&args[1], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            std::fs::write(&path, content.as_slice())
                .map(|_| Value::Nil)
                .map_err(|err| {
                    IcooError::runtime(format!("io.fs.write_bytes() failed: {}", err), Some(span))
                })
        }
        "append_text" => {
            expect_arity(&args, 2, span)?;
            let path = expect_string(&args[0], span)?;
            let content = expect_string(&args[1], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
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
        "append_bytes" => {
            expect_arity(&args, 2, span)?;
            let path = expect_string(&args[0], span)?;
            let content = expect_bytes(&args[1], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .and_then(|mut file| file.write_all(content.as_slice()))
                .map(|_| Value::Nil)
                .map_err(|err| {
                    IcooError::runtime(format!("io.fs.append_bytes() failed: {}", err), Some(span))
                })
        }
        "list_dir" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_list_path(&path, span)?;
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
