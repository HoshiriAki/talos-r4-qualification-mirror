use crate::repositories::{TenantResolutionRecord, TenantResolutionRepository};
use axum::{
    extract::{Request, State},
    http::header::HOST,
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Extract tenant context from request and inject into extensions.
///
/// Tenant identification strategy:
/// 1. Subdomain: acme.talos.app → tenant_slug = "acme"
/// 2. Custom header: X-Tenant-ID (for API keys)
/// 3. Default: "default" tenant (for backward compatibility)
///
/// If tenant not found, continues without injecting (single-tenant mode).
pub async fn extract_tenant_middleware(
    State(repository): State<TenantResolutionRepository>,
    mut req: Request,
    next: Next,
) -> Response {
    // Extract host from headers
    let host = req
        .headers()
        .get(HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");

    // Extract tenant slug from host
    let tenant_slug = extract_tenant_slug_from_host(host);

    match repository.resolve_by_slug(&tenant_slug) {
        Ok(Some(record)) if record.status == "active" => {
            // Inject the active tenant selected by the trusted persistence authority.
            req.extensions_mut().insert(Tenant::from(record));
        }
        Ok(Some(_)) | Ok(None) if tenant_slug != "default" => {
            // A named subdomain is a tenant selection attempt. Continuing here
            // would allow the request to reach non-tenant extractors without a
            // scope, so reject missing and inactive tenants instead of silently
            // falling back.
            return (axum::http::StatusCode::NOT_FOUND, "Tenant not found").into_response();
        }
        Err(_) if tenant_slug != "default" => {
            // Database errors are not evidence that a named tenant is valid.
            return (axum::http::StatusCode::NOT_FOUND, "Tenant not found").into_response();
        }
        Ok(Some(_)) | Ok(None) | Err(_) => {
            // Keep the explicitly supported localhost/apex default-host path
            // compatible with an unseeded single-tenant development database.
        }
    }

    next.run(req).await
}

/// Extract tenant slug from host header.
///
/// Examples:
/// - "acme.talos.app" → "acme"
/// - "localhost:3000" → "default"
/// - "talos.app" → "default"
fn extract_tenant_slug_from_host(host: &str) -> String {
    let host = normalize_host(host);

    if host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    {
        return "default".to_string();
    }

    // IPv4 loopback and literal addresses are hosts, never tenant subdomains.
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        return "default".to_string();
    }

    // Split by dots
    let parts: Vec<&str> = host.split('.').collect();

    // If host is subdomain.domain.tld, extract subdomain
    if parts.len() >= 3 {
        parts[0].to_string()
    } else {
        // localhost or domain.tld → default tenant
        "default".to_string()
    }
}

/// Tenant context extracted from request.
#[derive(Debug, Clone)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub status: String,
    pub plan: String,
    pub settings: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<TenantResolutionRecord> for Tenant {
    fn from(record: TenantResolutionRecord) -> Self {
        Self {
            id: record.id,
            name: record.name,
            slug: record.slug,
            status: record.status,
            plan: record.plan,
            settings: record.settings,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

impl Tenant {
    /// Check if tenant has a specific feature enabled.
    pub fn has_feature(&self, feature: &str) -> bool {
        if let Some(settings) = &self.settings {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(settings) {
                if let Some(features) = json.get("features") {
                    return features
                        .get(feature)
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tenant_slug_subdomain() {
        assert_eq!(extract_tenant_slug_from_host("acme.talos.app"), "acme");
        assert_eq!(extract_tenant_slug_from_host("demo.talos.app"), "demo");
        assert_eq!(
            extract_tenant_slug_from_host("tenant1.staging.talos.app"),
            "tenant1"
        );
    }

    #[test]
    fn test_extract_tenant_slug_localhost() {
        assert_eq!(extract_tenant_slug_from_host("localhost"), "default");
        assert_eq!(extract_tenant_slug_from_host("localhost:3000"), "default");
        assert_eq!(extract_tenant_slug_from_host("127.0.0.1"), "default");
        assert_eq!(extract_tenant_slug_from_host("127.0.0.1:3000"), "default");
        assert_eq!(extract_tenant_slug_from_host("[::1]:3000"), "default");
    }

    #[test]
    fn test_extract_tenant_slug_apex_domain() {
        assert_eq!(extract_tenant_slug_from_host("talos.app"), "default");
        assert_eq!(extract_tenant_slug_from_host("example.com"), "default");
    }

    #[test]
    fn test_unknown_subdomain_does_not_fall_back_to_default() {
        assert_eq!(
            extract_tenant_slug_from_host("unknown.talos.app"),
            "unknown"
        );
        assert_ne!(
            extract_tenant_slug_from_host("unknown.talos.app"),
            "default"
        );
    }
}

/// Normalize a Host header while preserving IPv4 and IPv6 loopback hosts.
fn normalize_host(host: &str) -> &str {
    if let Some(bracketed) = host.strip_prefix('[') {
        return bracketed
            .split_once(']')
            .map(|(address, _)| address)
            .unwrap_or(host);
    }

    if host.parse::<std::net::IpAddr>().is_ok() {
        return host;
    }

    match host.rsplit_once(':') {
        Some((name, port)) if port.parse::<u16>().is_ok() => name,
        _ => host,
    }
}
