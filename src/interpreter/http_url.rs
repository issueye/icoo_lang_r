use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use std::net::Ipv6Addr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HttpScheme {
    Http,
    Https,
}

impl HttpScheme {
    pub(crate) fn default_port(self) -> u16 {
        match self {
            Self::Http => 80,
            Self::Https => 443,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedHttpUrl {
    pub(crate) scheme: HttpScheme,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path: String,
    is_ipv6_literal: bool,
}

impl ParsedHttpUrl {
    pub(crate) fn parse(url: &str, span: Span) -> IcooResult<Self> {
        let (scheme, rest) = if let Some(rest) = url.strip_prefix("http://") {
            (HttpScheme::Http, rest)
        } else if let Some(rest) = url.strip_prefix("https://") {
            (HttpScheme::Https, rest)
        } else {
            return Err(IcooError::runtime(
                "only http:// and https:// URLs are supported",
                Some(span),
            ));
        };

        let (authority, path) = split_authority_and_path(rest);
        if authority.is_empty() {
            return Err(IcooError::runtime("URL host is required", Some(span)));
        }

        let default_port = scheme.default_port();
        let (host, port, is_ipv6_literal) = parse_authority(authority, default_port, span)?;
        Ok(Self {
            scheme,
            host,
            port,
            path,
            is_ipv6_literal,
        })
    }

    pub(crate) fn host_header(&self) -> String {
        let host = if self.is_ipv6_literal {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };

        if self.port == self.scheme.default_port() {
            host
        } else {
            format!("{}:{}", host, self.port)
        }
    }

    pub(crate) fn connect_host(&self) -> String {
        if self.is_ipv6_literal {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

fn split_authority_and_path(rest: &str) -> (&str, String) {
    match rest.find(['/', '?']) {
        Some(index) if rest.as_bytes()[index] == b'/' => {
            let (authority, path) = rest.split_at(index);
            (authority, path.to_string())
        }
        Some(index) => {
            let (authority, query) = rest.split_at(index);
            (authority, format!("/{}", query))
        }
        None => (rest, "/".to_string()),
    }
}

fn parse_authority(
    authority: &str,
    default_port: u16,
    span: Span,
) -> IcooResult<(String, u16, bool)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after_host) = rest.split_once(']').ok_or_else(|| {
            IcooError::runtime("IPv6 URL host is missing closing bracket", Some(span))
        })?;
        if host.is_empty() {
            return Err(IcooError::runtime("URL host is required", Some(span)));
        }
        host.parse::<Ipv6Addr>()
            .map_err(|_| IcooError::runtime("invalid IPv6 URL host", Some(span)))?;
        let port = if after_host.is_empty() {
            default_port
        } else if let Some(port) = after_host.strip_prefix(':') {
            parse_port(port, span)?
        } else {
            return Err(IcooError::runtime(
                "invalid IPv6 URL host: expected port after closing bracket",
                Some(span),
            ));
        };
        return Ok((host.to_string(), port, true));
    }

    if authority.contains(']') {
        return Err(IcooError::runtime(
            "invalid IPv6 URL host: unexpected closing bracket",
            Some(span),
        ));
    }
    if authority.matches(':').count() > 1 {
        return Err(IcooError::runtime(
            "IPv6 URL hosts must be enclosed in brackets",
            Some(span),
        ));
    }

    let (host, port) = if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() {
            return Err(IcooError::runtime("URL host is required", Some(span)));
        }
        (host, parse_port(port, span)?)
    } else {
        (authority, default_port)
    };
    if host.is_empty() {
        return Err(IcooError::runtime("URL host is required", Some(span)));
    }

    Ok((host.to_string(), port, false))
}

fn parse_port(port: &str, span: Span) -> IcooResult<u16> {
    let port = port
        .parse::<u16>()
        .map_err(|_| IcooError::runtime("URL port must be between 1 and 65535", Some(span)))?;
    if port == 0 {
        return Err(IcooError::runtime(
            "URL port must be between 1 and 65535",
            Some(span),
        ));
    }
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Span {
        Span::new(1, 1, 0, 1)
    }

    fn parse(url: &str) -> ParsedHttpUrl {
        ParsedHttpUrl::parse(url, span()).unwrap()
    }

    fn error_text(url: &str) -> String {
        ParsedHttpUrl::parse(url, span()).unwrap_err().to_string()
    }

    #[test]
    fn parses_http_url_with_default_port_and_path() {
        let parsed = parse("http://example.com/path");

        assert_eq!(parsed.scheme, HttpScheme::Http);
        assert_eq!(parsed.host, "example.com");
        assert_eq!(parsed.port, 80);
        assert_eq!(parsed.path, "/path");
        assert_eq!(parsed.host_header(), "example.com");
        assert_eq!(parsed.connect_host(), "example.com:80");
    }

    #[test]
    fn parses_https_url_with_default_path() {
        let parsed = parse("https://example.com");

        assert_eq!(parsed.scheme, HttpScheme::Https);
        assert_eq!(parsed.host, "example.com");
        assert_eq!(parsed.port, 443);
        assert_eq!(parsed.path, "/");
        assert_eq!(parsed.host_header(), "example.com");
        assert_eq!(parsed.connect_host(), "example.com:443");
    }

    #[test]
    fn parses_explicit_non_default_port() {
        let parsed = parse("https://example.com:8443/api");

        assert_eq!(parsed.port, 8443);
        assert_eq!(parsed.host_header(), "example.com:8443");
        assert_eq!(parsed.connect_host(), "example.com:8443");
    }

    #[test]
    fn omits_explicit_default_port_from_host_header() {
        let parsed = parse("http://example.com:80/");

        assert_eq!(parsed.port, 80);
        assert_eq!(parsed.host_header(), "example.com");
    }

    #[test]
    fn parses_bracketed_ipv6_with_port_path_and_query() {
        let parsed = parse("http://[::1]:8080/path?q=1");

        assert_eq!(parsed.scheme, HttpScheme::Http);
        assert_eq!(parsed.host, "::1");
        assert_eq!(parsed.port, 8080);
        assert_eq!(parsed.path, "/path?q=1");
        assert_eq!(parsed.host_header(), "[::1]:8080");
        assert_eq!(parsed.connect_host(), "[::1]:8080");
    }

    #[test]
    fn parses_bracketed_ipv6_with_default_https_port() {
        let parsed = parse("https://[2001:db8::1]/");

        assert_eq!(parsed.scheme, HttpScheme::Https);
        assert_eq!(parsed.host, "2001:db8::1");
        assert_eq!(parsed.port, 443);
        assert_eq!(parsed.path, "/");
        assert_eq!(parsed.host_header(), "[2001:db8::1]");
        assert_eq!(parsed.connect_host(), "[2001:db8::1]:443");
    }

    #[test]
    fn preserves_query_without_explicit_path() {
        let parsed = parse("http://example.com?q=1");

        assert_eq!(parsed.path, "/?q=1");
    }

    #[test]
    fn rejects_empty_host() {
        assert!(error_text("http:///path").contains("URL host is required"));
    }

    #[test]
    fn keeps_unsupported_scheme_error_text_stable() {
        assert!(error_text("ftp://example.com")
            .contains("only http:// and https:// URLs are supported"));
    }

    #[test]
    fn rejects_invalid_port() {
        assert!(
            error_text("http://example.com:0/").contains("URL port must be between 1 and 65535")
        );
        assert!(
            error_text("http://example.com:abc/").contains("URL port must be between 1 and 65535")
        );
    }

    #[test]
    fn rejects_unbracketed_ipv6() {
        assert!(error_text("http://::1/").contains("IPv6 URL hosts must be enclosed in brackets"));
    }

    #[test]
    fn rejects_malformed_bracketed_ipv6() {
        assert!(error_text("http://[::1/path").contains("missing closing bracket"));
        assert!(error_text("http://[not-ipv6]/").contains("invalid IPv6 URL host"));
    }
}
