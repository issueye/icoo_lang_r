use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::IcooResult;
use crate::interpreter::{expect_arity, expect_bytes, expect_string};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "Buffer",
    kind: "Buffer",
    type_name: "BufferFactory",
    methods: &[
        NativeMethodSpec {
            name: "new",
            arity: NativeAritySpec::Exact(0),
            params: &[],
            variadic: None,
            return_type: "Buffer",
        },
        NativeMethodSpec {
            name: "from_bytes",
            arity: NativeAritySpec::Exact(1),
            params: &["Bytes"],
            variadic: None,
            return_type: "Buffer",
        },
        NativeMethodSpec {
            name: "from_string",
            arity: NativeAritySpec::Exact(1),
            params: &["String"],
            variadic: None,
            return_type: "Buffer",
        },
    ],
};

pub(crate) fn call(name: &str, args: Vec<Value>, span: Span) -> Option<IcooResult<Value>> {
    Some(dispatch(name, args, span))
}

fn dispatch(name: &str, args: Vec<Value>, span: Span) -> IcooResult<Value> {
    match name {
        "new" => {
            expect_arity(&args, 0, span)?;
            Ok(Value::Buffer(Rc::new(RefCell::new(Vec::new()))))
        }
        "from_bytes" => {
            expect_arity(&args, 1, span)?;
            let bytes = expect_bytes(&args[0], span)?;
            Ok(Value::Buffer(Rc::new(RefCell::new(bytes.as_ref().clone()))))
        }
        "from_string" => {
            expect_arity(&args, 1, span)?;
            let text = expect_string(&args[0], span)?;
            Ok(Value::Buffer(Rc::new(RefCell::new(text.into_bytes()))))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
