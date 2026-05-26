pub(crate) const ALPN_HTTP_11: &[u8] = b"http/1.1";
pub(crate) const ALPN_H2: &[u8] = b"h2";

pub(crate) const HTTP_11_ONLY_ALPN_PROTOCOLS: &[&[u8]] = &[ALPN_HTTP_11];
pub(crate) const H2_AND_HTTP_11_ALPN_PROTOCOLS: &[&[u8]] = &[ALPN_H2, ALPN_HTTP_11];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HttpAlpnPolicy {
    Http11Only,
    #[allow(dead_code)]
    H2AndHttp11,
}

impl Default for HttpAlpnPolicy {
    fn default() -> Self {
        Self::Http11Only
    }
}

impl HttpAlpnPolicy {
    pub(crate) fn protocols(self) -> &'static [&'static [u8]] {
        match self {
            Self::Http11Only => HTTP_11_ONLY_ALPN_PROTOCOLS,
            Self::H2AndHttp11 => H2_AND_HTTP_11_ALPN_PROTOCOLS,
        }
    }

    pub(crate) fn rustls_protocols(self) -> Vec<Vec<u8>> {
        self.protocols()
            .iter()
            .map(|protocol| protocol.to_vec())
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NegotiatedHttpProtocol {
    Http11,
    H2,
    Unknown,
}

pub(crate) fn negotiated_http_protocol(alpn_protocol: Option<&[u8]>) -> NegotiatedHttpProtocol {
    match alpn_protocol {
        None => NegotiatedHttpProtocol::Http11,
        Some(ALPN_HTTP_11) => NegotiatedHttpProtocol::Http11,
        Some(ALPN_H2) => NegotiatedHttpProtocol::H2,
        Some(_) => NegotiatedHttpProtocol::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_alpn_default_policy_only_advertises_http_11() {
        assert_eq!(
            HttpAlpnPolicy::default().protocols(),
            &[ALPN_HTTP_11] as &[&[u8]]
        );
        assert_eq!(
            HttpAlpnPolicy::default().rustls_protocols(),
            vec![b"http/1.1".to_vec()]
        );
    }

    #[test]
    fn http_alpn_future_h2_policy_prefers_h2_then_http_11() {
        assert_eq!(
            HttpAlpnPolicy::H2AndHttp11.protocols(),
            &[ALPN_H2, ALPN_HTTP_11] as &[&[u8]]
        );
        assert_eq!(
            HttpAlpnPolicy::H2AndHttp11.rustls_protocols(),
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        );
    }

    #[test]
    fn http_alpn_classifies_negotiated_protocols() {
        assert_eq!(
            negotiated_http_protocol(None),
            NegotiatedHttpProtocol::Http11
        );
        assert_eq!(
            negotiated_http_protocol(Some(b"http/1.1")),
            NegotiatedHttpProtocol::Http11
        );
        assert_eq!(
            negotiated_http_protocol(Some(b"h2")),
            NegotiatedHttpProtocol::H2
        );
        assert_eq!(
            negotiated_http_protocol(Some(b"spdy/3")),
            NegotiatedHttpProtocol::Unknown
        );
    }
}
