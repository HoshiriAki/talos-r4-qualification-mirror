//! Trusted reverse-proxy boundary for R4-P4.
//!
//! Forwarded headers are ambient strings until the socket peer is proven to be
//! a configured proxy. This middleware resolves one client IP, removes every
//! raw forwarding header, and leaves downstream auth/rate-limit code only a
//! sanitized `X-Real-IP` value when proxy mode is enabled. Forwarded scheme and
//! host are not current TALOS authority inputs and therefore never propagate.

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use ipnet::IpNet;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");
const X_REAL_IP: HeaderName = HeaderName::from_static("x-real-ip");
const X_FORWARDED_PROTO: HeaderName = HeaderName::from_static("x-forwarded-proto");
const X_FORWARDED_HOST: HeaderName = HeaderName::from_static("x-forwarded-host");

#[derive(Clone, Debug)]
pub struct TrustedProxyPolicy {
    trusted_hops: usize,
    trusted_networks: Vec<IpNet>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedClientIp(pub IpAddr);

impl TrustedProxyPolicy {
    pub fn from_env() -> Self {
        let trusted_hops = std::env::var("TRUSTED_PROXY_COUNT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let raw = std::env::var("TRUSTED_PROXY_CIDRS").unwrap_or_default();
        let candidates: Vec<&str> = raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        let parsed: Result<Vec<IpNet>, _> = candidates.iter().map(|value| value.parse()).collect();

        // Invalid CIDR configuration fails closed: no socket peer is trusted.
        let trusted_networks = parsed.unwrap_or_default();
        Self {
            trusted_hops,
            trusted_networks,
        }
    }

    #[cfg(test)]
    fn new(trusted_hops: usize, trusted_networks: Vec<IpNet>) -> Self {
        Self {
            trusted_hops,
            trusted_networks,
        }
    }

    fn is_trusted(&self, ip: IpAddr) -> bool {
        self.trusted_hops > 0
            && self
                .trusted_networks
                .iter()
                .any(|network| network.contains(&ip))
    }

    pub fn resolve_client_ip(&self, peer_ip: IpAddr, headers: &HeaderMap) -> IpAddr {
        if !self.is_trusted(peer_ip) {
            return peer_ip;
        }

        if let Some(raw) = headers
            .get(&X_FORWARDED_FOR)
            .and_then(|value| value.to_str().ok())
        {
            let chain: Result<Vec<IpAddr>, _> = raw
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::parse)
                .collect();
            let Ok(chain) = chain else {
                return peer_ip;
            };
            if chain.len() < self.trusted_hops {
                return peer_ip;
            }

            let client_index = chain.len() - self.trusted_hops;
            // The socket peer accounts for the last trusted hop. Every
            // forwarded hop between the selected client and the socket peer
            // must independently belong to the trusted proxy allowlist.
            if chain[client_index + 1..]
                .iter()
                .any(|proxy_ip| !self.is_trusted(*proxy_ip))
            {
                return peer_ip;
            }
            return chain[client_index];
        }

        if self.trusted_hops == 1
            && let Some(real_ip) = headers
                .get(&X_REAL_IP)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<IpAddr>().ok())
        {
            return real_ip;
        }

        peer_ip
    }

    fn sanitize(&self, peer_ip: IpAddr, headers: &mut HeaderMap) -> IpAddr {
        let resolved = self.resolve_client_ip(peer_ip, headers);

        // Raw forwarding authority never continues past this boundary. P4 only
        // consumes client-IP forwarding. Scheme/host authority is intentionally
        // disabled until a future contract names a validated use for it.
        headers.remove(&X_FORWARDED_FOR);
        headers.remove(&X_REAL_IP);
        headers.remove(&X_FORWARDED_PROTO);
        headers.remove(&X_FORWARDED_HOST);

        // Compatibility bridge for current auth rate-limit code: when proxy
        // mode is configured, downstream receives only the resolved value.
        if self.trusted_hops > 0
            && let Ok(value) = HeaderValue::from_str(&resolved.to_string())
        {
            headers.insert(X_REAL_IP, value);
        }

        resolved
    }
}

pub async fn trusted_proxy_headers(
    State(policy): State<Arc<TrustedProxyPolicy>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    mut request: Request,
    next: Next,
) -> Response {
    let resolved = policy.sanitize(peer_addr.ip(), request.headers_mut());
    request.extensions_mut().insert(ResolvedClientIp(resolved));
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::TrustedProxyPolicy;
    use axum::http::{HeaderMap, HeaderValue};
    use ipnet::IpNet;
    use std::net::IpAddr;

    fn ip(value: &str) -> IpAddr {
        value.parse().unwrap()
    }

    fn net(value: &str) -> IpNet {
        value.parse().unwrap()
    }

    #[test]
    fn direct_client_cannot_spoof_forwarded_client_ip() {
        let policy = TrustedProxyPolicy::new(1, vec![net("10.0.0.0/8")]);
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.5"));

        assert_eq!(
            policy.resolve_client_ip(ip("198.51.100.20"), &headers),
            ip("198.51.100.20")
        );
    }

    #[test]
    fn trusted_single_proxy_resolves_last_forwarded_client() {
        let policy = TrustedProxyPolicy::new(1, vec![net("10.0.0.0/8")]);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("192.0.2.44, 203.0.113.9"),
        );

        assert_eq!(
            policy.resolve_client_ip(ip("10.0.0.10"), &headers),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn multi_hop_chain_rejects_untrusted_intermediate_proxy() {
        let policy = TrustedProxyPolicy::new(2, vec![net("10.0.0.0/8")]);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("192.0.2.44, 172.16.0.9"),
        );

        assert_eq!(
            policy.resolve_client_ip(ip("10.0.0.10"), &headers),
            ip("10.0.0.10")
        );
    }

    #[test]
    fn multi_hop_chain_accepts_only_configured_proxy_hops() {
        let policy = TrustedProxyPolicy::new(2, vec![net("10.0.0.0/8"), net("172.16.0.0/12")]);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("192.0.2.44, 172.16.0.9"),
        );

        assert_eq!(
            policy.resolve_client_ip(ip("10.0.0.10"), &headers),
            ip("192.0.2.44")
        );
    }

    #[test]
    fn trusted_proxy_does_not_propagate_raw_scheme_or_host_authority() {
        let policy = TrustedProxyPolicy::new(1, vec![net("10.0.0.0/8")]);
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("192.0.2.44"));
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        headers.insert("x-forwarded-host", HeaderValue::from_static("evil.example"));

        let resolved = policy.sanitize(ip("10.0.0.10"), &mut headers);
        assert_eq!(resolved, ip("192.0.2.44"));
        assert!(headers.get("x-forwarded-for").is_none());
        assert!(headers.get("x-forwarded-proto").is_none());
        assert!(headers.get("x-forwarded-host").is_none());
        assert_eq!(headers.get("x-real-ip").unwrap(), "192.0.2.44");
    }
}
