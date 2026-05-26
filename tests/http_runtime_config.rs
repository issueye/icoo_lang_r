use icoo_lang_r::runtime::http_config::{HttpProxyConfig, RuntimeHttpConfig, DEFAULT_HTTP_TIMEOUT};
use std::time::Duration;

#[test]
fn default_http_config_preserves_current_runtime_behavior() {
    let config = RuntimeHttpConfig::default();

    assert_eq!(config.connect_timeout, DEFAULT_HTTP_TIMEOUT);
    assert_eq!(config.read_timeout, Duration::from_secs(5));
    assert_eq!(config.write_timeout, Duration::from_secs(5));
    assert_eq!(config.max_redirects, 0);
    assert_eq!(config.proxy, None);
}

#[test]
fn new_matches_default_http_config() {
    assert_eq!(RuntimeHttpConfig::new(), RuntimeHttpConfig::default());
}

#[test]
fn builder_sets_timeouts_and_redirect_limit() {
    let config = RuntimeHttpConfig::new()
        .with_timeouts(
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(3),
        )
        .with_max_redirects(4);

    assert_eq!(config.connect_timeout, Duration::from_secs(1));
    assert_eq!(config.read_timeout, Duration::from_secs(2));
    assert_eq!(config.write_timeout, Duration::from_secs(3));
    assert_eq!(config.max_redirects, 4);
}

#[test]
fn proxy_config_stores_host_port_and_optional_authorization() {
    let proxy = HttpProxyConfig::new("proxy.internal", 8080)
        .unwrap()
        .with_authorization("Basic dXNlcjpwYXNz");

    assert_eq!(proxy.host, "proxy.internal");
    assert_eq!(proxy.port, 8080);
    assert_eq!(proxy.authorization.as_deref(), Some("Basic dXNlcjpwYXNz"));
}

#[test]
fn config_builder_accepts_valid_proxy() {
    let proxy = HttpProxyConfig::new("proxy.internal", 3128).unwrap();
    let config = RuntimeHttpConfig::default()
        .with_proxy(proxy.clone())
        .unwrap();

    assert_eq!(config.proxy, Some(proxy));
}

#[test]
fn proxy_constructor_rejects_zero_port() {
    let error = HttpProxyConfig::new("proxy.internal", 0).unwrap_err();

    assert_eq!(error, "http proxy port must be non-zero");
}

#[test]
fn config_builder_rejects_zero_port_proxy() {
    let proxy = HttpProxyConfig {
        host: "proxy.internal".to_string(),
        port: 0,
        authorization: None,
    };
    let error = RuntimeHttpConfig::default().with_proxy(proxy).unwrap_err();

    assert_eq!(error, "http proxy port must be non-zero");
}
