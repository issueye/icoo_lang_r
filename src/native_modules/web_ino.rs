use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::IcooResult;
use crate::interpreter::expect_arity;
use crate::lexer::token::Span;
use crate::runtime::value::{Value, WebInoApp};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.web.ino",
    kind: "web.ino",
    type_name: "WebIno",
    methods: &[
        NativeMethodSpec {
            name: "App",
            arity: NativeAritySpec::Exact(0),
            params: &[],
            variadic: None,
            return_type: "WebInoApp",
        },
        NativeMethodSpec {
            name: "create",
            arity: NativeAritySpec::Exact(0),
            params: &[],
            variadic: None,
            return_type: "WebInoApp",
        },
    ],
};

pub(crate) fn call(name: &str, args: Vec<Value>, span: Span) -> Option<IcooResult<Value>> {
    Some(dispatch(name, args, span))
}

fn dispatch(name: &str, args: Vec<Value>, span: Span) -> IcooResult<Value> {
    match name {
        "App" | "create" => {
            expect_arity(&args, 0, span)?;
            Ok(Value::WebInoApp(Rc::new(RefCell::new(WebInoApp {
                routes: HashMap::new(),
            }))))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}
