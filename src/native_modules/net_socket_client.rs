use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_bytes, expect_int, expect_string, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::rc::Rc;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.net.socket.client",
    kind: "net.socket.client",
    type_name: "NetSocketClient",
    methods: &[
        NativeMethodSpec {
            name: "send",
            arity: NativeAritySpec::Exact(3),
            params: &["String", "Int", "String"],
            variadic: None,
            return_type: "String",
        },
        NativeMethodSpec {
            name: "send_bytes",
            arity: NativeAritySpec::Exact(3),
            params: &["String", "Int", "Bytes"],
            variadic: None,
            return_type: "Bytes",
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
        "send" => {
            expect_arity(&args, 3, span)?;
            let host = expect_string(&args[0], span)?;
            let port = expect_port(&args[1], "client port", span)?;
            let body = expect_string(&args[2], span)?;
            let response = socket_send(runtime, &host, port, body.as_bytes(), span)?;
            Ok(Value::String(
                String::from_utf8_lossy(&response).into_owned(),
            ))
        }
        "send_bytes" => {
            expect_arity(&args, 3, span)?;
            let host = expect_string(&args[0], span)?;
            let port = expect_port(&args[1], "client port", span)?;
            let body = expect_bytes(&args[2], span)?;
            let response = socket_send(runtime, &host, port, body.as_slice(), span)?;
            Ok(Value::Bytes(Rc::new(response)))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}

fn socket_send(
    runtime: &Interpreter,
    host: &str,
    port: u16,
    body: &[u8],
    span: Span,
) -> IcooResult<Vec<u8>> {
    runtime
        .permissions()
        .check_net_connect_target(host, port, span)?;
    let mut stream = TcpStream::connect((host, port)).map_err(|err| {
        IcooError::runtime(
            format!("socket client connection failed: {}", err),
            Some(span),
        )
    })?;
    stream.write_all(body).map_err(|err| {
        IcooError::runtime(format!("socket client write failed: {}", err), Some(span))
    })?;
    stream.shutdown(Shutdown::Write).map_err(|err| {
        IcooError::runtime(
            format!("socket client shutdown failed: {}", err),
            Some(span),
        )
    })?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).map_err(|err| {
        IcooError::runtime(format!("socket client read failed: {}", err), Some(span))
    })?;
    Ok(response)
}

fn expect_port(value: &Value, label: &str, span: Span) -> IcooResult<u16> {
    let port = expect_int(value, span)?;
    if !(1..=65535).contains(&port) {
        return Err(IcooError::runtime(
            format!("{} must be between 1 and 65535", label),
            Some(span),
        ));
    }
    Ok(port as u16)
}
