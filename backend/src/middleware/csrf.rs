use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ErrorBody;
use crate::middleware::api_governance::IngressSurface;
use crate::state::AppState;

fn csrf_rejection() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorBody {
            ok: false,
            code: Some("AUTH_CSRF_REJECTED".into()),
            error: "Cross-site request rejected".into(),
        }),
    )
        .into_response()
}

/// Compare an Origin/Referer URL against the request Host authority.
///
/// `PUBLIC_HTTPS=true` is the production transport declaration, so a state-
/// changing browser request must then originate from HTTPS as well as the same
/// host/port authority. When public HTTPS is disabled (for example loopback
/// development), the historical HTTP/HTTPS scheme flexibility is retained.
fn csrf_source_matches(raw: &str, authority: &str, require_https: bool) -> bool {
    let Ok(parsed) = url::Url::parse(raw) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    if require_https && parsed.scheme() != "https" {
        return false;
    }

    let Some(origin_host) = parsed.host_str() else {
        return false;
    };
    let origin_port = parsed.port_or_known_default();

    // Parse Host as an authority by attaching the already-validated URL scheme.
    // `url::Url` then normalizes explicit default ports (for example :443).
    let Ok(expected) = url::Url::parse(&format!("{}://{authority}", parsed.scheme())) else {
        return false;
    };
    let Some(expected_host) = expected.host_str() else {
        return false;
    };

    origin_host.eq_ignore_ascii_case(expected_host)
        && origin_port == expected.port_or_known_default()
}

fn csrf_headers_match(
    headers: &axum::http::HeaderMap,
    authority: &str,
    require_https: bool,
) -> bool {
    // Origin is the authoritative browser source signal when present. A
    // contradictory Origin must fail closed rather than being overridden by a
    // weaker Referer value.
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        return csrf_source_matches(origin, authority, require_https);
    }

    headers
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|referer| csrf_source_matches(referer, authority, require_https))
}

pub async fn csrf_check(
    State(state): State<Arc<AppState>>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, Response> {
    // CSRF exemptions are defined by the single R4-P4 ingress-surface contract.
    // Machine execution is bearer-only and rejects cookies; provider callbacks
    // authenticate at their own token/signature boundary. Provisioning routes
    // such as `/api/machine-clients` remain BrowserSession and are protected.
    if IngressSurface::classify(req.uri().path()).csrf_exempt() {
        return Ok(next.run(req).await);
    }
    if matches!(
        req.method(),
        &Method::GET | &Method::HEAD | &Method::OPTIONS
    ) {
        return Ok(next.run(req).await);
    }

    let headers = req.headers();
    let authority = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let require_https = state.config.public_https;

    if csrf_headers_match(headers, authority, require_https) {
        return Ok(next.run(req).await);
    }

    Err(csrf_rejection())
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::{csrf_headers_match, csrf_source_matches};
    use crate::middleware::api_governance::IngressSurface;

    #[test]
    fn machine_client_provisioning_is_not_csrf_exempt() {
        assert!(!IngressSurface::classify("/api/machine-clients").csrf_exempt());
        assert!(IngressSurface::classify("/api/machine/v1/tenants/t1/execute").csrf_exempt());
    }

    #[test]
    fn csrf_authority_preserves_explicit_port_and_normalizes_default_port() {
        assert!(csrf_source_matches(
            "https://example.test:8443/path",
            "example.test:8443",
            true
        ));
        assert!(!csrf_source_matches(
            "https://example.test:9443/path",
            "example.test:8443",
            true
        ));
        assert!(csrf_source_matches(
            "https://example.test:443/path",
            "example.test",
            true
        ));
    }

    #[test]
    fn public_https_rejects_same_authority_http_origin() {
        assert!(!csrf_source_matches(
            "http://example.test/path",
            "example.test",
            true
        ));
        assert!(csrf_source_matches(
            "https://example.test/path",
            "example.test",
            true
        ));
    }

    #[test]
    fn conflicting_origin_cannot_be_overridden_by_referer() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://attacker.example"),
        );
        headers.insert(
            header::REFERER,
            HeaderValue::from_static("https://example.test/app"),
        );
        assert!(!csrf_headers_match(&headers, "example.test", true));

        headers.remove(header::ORIGIN);
        assert!(csrf_headers_match(&headers, "example.test", true));
    }
}
