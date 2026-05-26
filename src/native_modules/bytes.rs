use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_string};
use crate::lexer::token::Span;
use crate::runtime::limits::check_bytes_len;
use crate::runtime::value::{bytes_from_base64, bytes_from_hex, Value};
use std::rc::Rc;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "Bytes",
    kind: "Bytes",
    type_name: "BytesFactory",
    methods: &[
        NativeMethodSpec {
            name: "empty",
            arity: NativeAritySpec::Exact(0),
            params: &[],
            variadic: None,
            return_type: "Bytes",
        },
        NativeMethodSpec {
            name: "from_hex",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Bytes",
        },
        NativeMethodSpec {
            name: "from_base64",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Bytes",
        },
        NativeMethodSpec {
            name: "from_string",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Bytes",
        },
    ],
};

pub(crate) fn call(name: &str, args: Vec<Value>, span: Span) -> Option<IcooResult<Value>> {
    Some(dispatch(name, args, span))
}

fn dispatch(name: &str, args: Vec<Value>, span: Span) -> IcooResult<Value> {
    match name {
        "empty" => {
            expect_arity(&args, 0, span)?;
            Ok(Value::Bytes(Rc::new(Vec::new())))
        }
        "from_hex" => {
            expect_arity(&args, 1, span)?;
            let text = expect_string(&args[0], span)?;
            let bytes = bytes_from_hex(&text).map_err(|err| {
                IcooError::runtime(format!("Bytes.from_hex() failed: {}", err), Some(span))
            })?;
            check_bytes_len(bytes.len(), span)?;
            Ok(Value::Bytes(Rc::new(bytes)))
        }
        "from_base64" => {
            expect_arity(&args, 1, span)?;
            let text = expect_string(&args[0], span)?;
            let bytes = bytes_from_base64(&text).map_err(|err| {
                IcooError::runtime(format!("Bytes.from_base64() failed: {}", err), Some(span))
            })?;
            check_bytes_len(bytes.len(), span)?;
            Ok(Value::Bytes(Rc::new(bytes)))
        }
        "from_string" => {
            expect_arity(&args, 1, span)?;
            let text = expect_string(&args[0], span)?;
            check_bytes_len(text.len(), span)?;
            Ok(Value::Bytes(Rc::new(text.into_bytes())))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
