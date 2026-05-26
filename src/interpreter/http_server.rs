use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::runtime::permissions::RuntimePermissions;

pub(crate) fn http_server_serve_once(
    permissions: &RuntimePermissions,
    host: &str,
    port: u16,
    body: &str,
    span: Span,
) -> IcooResult<()> {
    permissions.check_net_listen_endpoint(host, port, span)?;
    let listener = std::net::TcpListener::bind((host, port)).map_err(|err| {
        IcooError::runtime(format!("http server bind failed: {}", err), Some(span))
    })?;
    let (mut stream, _) = listener.accept().map_err(|err| {
        IcooError::runtime(format!("http server accept failed: {}", err), Some(span))
    })?;
    let mut buffer = [0_u8; 1024];
    let _ = std::io::Read::read(&mut stream, &mut buffer);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    std::io::Write::write_all(&mut stream, response.as_bytes())
        .map_err(|err| IcooError::runtime(format!("http server write failed: {}", err), Some(span)))
}
