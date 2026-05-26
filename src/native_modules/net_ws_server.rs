use super::{NativeAritySpec, NativeMethodSpec, NativeModuleSpec};
use crate::error::{IcooError, IcooResult};
use crate::interpreter::{expect_arity, expect_int, expect_string, is_callable, Interpreter};
use crate::lexer::token::Span;
use crate::runtime::value::{bytes_to_base64, Value};
use std::collections::HashMap;
use std::io::{Read, Write};

const WS_MAX_PAYLOAD_LEN: usize = 65_535;
const WS_ACCEPT_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.net.ws.server",
    kind: "net.ws.server",
    type_name: "NetWsServer",
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
            let port = expect_int(&args[1], span)?;
            if !(1..=65535).contains(&port) {
                return Err(IcooError::runtime(
                    "server port must be between 1 and 65535",
                    Some(span),
                ));
            }
            let handler = args[2].clone();
            if !is_callable(&handler) {
                return Err(IcooError::runtime(
                    "websocket handler must be callable",
                    Some(span),
                ));
            }
            serve_once(runtime, &host, port as u16, handler, span)?;
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
    let listener = std::net::TcpListener::bind((host, port)).map_err(|err| {
        IcooError::runtime(format!("websocket server bind failed: {}", err), Some(span))
    })?;
    let (mut stream, _) = listener.accept().map_err(|err| {
        IcooError::runtime(
            format!("websocket server accept failed: {}", err),
            Some(span),
        )
    })?;
    let headers = read_upgrade_request(&mut stream, span)?;
    let key = headers.get("sec-websocket-key").ok_or_else(|| {
        IcooError::runtime("websocket upgrade missing Sec-WebSocket-Key", Some(span))
    })?;
    let accept = websocket_accept(key);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept
    );
    stream.write_all(response.as_bytes()).map_err(|err| {
        IcooError::runtime(
            format!("websocket server handshake write failed: {}", err),
            Some(span),
        )
    })?;

    let frame = read_frame(&mut stream, true, span)?;
    if frame.opcode == 0x8 {
        let _ = write_frame(&mut stream, 0x8, &[], false, span);
        return Ok(());
    }
    if !matches!(frame.opcode, 0x1 | 0x2) {
        return Err(IcooError::runtime(
            format!("unsupported websocket request opcode: {}", frame.opcode),
            Some(span),
        ));
    }

    let reply = call_handler(runtime, handler, frame.payload, span)?;
    match reply {
        Value::Bytes(bytes) => write_frame(&mut stream, 0x2, &bytes, false, span),
        other => write_frame(&mut stream, 0x1, other.display().as_bytes(), false, span),
    }
}

fn call_handler(
    runtime: &mut Interpreter,
    handler: Value,
    payload: Vec<u8>,
    span: Span,
) -> IcooResult<Value> {
    runtime.call_value(handler, vec![Value::Bytes(std::rc::Rc::new(payload))], span)
}

fn read_upgrade_request(
    stream: &mut std::net::TcpStream,
    span: Span,
) -> IcooResult<HashMap<String, String>> {
    let head = read_http_head(stream, span)?;
    let mut lines = head.lines();
    let request_line = lines.next().ok_or_else(|| {
        IcooError::runtime(
            "invalid websocket request: missing request line",
            Some(span),
        )
    })?;
    if !request_line.starts_with("GET ") {
        return Err(IcooError::runtime(
            "websocket upgrade must use GET",
            Some(span),
        ));
    }
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let upgrade = headers
        .get("upgrade")
        .map(|value| value.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);
    if !upgrade {
        return Err(IcooError::runtime(
            "websocket upgrade header is required",
            Some(span),
        ));
    }
    Ok(headers)
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
                "invalid websocket HTTP request: missing header terminator",
                Some(span),
            ));
        }
        bytes.extend_from_slice(&buffer[..size]);
        if let Some(index) = find_header_end(&bytes) {
            return Ok(String::from_utf8_lossy(&bytes[..index]).into_owned());
        }
        if bytes.len() > 16 * 1024 {
            return Err(IcooError::runtime(
                "websocket HTTP headers are too large",
                Some(span),
            ));
        }
    }
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 2)
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
            "client websocket frames must be masked"
        } else {
            "expected an unmasked websocket frame"
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
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.push(0x80 | opcode);
    let mask_bit = if masked { 0x80 } else { 0 };
    if payload.len() <= 125 {
        frame.push(mask_bit | payload.len() as u8);
    } else {
        frame.push(mask_bit | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(payload);
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

fn websocket_accept(key: &str) -> String {
    let mut input = Vec::with_capacity(key.len() + WS_ACCEPT_GUID.len());
    input.extend_from_slice(key.as_bytes());
    input.extend_from_slice(WS_ACCEPT_GUID);
    bytes_to_base64(&sha1(&input))
}

fn sha1(input: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xefcdab89;
    let mut h2: u32 = 0x98badcfe;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xc3d2e1f0;

    let bit_len = (input.len() as u64) * 8;
    let mut message = input.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.chunks_exact(64) {
        let mut w = [0_u32; 80];
        for (index, word) in w.iter_mut().take(16).enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for index in 16..80 {
            w[index] = (w[index - 3] ^ w[index - 8] ^ w[index - 14] ^ w[index - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;
        for (index, word) in w.iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5a827999),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut output = [0_u8; 20];
    output[..4].copy_from_slice(&h0.to_be_bytes());
    output[4..8].copy_from_slice(&h1.to_be_bytes());
    output[8..12].copy_from_slice(&h2.to_be_bytes());
    output[12..16].copy_from_slice(&h3.to_be_bytes());
    output[16..20].copy_from_slice(&h4.to_be_bytes());
    output
}
