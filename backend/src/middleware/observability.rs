//! Observability middleware: request tracing + Prometheus metrics
use axum::extract::{Request, State};
use axum::http::Uri;
use axum::middleware::Next;
use axum::response::Response;
use std::time::Instant;

use crate::middleware::api_governance::{IngressSurface, RequestId};
use crate::observability::{MetricsSink, RuntimeMetrics};

pub async fn observability_middleware(
    State(metrics): State<std::sync::Arc<RuntimeMetrics>>,
    req: Request,
    next: Next,
) -> Response {
    let method = req.method().clone();
    let path = log_path(req.uri()).to_owned();
    let request_id = req
        .extensions()
        .get::<RequestId>()
        .map(|id| id.as_str().to_owned())
        .unwrap_or_else(|| "unassigned".to_string());
    let ingress_surface = req
        .extensions()
        .get::<IngressSurface>()
        .copied()
        .unwrap_or_else(|| IngressSurface::classify(req.uri().path()));
    let start = Instant::now();

    let response = next.run(req).await;

    let duration = start.elapsed();
    let status = response.status().as_u16();
    metrics.http_request(method.as_str(), &path, status, duration);

    tracing::info!(
        target: "http",
        request_id = %request_id,
        ingress_surface = ingress_surface.as_str(),
        method = %method,
        path = %path,
        status = status,
        duration_ms = duration.as_millis(),
        "{} {} → {} ({:.0}ms)",
        method, path, status, duration.as_secs_f64() * 1000.0
    );

    response
}

/// Never log query parameters. The fixture callback uses an unguessable bearer
/// token in the path, so redact its entire dynamic segment before the request
/// crosses the normal observability boundary.
fn log_path(uri: &Uri) -> &str {
    if uri.path().starts_with("/api/integrations/webhooks/") {
        "/api/integrations/webhooks/[REDACTED]"
    } else if uri
        .path()
        .starts_with("/api/v3/rental-settlement/commands/")
    {
        "/api/v3/rental-settlement/commands/[COMMAND]"
    } else {
        uri.path()
    }
}

#[cfg(test)]
mod tests {
    use axum::http::Uri;

    use super::log_path;

    #[test]
    fn redacts_fixture_webhook_token_and_all_query_parameters() {
        let callback: Uri = "/api/integrations/webhooks/capability-token?debug=secret"
            .parse()
            .unwrap();
        let settlement: Uri =
            "/api/v3/rental-settlement/commands/close-order-raw?order_id=order-raw"
                .parse()
                .unwrap();
        let ordinary: Uri = "/api/orders?cursor=private".parse().unwrap();

        assert_eq!(log_path(&callback), "/api/integrations/webhooks/[REDACTED]");
        assert_eq!(
            log_path(&settlement),
            "/api/v3/rental-settlement/commands/[COMMAND]"
        );
        assert_eq!(log_path(&ordinary), "/api/orders");
    }
}
