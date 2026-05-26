#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HttpProxyTarget {
    host: String,
    port: u16,
}

impl HttpProxyTarget {
    pub(crate) fn new(host: impl Into<String>, port: u16) -> Result<Self, String> {
        let host = host.into();
        validate_host_port(&host, port)?;
        Ok(Self { host, port })
    }

    pub(crate) fn absolute_form_http_target(&self, path: &str) -> String {
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        format!("http://{}:{}{}", format_host(&self.host), self.port, path)
    }

    pub(crate) fn connect_request(&self, proxy: &HttpProxyConfig) -> String {
        let authority = self.authority();
        let mut request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
        if let Some(authorization) = proxy.authorization() {
            request.push_str("Proxy-Authorization: ");
            request.push_str(authorization);
            request.push_str("\r\n");
        }
        request.push_str("\r\n");
        request
    }

    fn authority(&self) -> String {
        format!("{}:{}", format_host(&self.host), self.port)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HttpProxyConfig {
    host: String,
    port: u16,
    authorization: Option<String>,
}

impl HttpProxyConfig {
    pub(crate) fn new(
        host: impl Into<String>,
        port: u16,
        authorization: Option<String>,
    ) -> Result<Self, String> {
        let host = host.into();
        validate_host_port(&host, port)?;
        Ok(Self {
            host,
            port,
            authorization,
        })
    }

    pub(crate) fn host(&self) -> &str {
        &self.host
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn authorization(&self) -> Option<&str> {
        self.authorization.as_deref()
    }
}

fn validate_host_port(host: &str, port: u16) -> Result<(), String> {
    if host.trim().is_empty() {
        return Err("proxy host is required".to_string());
    }
    if port == 0 {
        return Err("proxy port must be between 1 and 65535".to_string());
    }
    Ok(())
}

fn format_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{HttpProxyConfig, HttpProxyTarget};

    #[test]
    fn http_proxy_rejects_empty_host() {
        assert_eq!(
            HttpProxyConfig::new("", 8080, None).unwrap_err(),
            "proxy host is required"
        );
        assert_eq!(
            HttpProxyTarget::new("  ", 80).unwrap_err(),
            "proxy host is required"
        );
    }

    #[test]
    fn http_proxy_rejects_zero_port() {
        assert_eq!(
            HttpProxyConfig::new("proxy.local", 0, None).unwrap_err(),
            "proxy port must be between 1 and 65535"
        );
        assert_eq!(
            HttpProxyTarget::new("example.com", 0).unwrap_err(),
            "proxy port must be between 1 and 65535"
        );
    }

    #[test]
    fn stores_proxy_configuration() {
        let proxy =
            HttpProxyConfig::new("proxy.local", 3128, Some("Basic dXNlcjpwYXNz".to_string()))
                .unwrap();

        assert_eq!(proxy.host(), "proxy.local");
        assert_eq!(proxy.port(), 3128);
        assert_eq!(proxy.authorization(), Some("Basic dXNlcjpwYXNz"));
    }

    #[test]
    fn builds_absolute_form_target_for_plain_http_proxy_request() {
        let target = HttpProxyTarget::new("example.com", 80).unwrap();

        assert_eq!(
            target.absolute_form_http_target("/search?q=rust"),
            "http://example.com:80/search?q=rust"
        );
        assert_eq!(
            target.absolute_form_http_target("plain/path"),
            "http://example.com:80/plain/path"
        );
    }

    #[test]
    fn builds_connect_request_without_proxy_authorization() {
        let proxy = HttpProxyConfig::new("proxy.local", 3128, None).unwrap();
        let target = HttpProxyTarget::new("example.com", 443).unwrap();

        assert_eq!(
            target.connect_request(&proxy),
            "CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n"
        );
    }

    #[test]
    fn builds_connect_request_with_proxy_authorization() {
        let proxy =
            HttpProxyConfig::new("proxy.local", 3128, Some("Basic dXNlcjpwYXNz".to_string()))
                .unwrap();
        let target = HttpProxyTarget::new("secure.example", 443).unwrap();

        assert_eq!(
            target.connect_request(&proxy),
            "CONNECT secure.example:443 HTTP/1.1\r\nHost: secure.example:443\r\nProxy-Authorization: Basic dXNlcjpwYXNz\r\n\r\n"
        );
    }

    #[test]
    fn brackets_ipv6_hosts_in_proxy_request_targets() {
        let proxy = HttpProxyConfig::new("proxy.local", 3128, None).unwrap();
        let target = HttpProxyTarget::new("2001:db8::1", 443).unwrap();

        assert_eq!(
            target.absolute_form_http_target("/"),
            "http://[2001:db8::1]:443/"
        );
        assert_eq!(
            target.connect_request(&proxy),
            "CONNECT [2001:db8::1]:443 HTTP/1.1\r\nHost: [2001:db8::1]:443\r\n\r\n"
        );
    }
}
