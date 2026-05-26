use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedirectRequest {
    pub url: String,
    pub method: String,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RedirectError {
    TooManyRedirects { max_redirects: usize },
    MissingLocation,
    UnsupportedUrl(String),
}

impl fmt::Display for RedirectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyRedirects { max_redirects } => {
                write!(f, "maximum redirect count exceeded ({})", max_redirects)
            }
            Self::MissingLocation => write!(f, "redirect response is missing Location header"),
            Self::UnsupportedUrl(message) => write!(f, "{}", message),
        }
    }
}

impl std::error::Error for RedirectError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedRedirectUrl<'a> {
    scheme: &'a str,
    authority: &'a str,
    path: &'a str,
    query: Option<&'a str>,
}

pub(crate) fn is_redirect_status(status: i64) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

pub(crate) fn redirect_location(headers: &HashMap<String, String>) -> Option<&str> {
    headers.get("location").map(|location| location.trim())
}

pub(crate) fn redirect_request(
    status: i64,
    headers: &HashMap<String, String>,
    current_url: &str,
    method: &str,
    body: &[u8],
    redirect_count: usize,
    max_redirects: usize,
) -> Result<Option<RedirectRequest>, RedirectError> {
    if !is_redirect_status(status) {
        return Ok(None);
    }
    if redirect_count >= max_redirects {
        return Err(RedirectError::TooManyRedirects { max_redirects });
    }
    let location = redirect_location(headers).ok_or(RedirectError::MissingLocation)?;
    let url = resolve_redirect_url(current_url, location)?;
    let (method, body) = redirect_method_and_body(status, method, body);
    Ok(Some(RedirectRequest { url, method, body }))
}

pub(crate) fn resolve_redirect_url(
    current_url: &str,
    location: &str,
) -> Result<String, RedirectError> {
    let location = location.trim();
    if location.is_empty() {
        return Err(RedirectError::UnsupportedUrl(
            "redirect Location header is empty".to_string(),
        ));
    }
    if is_absolute_url(location) {
        validate_supported_url(location)?;
        return Ok(location.to_string());
    }

    let base = parse_redirect_url(current_url)?;
    if let Some(authority_path) = location.strip_prefix("//") {
        return Ok(format!("{}://{}", base.scheme, authority_path));
    }
    if location.starts_with('/') {
        return Ok(format!("{}://{}{}", base.scheme, base.authority, location));
    }
    if location.starts_with('?') {
        return Ok(format!(
            "{}://{}{}{}",
            base.scheme, base.authority, base.path, location
        ));
    }
    if location.starts_with('#') {
        let query = base
            .query
            .map(|query| format!("?{}", query))
            .unwrap_or_default();
        return Ok(format!(
            "{}://{}{}{}{}",
            base.scheme, base.authority, base.path, query, location
        ));
    }

    let base_dir = base_directory(base.path);
    let joined_path = normalize_path(&format!("{}{}", base_dir, location));
    Ok(format!(
        "{}://{}{}",
        base.scheme, base.authority, joined_path
    ))
}

pub(crate) fn redirect_method_and_body(
    status: i64,
    method: &str,
    body: &[u8],
) -> (String, Vec<u8>) {
    if status == 303 || ((status == 301 || status == 302) && method.eq_ignore_ascii_case("POST")) {
        ("GET".to_string(), Vec::new())
    } else {
        (method.to_string(), body.to_vec())
    }
}

fn is_absolute_url(url: &str) -> bool {
    url.contains("://")
}

fn validate_supported_url(url: &str) -> Result<(), RedirectError> {
    parse_redirect_url(url).map(|_| ())
}

fn parse_redirect_url(url: &str) -> Result<ParsedRedirectUrl<'_>, RedirectError> {
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("http://") {
        ("http", rest)
    } else if let Some(rest) = url.strip_prefix("https://") {
        ("https", rest)
    } else {
        return Err(RedirectError::UnsupportedUrl(
            "only http:// and https:// redirect URLs are supported".to_string(),
        ));
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err(RedirectError::UnsupportedUrl(
            "redirect URL host is required".to_string(),
        ));
    }

    let after_authority = &rest[authority_end..];
    let without_fragment = after_authority
        .split_once('#')
        .map(|(head, _)| head)
        .unwrap_or(after_authority);
    let (path, query) = without_fragment
        .split_once('?')
        .map(|(path, query)| (path, Some(query)))
        .unwrap_or((without_fragment, None));
    let path = if path.is_empty() { "/" } else { path };

    Ok(ParsedRedirectUrl {
        scheme,
        authority,
        path,
        query,
    })
}

fn base_directory(path: &str) -> String {
    if path.ends_with('/') {
        return path.to_string();
    }
    match path.rsplit_once('/') {
        Some((dir, _)) if dir.is_empty() => "/".to_string(),
        Some((dir, _)) => format!("{}/", dir),
        None => "/".to_string(),
    }
}

fn normalize_path(path: &str) -> String {
    let (path, suffix) = split_path_suffix(path);
    let absolute = path.starts_with('/');
    let trailing_slash = path.ends_with('/');
    let mut parts = Vec::new();

    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }

    let mut normalized = String::new();
    if absolute {
        normalized.push('/');
    }
    normalized.push_str(&parts.join("/"));
    if trailing_slash && !normalized.ends_with('/') {
        normalized.push('/');
    }
    if normalized.is_empty() {
        normalized.push('/');
    }
    normalized.push_str(suffix);
    normalized
}

fn split_path_suffix(path: &str) -> (&str, &str) {
    let suffix_start = path.find(['?', '#']).unwrap_or(path.len());
    (&path[..suffix_start], &path[suffix_start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(location: &str) -> HashMap<String, String> {
        HashMap::from([("location".to_string(), location.to_string())])
    }

    #[test]
    fn http_redirect_status_detection_matches_redirect_codes() {
        for status in [301, 302, 303, 307, 308] {
            assert!(is_redirect_status(status));
        }
        for status in [200, 300, 304, 400, 500] {
            assert!(!is_redirect_status(status));
        }
    }

    #[test]
    fn http_redirect_extracts_location_from_lowercased_headers() {
        let headers = headers(" /next ");
        assert_eq!(redirect_location(&headers), Some("/next"));
    }

    #[test]
    fn http_redirect_ignores_non_redirect_status() {
        let request = redirect_request(
            200,
            &headers("/next"),
            "https://example.com/start",
            "POST",
            b"body",
            0,
            0,
        )
        .unwrap();
        assert_eq!(request, None);
    }

    #[test]
    fn http_redirect_requires_location_header() {
        let err = redirect_request(
            302,
            &HashMap::new(),
            "https://example.com/start",
            "GET",
            b"",
            0,
            1,
        )
        .unwrap_err();
        assert_eq!(err, RedirectError::MissingLocation);
    }

    #[test]
    fn http_redirect_enforces_max_redirects() {
        let err = redirect_request(
            302,
            &headers("/next"),
            "https://example.com/start",
            "GET",
            b"",
            3,
            3,
        )
        .unwrap_err();
        assert_eq!(err, RedirectError::TooManyRedirects { max_redirects: 3 });
    }

    #[test]
    fn http_redirect_resolves_absolute_location() {
        let url = resolve_redirect_url("https://example.com/start", "http://other.test/path?q=1")
            .unwrap();
        assert_eq!(url, "http://other.test/path?q=1");
    }

    #[test]
    fn http_redirect_resolves_scheme_relative_location() {
        let url =
            resolve_redirect_url("https://example.com/start", "//cdn.example.com/file").unwrap();
        assert_eq!(url, "https://cdn.example.com/file");
    }

    #[test]
    fn http_redirect_resolves_root_relative_location() {
        let url = resolve_redirect_url("https://example.com/a/b?old=1", "/next?new=1").unwrap();
        assert_eq!(url, "https://example.com/next?new=1");
    }

    #[test]
    fn http_redirect_resolves_directory_relative_location() {
        let url = resolve_redirect_url("https://example.com/a/b/c?old=1", "../next?new=1").unwrap();
        assert_eq!(url, "https://example.com/a/next?new=1");
    }

    #[test]
    fn http_redirect_resolves_query_relative_location() {
        let url = resolve_redirect_url("https://example.com/a/b?old=1", "?new=1").unwrap();
        assert_eq!(url, "https://example.com/a/b?new=1");
    }

    #[test]
    fn http_redirect_rejects_unsupported_absolute_location_scheme() {
        let err = resolve_redirect_url("https://example.com/start", "ftp://example.com/file")
            .unwrap_err();
        assert_eq!(
            err,
            RedirectError::UnsupportedUrl(
                "only http:// and https:// redirect URLs are supported".to_string()
            )
        );
    }

    #[test]
    fn http_redirect_rewrites_303_to_get_and_drops_body() {
        let (method, body) = redirect_method_and_body(303, "PUT", b"body");
        assert_eq!(method, "GET");
        assert!(body.is_empty());
    }

    #[test]
    fn http_redirect_rewrites_301_302_post_to_get_and_drops_body() {
        for status in [301, 302] {
            let (method, body) = redirect_method_and_body(status, "post", b"body");
            assert_eq!(method, "GET");
            assert!(body.is_empty());
        }
    }

    #[test]
    fn http_redirect_preserves_method_and_body_for_307_308() {
        for status in [307, 308] {
            let (method, body) = redirect_method_and_body(status, "POST", b"body");
            assert_eq!(method, "POST");
            assert_eq!(body, b"body");
        }
    }
}
