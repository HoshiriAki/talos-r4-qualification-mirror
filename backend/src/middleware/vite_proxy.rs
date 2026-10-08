use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use std::sync::Arc;

use crate::state::AppState;

fn preserves_webhook_bearer_path(path: &str) -> bool {
    path.starts_with("/api/integrations/webhooks/")
}

/// Middleware: in dev mode, the backend must only be reachable through the Vite
/// proxy. Direct browser access → 301 redirect to the Vite dev server.
pub async fn vite_proxy_guard(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: axum::middleware::Next,
) -> Response {
    let Some(vite_url) = &state.config.vite_dev_url else {
        return next.run(req).await;
    };

    // Allow requests proxied by Vite
    if req
        .headers()
        .get("x-forwarded-by")
        .and_then(|v| v.to_str().ok())
        == Some("vite")
    {
        return next.run(req).await;
    }

    // A webhook endpoint token is a bearer secret in the path. Never copy it
    // into a redirect Location header, even during local Vite development.
    if preserves_webhook_bearer_path(req.uri().path()) {
        return next.run(req).await;
    }

    // Direct access — redirect to Vite, preserving path
    let path = req.uri().path_and_query().map_or("/", |pq| pq.as_str());
    let target = format!("{}{}", vite_url.trim_end_matches('/'), path);
    (StatusCode::MOVED_PERMANENTLY, Redirect::to(&target)).into_response()
}

#[cfg(test)]
mod tests {
    use super::preserves_webhook_bearer_path;

    #[test]
    fn bearer_callback_paths_are_never_redirected() {
        assert!(preserves_webhook_bearer_path(
            "/api/integrations/webhooks/bearer-token"
        ));
        assert!(!preserves_webhook_bearer_path(
            "/api/integrations/webhook-endpoints"
        ));
    }
}
