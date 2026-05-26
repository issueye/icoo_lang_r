use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;

pub const MAX_BYTES_LEN: usize = 64 * 1024 * 1024;
pub const MAX_HTTP_BODY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_HTTP_STREAM_CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_WEB_INO_REQUEST_BYTES: usize = 16 * 1024 * 1024;

pub fn check_bytes_len(len: usize, span: Span) -> IcooResult<()> {
    if len > MAX_BYTES_LEN {
        return Err(IcooError::runtime(
            format!(
                "bytes value exceeds maximum size: {} bytes (max {})",
                len, MAX_BYTES_LEN
            ),
            Some(span),
        ));
    }
    Ok(())
}

pub fn checked_bytes_total(left: usize, right: usize, span: Span) -> IcooResult<usize> {
    let total = left.checked_add(right).ok_or_else(|| {
        IcooError::runtime("bytes value exceeds maximum size: overflow", Some(span))
    })?;
    check_bytes_len(total, span)?;
    Ok(total)
}

pub fn check_http_body_len(len: usize, span: Span) -> IcooResult<()> {
    if len > MAX_HTTP_BODY_BYTES {
        return Err(IcooError::runtime(
            format!(
                "http response body exceeds maximum size: {} bytes (max {})",
                len, MAX_HTTP_BODY_BYTES
            ),
            Some(span),
        ));
    }
    Ok(())
}

pub fn check_http_stream_chunk_len(len: usize, span: Span) -> IcooResult<()> {
    if len > MAX_HTTP_STREAM_CHUNK_BYTES {
        return Err(IcooError::runtime(
            format!(
                "http stream chunk exceeds maximum size: {} bytes (max {})",
                len, MAX_HTTP_STREAM_CHUNK_BYTES
            ),
            Some(span),
        ));
    }
    Ok(())
}

pub fn check_web_ino_request_len(len: usize) -> Result<(), String> {
    if len > MAX_WEB_INO_REQUEST_BYTES {
        return Err(format!(
            "web.ino request body exceeds maximum size: {} bytes (max {})",
            len, MAX_WEB_INO_REQUEST_BYTES
        ));
    }
    Ok(())
}
