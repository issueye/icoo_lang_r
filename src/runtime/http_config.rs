use std::time::Duration;

pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHttpConfig {
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub max_redirects: usize,
    pub proxy: Option<HttpProxyConfig>,
}

impl RuntimeHttpConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_timeouts(
        mut self,
        connect_timeout: Duration,
        read_timeout: Duration,
        write_timeout: Duration,
    ) -> Self {
        self.connect_timeout = connect_timeout;
        self.read_timeout = read_timeout;
        self.write_timeout = write_timeout;
        self
    }

    pub fn with_max_redirects(mut self, max_redirects: usize) -> Self {
        self.max_redirects = max_redirects;
        self
    }

    pub fn with_proxy(mut self, proxy: HttpProxyConfig) -> Result<Self, String> {
        validate_proxy_port(proxy.port)?;
        self.proxy = Some(proxy);
        Ok(self)
    }
}

impl Default for RuntimeHttpConfig {
    fn default() -> Self {
        Self {
            connect_timeout: DEFAULT_HTTP_TIMEOUT,
            read_timeout: DEFAULT_HTTP_TIMEOUT,
            write_timeout: DEFAULT_HTTP_TIMEOUT,
            max_redirects: 0,
            proxy: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpProxyConfig {
    pub host: String,
    pub port: u16,
    pub authorization: Option<String>,
}

impl HttpProxyConfig {
    pub fn new(host: impl Into<String>, port: u16) -> Result<Self, String> {
        validate_proxy_port(port)?;
        Ok(Self {
            host: host.into(),
            port,
            authorization: None,
        })
    }

    pub fn with_authorization(mut self, authorization: impl Into<String>) -> Self {
        self.authorization = Some(authorization.into());
        self
    }
}

fn validate_proxy_port(port: u16) -> Result<(), String> {
    if port == 0 {
        return Err("http proxy port must be non-zero".to_string());
    }
    Ok(())
}
