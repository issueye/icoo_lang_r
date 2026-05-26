use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::IcooResult;
use crate::interpreter::{expect_arity, expect_string};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::path::{Component, Path, PathBuf};

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.path",
    kind: "path",
    type_name: "Path",
    methods: &[
        NativeMethodSpec {
            name: "join",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "String"],
            variadic: None,
            return_type: "String",
        },
        NativeMethodSpec {
            name: "normalize",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "String",
        },
        NativeMethodSpec {
            name: "dirname",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "String",
        },
        NativeMethodSpec {
            name: "basename",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "String",
        },
        NativeMethodSpec {
            name: "extension",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "String",
        },
        NativeMethodSpec {
            name: "is_absolute",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Bool",
        },
    ],
};

pub(crate) fn call(name: &str, args: Vec<Value>, span: Span) -> Option<IcooResult<Value>> {
    Some(dispatch(name, args, span))
}

fn dispatch(name: &str, args: Vec<Value>, span: Span) -> IcooResult<Value> {
    match name {
        "join" => {
            expect_arity(&args, 2, span)?;
            let base = expect_string(&args[0], span)?;
            let child = expect_string(&args[1], span)?;
            Ok(Value::String(
                Path::new(&base).join(child).to_string_lossy().into_owned(),
            ))
        }
        "normalize" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            Ok(Value::String(normalize_path(&path)))
        }
        "dirname" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            Ok(Value::String(
                Path::new(&path)
                    .parent()
                    .map(|parent| parent.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        "basename" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            Ok(Value::String(
                Path::new(&path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        "extension" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            Ok(Value::String(
                Path::new(&path)
                    .extension()
                    .map(|extension| extension.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        "is_absolute" => {
            expect_arity(&args, 1, span)?;
            let path = expect_string(&args[0], span)?;
            Ok(Value::Bool(Path::new(&path).is_absolute()))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}

fn normalize_path(path: &str) -> String {
    let path = Path::new(path);
    let anchored = path.is_absolute();
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if can_pop_normal_component(&normalized) {
                    normalized.pop();
                } else if !anchored {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(_) | Component::Prefix(_) | Component::RootDir => {
                normalized.push(component.as_os_str());
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        ".".to_string()
    } else {
        normalized.to_string_lossy().into_owned()
    }
}

fn can_pop_normal_component(path: &Path) -> bool {
    matches!(path.components().next_back(), Some(Component::Normal(_)))
}
