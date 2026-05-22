use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;

pub(crate) fn find_http_body_start(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

pub(crate) fn http_content_length(head: &str) -> Option<usize> {
    head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            value.trim().parse::<usize>().ok()
        } else {
            None
        }
    })
}

pub(crate) fn http_status_text(status: i64) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

pub(crate) fn ensure_http_header_name_value_no_crlf(
    name: &str,
    value: &str,
    span: Span,
) -> IcooResult<()> {
    if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
        return Err(IcooError::runtime(
            "HTTP header names and values cannot contain CR or LF",
            Some(span),
        ));
    }
    Ok(())
}
