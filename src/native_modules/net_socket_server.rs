use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_int, expect_string, is_callable, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::time::Duration;

const SOCKET_SERVER_READ_TIMEOUT: Duration = Duration::from_millis(200);

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.net.socket.server",
    kind: "net.socket.server",
    type_name: "NetSocketServer",
    methods: &[NativeMethodSpec {
        name: "serve_once",
        arity: NativeAritySpec::Exact(3),
        params: &["String", "Int", "Function"],
        variadic: None,
        return_type: "Nil",
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
        "serve_once" => {
            expect_arity(&args, 3, span)?;
            let host = expect_string(&args[0], span)?;
            let port = expect_port(&args[1], "server port", span)?;
            let handler = args[2].clone();
            if !is_callable(&handler) {
                return Err(IcooError::runtime(
                    "socket server handler must be callable",
                    Some(span),
                ));
            }
            serve_once(runtime, &host, port, handler, span)?;
            Ok(Value::Nil)
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}

fn serve_once(
    runtime: &mut Interpreter,
    host: &str,
    port: u16,
    handler: Value,
    span: Span,
) -> IcooResult<()> {
    runtime
        .permissions()
        .check_net_listen_target(host, port, span)?;
    let listener = TcpListener::bind((host, port)).map_err(|err| {
        IcooError::runtime(format!("socket server bind failed: {}", err), Some(span))
    })?;
    let (mut stream, _) = listener.accept().map_err(|err| {
        IcooError::runtime(format!("socket server accept failed: {}", err), Some(span))
    })?;
    stream
        .set_read_timeout(Some(SOCKET_SERVER_READ_TIMEOUT))
        .map_err(|err| IcooError::runtime(format!("socket server failed: {}", err), Some(span)))?;

    let request = read_available_bytes(&mut stream, span)?;
    let response = runtime.call_value(handler, vec![Value::Bytes(Rc::new(request))], span)?;
    let response_bytes = match response {
        Value::Bytes(bytes) => bytes.as_slice().to_vec(),
        value => value.display().into_bytes(),
    };
    stream.write_all(&response_bytes).map_err(|err| {
        IcooError::runtime(format!("socket server write failed: {}", err), Some(span))
    })
}

fn read_available_bytes(stream: &mut std::net::TcpStream, span: Span) -> IcooResult<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => bytes.extend_from_slice(&buffer[..size]),
            Err(err)
                if err.kind() == ErrorKind::WouldBlock || err.kind() == ErrorKind::TimedOut =>
            {
                break;
            }
            Err(err) => {
                return Err(IcooError::runtime(
                    format!("socket server read failed: {}", err),
                    Some(span),
                ));
            }
        }
    }
    Ok(bytes)
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
