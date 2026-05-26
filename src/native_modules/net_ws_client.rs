use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_bytes, expect_string, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::value::{bytes_to_base64, Value};
use std::io::{Read, Write};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const WS_MAX_PAYLOAD_LEN: usize = 65_535;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.net.ws.client",
    kind: "net.ws.client",
    type_name: "NetWsClient",
    methods: &[
        NativeMethodSpec {
            name: "send_text",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "String"],
            variadic: None,
            return_type: "String",
        },
        NativeMethodSpec {
            name: "send_bytes",
            arity: NativeAritySpec::Exact(2),
            params: &["String", "Bytes"],
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
        "send_text" => {
            expect_arity(&args, 2, span)?;
            let url = expect_string(&args[0], span)?;
            let message = expect_string(&args[1], span)?;
            let reply = send_once(runtime, &url, 0x1, message.as_bytes(), span)?;
            String::from_utf8(reply).map(Value::String).map_err(|_| {
                IcooError::runtime("websocket text reply is not valid UTF-8", Some(span))
            })
        }
        "send_bytes" => {
            expect_arity(&args, 2, span)?;
            let url = expect_string(&args[0], span)?;
            let message = expect_bytes(&args[1], span)?;
            let reply = send_once(runtime, &url, 0x2, &message, span)?;
            Ok(Value::Bytes(Rc::new(reply)))
        }
        _ => unreachable!("native module method should be registered before dispatch"),
    }
}

fn send_once(
    runtime: &Interpreter,
    url: &str,
    opcode: u8,
    payload: &[u8],
    span: Span,
) -> IcooResult<Vec<u8>> {
    ensure_payload_len(payload.len(), span)?;
    let parsed = parse_ws_url(url, span)?;
    runtime
        .permissions()
        .check_net_connect_target(&parsed.host, parsed.port, span)?;

    let mut stream =
        std::net::TcpStream::connect((parsed.host.as_str(), parsed.port)).map_err(|err| {
            IcooError::runtime(
                format!("websocket client connection failed: {}", err),
                Some(span),
            )
        })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| {
            IcooError::runtime(format!("websocket client failed: {}", err), Some(span))
        })?;

    let key = websocket_key();
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {}\r\n\r\n",
        parsed.path, parsed.host, key
    );
    stream.write_all(request.as_bytes()).map_err(|err| {
        IcooError::runtime(
            format!("websocket client write failed: {}", err),
            Some(span),
        )
    })?;
    read_upgrade_response(&mut stream, span)?;

    write_frame(&mut stream, opcode, payload, true, span)?;
    let frame = read_frame(&mut stream, false, span)?;
    let _ = write_frame(&mut stream, 0x8, &[], true, span);
    if frame.opcode == 0x8 {
        return Err(IcooError::runtime(
            "websocket peer closed without a reply frame",
            Some(span),
        ));
    }
    if !matches!(frame.opcode, 0x1 | 0x2) {
        return Err(IcooError::runtime(
            format!("unsupported websocket reply opcode: {}", frame.opcode),
            Some(span),
        ));
    }
    Ok(frame.payload)
}

struct ParsedWsUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_ws_url(url: &str, span: Span) -> IcooResult<ParsedWsUrl> {
    let Some(rest) = url.strip_prefix("ws://") else {
        return Err(IcooError::runtime(
            "only ws:// URLs are supported",
            Some(span),
        ));
    };
    let (host_port, path) = rest
        .split_once('/')
        .map(|(host, path)| (host, format!("/{}", path)))
        .unwrap_or((rest, "/".to_string()));
    if host_port.is_empty() {
        return Err(IcooError::runtime("URL host is required", Some(span)));
    }
    let (host, port) = if let Some((host, port)) = host_port.rsplit_once(':') {
        if host.is_empty() {
            return Err(IcooError::runtime("URL host is required", Some(span)));
        }
        let port = port
            .parse::<u16>()
            .map_err(|_| IcooError::runtime("URL port must be between 1 and 65535", Some(span)))?;
        if port == 0 {
            return Err(IcooError::runtime(
                "URL port must be between 1 and 65535",
                Some(span),
            ));
        }
        (host.to_string(), port)
    } else {
        (host_port.to_string(), 80)
    };
    Ok(ParsedWsUrl { host, port, path })
}

fn read_upgrade_response(stream: &mut std::net::TcpStream, span: Span) -> IcooResult<()> {
    let head = read_http_head(stream, span)?;
    let mut lines = head.lines();
    let status = lines.next().ok_or_else(|| {
        IcooError::runtime("invalid websocket response: missing status", Some(span))
    })?;
    if !status.contains(" 101 ") {
        return Err(IcooError::runtime(
            format!("websocket upgrade failed: {}", status.trim()),
            Some(span),
        ));
    }
    Ok(())
}

fn read_http_head(stream: &mut std::net::TcpStream, span: Span) -> IcooResult<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let size = stream.read(&mut buffer).map_err(|err| {
            IcooError::runtime(format!("websocket HTTP read failed: {}", err), Some(span))
        })?;
        if size == 0 {
            return Err(IcooError::runtime(
                "invalid websocket HTTP response: missing header terminator",
                Some(span),
            ));
        }
        bytes.extend_from_slice(&buffer[..size]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(String::from_utf8_lossy(&bytes).into_owned());
        }
        if bytes.len() > 16 * 1024 {
            return Err(IcooError::runtime(
                "websocket HTTP headers are too large",
                Some(span),
            ));
        }
    }
}

struct WsFrame {
    opcode: u8,
    payload: Vec<u8>,
}

fn read_frame(
    stream: &mut std::net::TcpStream,
    expect_masked: bool,
    span: Span,
) -> IcooResult<WsFrame> {
    let mut header = [0_u8; 2];
    stream.read_exact(&mut header).map_err(|err| {
        IcooError::runtime(format!("websocket frame read failed: {}", err), Some(span))
    })?;
    if header[0] & 0x80 == 0 {
        return Err(IcooError::runtime(
            "fragmented websocket frames are not supported",
            Some(span),
        ));
    }
    let opcode = header[0] & 0x0f;
    let masked = header[1] & 0x80 != 0;
    if masked != expect_masked {
        let message = if expect_masked {
            "expected a masked websocket frame"
        } else {
            "server websocket replies must not be masked"
        };
        return Err(IcooError::runtime(message, Some(span)));
    }
    let len_code = header[1] & 0x7f;
    let len = match len_code {
        0..=125 => len_code as usize,
        126 => {
            let mut bytes = [0_u8; 2];
            stream.read_exact(&mut bytes).map_err(|err| {
                IcooError::runtime(format!("websocket frame read failed: {}", err), Some(span))
            })?;
            u16::from_be_bytes(bytes) as usize
        }
        _ => {
            return Err(IcooError::runtime(
                "websocket payloads larger than 65535 bytes are not supported",
                Some(span),
            ))
        }
    };
    ensure_payload_len(len, span)?;
    let mut mask = [0_u8; 4];
    if masked {
        stream.read_exact(&mut mask).map_err(|err| {
            IcooError::runtime(format!("websocket frame read failed: {}", err), Some(span))
        })?;
    }
    let mut payload = vec![0_u8; len];
    stream.read_exact(&mut payload).map_err(|err| {
        IcooError::runtime(format!("websocket frame read failed: {}", err), Some(span))
    })?;
    if masked {
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % 4];
        }
    }
    Ok(WsFrame { opcode, payload })
}

fn write_frame(
    stream: &mut std::net::TcpStream,
    opcode: u8,
    payload: &[u8],
    masked: bool,
    span: Span,
) -> IcooResult<()> {
    ensure_payload_len(payload.len(), span)?;
    let mut frame = Vec::with_capacity(payload.len() + 8);
    frame.push(0x80 | opcode);
    let mask_bit = if masked { 0x80 } else { 0 };
    if payload.len() <= 125 {
        frame.push(mask_bit | payload.len() as u8);
    } else {
        frame.push(mask_bit | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    if masked {
        let mask = websocket_mask();
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
    } else {
        frame.extend_from_slice(payload);
    }
    stream.write_all(&frame).map_err(|err| {
        IcooError::runtime(format!("websocket frame write failed: {}", err), Some(span))
    })
}

fn ensure_payload_len(len: usize, span: Span) -> IcooResult<()> {
    if len > WS_MAX_PAYLOAD_LEN {
        Err(IcooError::runtime(
            "websocket payloads larger than 65535 bytes are not supported",
            Some(span),
        ))
    } else {
        Ok(())
    }
}

fn websocket_key() -> String {
    let seed = pseudo_random_u64();
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&seed.to_be_bytes());
    bytes[8..].copy_from_slice(&seed.rotate_left(17).to_be_bytes());
    bytes_to_base64(&bytes)
}

fn websocket_mask() -> [u8; 4] {
    (pseudo_random_u64() as u32).to_be_bytes()
}

fn pseudo_random_u64() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    let stack = (&nanos as *const u64 as usize) as u64;
    nanos ^ stack.rotate_left(13)
}
