use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_bytes, expect_int, expect_string, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::limits::check_bytes_len;
use crate::runtime::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

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
            name: "read_bytes_range",
            arity: NativeAritySpec::Exact(3),
            params: &["String", "Int", "Int"],
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
            name: "write_text_atomic",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "write_bytes_atomic",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "Bytes"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "write_bytes_at",
            arity: NativeAritySpec::Exact(3),
            params: &["String", "Int", "Bytes"],
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
        NativeMethodSpec {
            name: "mkdir",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "mkdir_all",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "remove_file",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "remove_dir",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "remove_dir_all",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "rename",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "copy",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "String"],
            variadic: None,
            return_type: "Int",
        },
        NativeMethodSpec {
            name: "metadata",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "symlink_metadata",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Map<String, Any>",
        },
        NativeMethodSpec {
            name: "canonicalize",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "String",
        },
        NativeMethodSpec {
            name: "read_link",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "String",
        },
        NativeMethodSpec {
            name: "create_symlink_file",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "create_symlink_dir",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "String"],
            variadic: None,
            return_type: "Nil",
        },
        NativeMethodSpec {
            name: "create_temp_file",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "String"],
            variadic: None,
            return_type: "String",
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
            Ok(Value::Bool(Path::new(&path).exists()))
        }
        "is_file" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            Ok(Value::Bool(Path::new(&path).is_file()))
        }
        "is_dir" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            Ok(Value::Bool(Path::new(&path).is_dir()))
        }
        "read_text" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            fs::read_to_string(&path).map(Value::String).map_err(|err| {
                IcooError::runtime(format!("io.fs.read_text() failed: {}", err), Some(span))
            })
        }
        "read_bytes" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            let bytes = fs::read(&path).map_err(|err| {
                IcooError::runtime(format!("io.fs.read_bytes() failed: {}", err), Some(span))
            })?;
            check_bytes_len(bytes.len(), span)?;
            Ok(Value::Bytes(Rc::new(bytes)))
        }
        "read_bytes_range" => {
            expect_arity(&args, 3, span)?;
            let path = expect_string(&args[0], span)?;
            let offset = expect_non_negative_usize(&args[1], "offset", span)?;
            let length = expect_non_negative_usize(&args[2], "length", span)?;
            check_bytes_len(length, span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            let mut file = File::open(&path).map_err(|err| {
                IcooError::runtime(
                    format!("io.fs.read_bytes_range() failed: {}", err),
                    Some(span),
                )
            })?;
            file.seek(SeekFrom::Start(offset as u64)).map_err(|err| {
                IcooError::runtime(
                    format!("io.fs.read_bytes_range() failed: {}", err),
                    Some(span),
                )
            })?;
            let mut bytes = vec![0; length];
            let size = file.read(&mut bytes).map_err(|err| {
                IcooError::runtime(
                    format!("io.fs.read_bytes_range() failed: {}", err),
                    Some(span),
                )
            })?;
            bytes.truncate(size);
            Ok(Value::Bytes(Rc::new(bytes)))
        }
        "write_text" => {
            expect_arity(&args, 2, span)?;
            let path = expect_string(&args[0], span)?;
            let content = expect_string(&args[1], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            fs::write(&path, content)
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
            fs::write(&path, content.as_slice())
                .map(|_| Value::Nil)
                .map_err(|err| {
                    IcooError::runtime(format!("io.fs.write_bytes() failed: {}", err), Some(span))
                })
        }
        "write_text_atomic" => {
            expect_arity(&args, 2, span)?;
            let path = expect_string(&args[0], span)?;
            let content = expect_string(&args[1], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            write_atomic(
                runtime,
                &path,
                content.as_bytes(),
                span,
                "write_text_atomic",
            )?;
            Ok(Value::Nil)
        }
        "write_bytes_atomic" => {
            expect_arity(&args, 2, span)?;
            let path = expect_string(&args[0], span)?;
            let content = expect_bytes(&args[1], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            write_atomic(
                runtime,
                &path,
                content.as_slice(),
                span,
                "write_bytes_atomic",
            )?;
            Ok(Value::Nil)
        }
        "write_bytes_at" => {
            expect_arity(&args, 3, span)?;
            let path = expect_string(&args[0], span)?;
            let offset = expect_non_negative_usize(&args[1], "offset", span)?;
            let content = expect_bytes(&args[2], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            let mut file = OpenOptions::new().write(true).open(&path).map_err(|err| {
                IcooError::runtime(
                    format!("io.fs.write_bytes_at() failed: {}", err),
                    Some(span),
                )
            })?;
            file.seek(SeekFrom::Start(offset as u64)).map_err(|err| {
                IcooError::runtime(
                    format!("io.fs.write_bytes_at() failed: {}", err),
                    Some(span),
                )
            })?;
            file.write_all(content.as_slice()).map_err(|err| {
                IcooError::runtime(
                    format!("io.fs.write_bytes_at() failed: {}", err),
                    Some(span),
                )
            })?;
            Ok(Value::Nil)
        }
        "append_text" => {
            expect_arity(&args, 2, span)?;
            let path = expect_string(&args[0], span)?;
            let content = expect_string(&args[1], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            OpenOptions::new()
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
            OpenOptions::new()
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
            for entry in fs::read_dir(&path).map_err(|err| {
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
        "mkdir" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            fs::create_dir(&path).map(|_| Value::Nil).map_err(|err| {
                IcooError::runtime(format!("io.fs.mkdir() failed: {}", err), Some(span))
            })
        }
        "mkdir_all" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            fs::create_dir_all(&path)
                .map(|_| Value::Nil)
                .map_err(|err| {
                    IcooError::runtime(format!("io.fs.mkdir_all() failed: {}", err), Some(span))
                })
        }
        "remove_file" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            fs::remove_file(&path).map(|_| Value::Nil).map_err(|err| {
                IcooError::runtime(format!("io.fs.remove_file() failed: {}", err), Some(span))
            })
        }
        "remove_dir" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            fs::remove_dir(&path).map(|_| Value::Nil).map_err(|err| {
                IcooError::runtime(format!("io.fs.remove_dir() failed: {}", err), Some(span))
            })
        }
        "remove_dir_all" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_write_path(&path, span)?;
            fs::remove_dir_all(&path)
                .map(|_| Value::Nil)
                .map_err(|err| {
                    IcooError::runtime(
                        format!("io.fs.remove_dir_all() failed: {}", err),
                        Some(span),
                    )
                })
        }
        "rename" => {
            expect_arity(&args, 2, span)?;
            let from = expect_string(&args[0], span)?;
            let to = expect_string(&args[1], span)?;
            runtime.permissions().check_fs_write_path(&from, span)?;
            runtime.permissions().check_fs_write_path(&to, span)?;
            fs::rename(&from, &to).map(|_| Value::Nil).map_err(|err| {
                IcooError::runtime(format!("io.fs.rename() failed: {}", err), Some(span))
            })
        }
        "copy" => {
            expect_arity(&args, 2, span)?;
            let from = expect_string(&args[0], span)?;
            let to = expect_string(&args[1], span)?;
            runtime.permissions().check_fs_read_path(&from, span)?;
            runtime.permissions().check_fs_write_path(&to, span)?;
            let copied = fs::copy(&from, &to).map_err(|err| {
                IcooError::runtime(format!("io.fs.copy() failed: {}", err), Some(span))
            })?;
            Ok(Value::Int(u64_to_i64_saturating(copied)))
        }
        "metadata" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            let metadata = fs::metadata(&path).map_err(|err| {
                IcooError::runtime(format!("io.fs.metadata() failed: {}", err), Some(span))
            })?;
            Ok(metadata_value(metadata))
        }
        "symlink_metadata" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            let metadata = fs::symlink_metadata(&path).map_err(|err| {
                IcooError::runtime(
                    format!("io.fs.symlink_metadata() failed: {}", err),
                    Some(span),
                )
            })?;
            Ok(metadata_value(metadata))
        }
        "canonicalize" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            fs::canonicalize(&path)
                .map(|path| Value::String(path.to_string_lossy().into_owned()))
                .map_err(|err| {
                    IcooError::runtime(format!("io.fs.canonicalize() failed: {}", err), Some(span))
                })
        }
        "read_link" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            runtime.permissions().check_fs_read_path(&path, span)?;
            fs::read_link(&path)
                .map(|path| Value::String(path.to_string_lossy().into_owned()))
                .map_err(|err| {
                    IcooError::runtime(format!("io.fs.read_link() failed: {}", err), Some(span))
                })
        }
        "create_symlink_file" => {
            expect_arity(&args, 2, span)?;
            let target = expect_string(&args[0], span)?;
            let link = expect_string(&args[1], span)?;
            runtime.permissions().check_fs_read_path(&target, span)?;
            runtime.permissions().check_fs_write_path(&link, span)?;
            create_symlink_file(&target, &link, span)?;
            Ok(Value::Nil)
        }
        "create_symlink_dir" => {
            expect_arity(&args, 2, span)?;
            let target = expect_string(&args[0], span)?;
            let link = expect_string(&args[1], span)?;
            runtime.permissions().check_fs_read_path(&target, span)?;
            runtime.permissions().check_fs_write_path(&link, span)?;
            create_symlink_dir(&target, &link, span)?;
            Ok(Value::Nil)
        }
        "create_temp_file" => {
            expect_arity(&args, 2, span)?;
            let dir = expect_string(&args[0], span)?;
            let prefix = expect_string(&args[1], span)?;
            runtime.permissions().check_fs_write_path(&dir, span)?;
            let path = create_temp_file(runtime, &dir, &prefix, span)?;
            Ok(Value::String(path.to_string_lossy().into_owned()))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}

fn expect_non_negative_usize(value: &Value, name: &str, span: Span) -> IcooResult<usize> {
    let value = expect_int(value, span)?;
    if value < 0 {
        return Err(IcooError::runtime(
            format!("{} must be non-negative", name),
            Some(span),
        ));
    }
    usize::try_from(value).map_err(|_| {
        IcooError::runtime(
            format!("{} is too large for this platform", name),
            Some(span),
        )
    })
}

fn metadata_value(metadata: fs::Metadata) -> Value {
    let file_type = metadata.file_type();
    let mut map = HashMap::new();
    map.insert("is_file".to_string(), Value::Bool(file_type.is_file()));
    map.insert("is_dir".to_string(), Value::Bool(file_type.is_dir()));
    map.insert(
        "is_symlink".to_string(),
        Value::Bool(file_type.is_symlink()),
    );
    map.insert(
        "len".to_string(),
        Value::Int(u64_to_i64_saturating(metadata.len())),
    );
    map.insert(
        "readonly".to_string(),
        Value::Bool(metadata.permissions().readonly()),
    );
    map.insert("type".to_string(), Value::String(file_type_name(file_type)));
    map.insert(
        "modified_ms".to_string(),
        system_time_value(metadata.modified()),
    );
    map.insert(
        "accessed_ms".to_string(),
        system_time_value(metadata.accessed()),
    );
    map.insert(
        "created_ms".to_string(),
        system_time_value(metadata.created()),
    );
    Value::Map(Rc::new(RefCell::new(map)))
}

fn file_type_name(file_type: fs::FileType) -> String {
    if file_type.is_file() {
        "file".to_string()
    } else if file_type.is_dir() {
        "dir".to_string()
    } else if file_type.is_symlink() {
        "symlink".to_string()
    } else {
        "other".to_string()
    }
}

fn system_time_value(time: std::io::Result<SystemTime>) -> Value {
    match time
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
    {
        Some(duration) => Value::Int(u128_to_i64_saturating(duration.as_millis())),
        None => Value::Nil,
    }
}

fn u64_to_i64_saturating(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn u128_to_i64_saturating(value: u128) -> i64 {
    value.min(i64::MAX as u128) as i64
}

fn write_atomic(
    runtime: &Interpreter,
    path: &str,
    bytes: &[u8],
    span: Span,
    operation: &str,
) -> IcooResult<()> {
    let target = Path::new(path);
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let temp_path = unique_temp_path(dir, ".icoo-atomic", span)?;
    runtime
        .permissions()
        .check_fs_write_path(temp_path.to_string_lossy().as_ref(), span)?;
    let write_result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|_| replace_with_temp(&temp_path, target));
    if let Err(err) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(IcooError::runtime(
            format!("io.fs.{}() failed: {}", operation, err),
            Some(span),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_with_temp(temp_path: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(temp_path, target)
}

#[cfg(windows)]
fn replace_with_temp(temp_path: &Path, target: &Path) -> std::io::Result<()> {
    if !target.exists() {
        return fs::rename(temp_path, target);
    }

    let backup_path = unique_backup_path(target);
    fs::rename(target, &backup_path)?;
    match fs::rename(temp_path, target) {
        Ok(()) => {
            let _ = fs::remove_file(&backup_path);
            Ok(())
        }
        Err(err) => {
            let _ = fs::rename(&backup_path, target);
            Err(err)
        }
    }
}

#[cfg(windows)]
fn unique_backup_path(target: &Path) -> PathBuf {
    let mut path = target.to_path_buf();
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let file_name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "target".to_string());
    path.set_file_name(format!(".{}.{}.replace-backup", file_name, suffix));
    path
}

fn create_temp_file(
    runtime: &Interpreter,
    dir: &str,
    prefix: &str,
    span: Span,
) -> IcooResult<PathBuf> {
    if prefix.is_empty()
        || prefix.contains('/')
        || prefix.contains('\\')
        || prefix == "."
        || prefix == ".."
    {
        return Err(IcooError::runtime(
            "temporary file prefix must be a non-empty file name prefix",
            Some(span),
        ));
    }
    let dir = Path::new(dir);
    for _ in 0..128 {
        let path = unique_temp_path(dir, prefix, span)?;
        runtime
            .permissions()
            .check_fs_write_path(path.to_string_lossy().as_ref(), span)?;
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(IcooError::runtime(
                    format!("io.fs.create_temp_file() failed: {}", err),
                    Some(span),
                ))
            }
        }
    }
    Err(IcooError::runtime(
        "io.fs.create_temp_file() failed: could not allocate a unique path",
        Some(span),
    ))
}

fn unique_temp_path(dir: &Path, prefix: &str, span: Span) -> IcooResult<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| IcooError::runtime(format!("system time error: {}", err), Some(span)))?
        .as_nanos();
    let thread_id = format!("{:?}", std::thread::current().id())
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    Ok(dir.join(format!(
        "{}-{}-{}-{}",
        prefix,
        std::process::id(),
        thread_id,
        timestamp
    )))
}

#[cfg(unix)]
fn create_symlink_file(target: &str, link: &str, span: Span) -> IcooResult<()> {
    std::os::unix::fs::symlink(target, link).map_err(|err| {
        IcooError::runtime(
            format!("io.fs.create_symlink_file() failed: {}", err),
            Some(span),
        )
    })
}

#[cfg(unix)]
fn create_symlink_dir(target: &str, link: &str, span: Span) -> IcooResult<()> {
    std::os::unix::fs::symlink(target, link).map_err(|err| {
        IcooError::runtime(
            format!("io.fs.create_symlink_dir() failed: {}", err),
            Some(span),
        )
    })
}

#[cfg(windows)]
fn create_symlink_file(target: &str, link: &str, span: Span) -> IcooResult<()> {
    std::os::windows::fs::symlink_file(target, link).map_err(|err| {
        IcooError::runtime(
            format!("io.fs.create_symlink_file() failed: {}", err),
            Some(span),
        )
    })
}

#[cfg(windows)]
fn create_symlink_dir(target: &str, link: &str, span: Span) -> IcooResult<()> {
    std::os::windows::fs::symlink_dir(target, link).map_err(|err| {
        IcooError::runtime(
            format!("io.fs.create_symlink_dir() failed: {}", err),
            Some(span),
        )
    })
}
