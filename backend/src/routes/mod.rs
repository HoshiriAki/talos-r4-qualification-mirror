pub mod api_keys;
pub mod assets;
pub mod audit;
pub mod auth;
pub mod backup;
pub mod barcode;
pub mod booking;
pub mod compliance;
pub mod contract;
pub mod credit;
pub mod customers;
pub mod damage;
pub mod dashboard;
pub mod device_models;
pub mod devices;
pub mod finance;
pub mod finance_tax;
pub mod health;
pub mod integrations;
pub mod machine_api;
pub mod meta;
pub mod metrics;
pub mod notify;
pub mod optical_sop;
pub mod order_lifecycle_v2;
pub mod orders;
pub mod orders_v2;
pub mod overdue;
pub mod platform_memberships;
pub mod pricing;
pub mod quotes;
pub mod r3_settlement;
pub mod reports;
pub mod reservations_v2;
pub mod security;
pub mod tasks;
pub mod tenant_governance;
pub mod tenant_memberships;
pub mod tenant_preview;
pub mod tenant_simulation;
pub mod tenants;
pub mod user_settings;
pub mod warehouses;

use axum::Extension;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderName, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::error::ErrorBody;
use crate::middleware::api_governance;
use crate::middleware::csrf;
use crate::middleware::vite_proxy::vite_proxy_guard;
use crate::observability::RuntimeMetrics;
use crate::services::auth_rate_limit::AuthRateLimiter;
use crate::state::AppState;

/// The process-level metrics sink is composed explicitly at the HTTP boundary
/// rather than becoming application state. Handlers, middleware, Registry,
/// workers, and R3 repositories receive clones of this one bounded sink from
/// `main` without creating an ambient metrics dependency.
pub fn create_router(state: Arc<AppState>, metrics: Arc<RuntimeMetrics>) -> Router {
    // P7 authentication throttling is an HTTP ingress authority. Compose it as
    // an explicit Extension rather than expanding the frozen SP-06 AppState.
    let auth_rate_limiter = Arc::new(AuthRateLimiter::from_repository(
        state.auth_security_repository().clone(),
    ));

    // P9-SP05: Prometheus receives a metrics-only scrape authority. The token
    // is never accepted by ordinary browser/machine routes, and the scrape
    // extractor also binds it to the configured direct backend peer.
    let metrics_scrape_authority = Arc::new(metrics::MetricsScrapeAuthority::from_env());

    // `main` validates and normalizes this origin before any process side
    // effects, then stores only the origin tuple in AppConfig. Keep CORS inside
    // the common P4 governance wrapper so preflight responses also receive
    // server request identity and security headers without changing the R3
    // create_router composition contract.
    let cors_origin = state
        .config
        .cors_allowed_origin
        .as_deref()
        .expect("normalized CORS origin must be configured before router assembly")
        .parse::<axum::http::HeaderValue>()
        .expect("normalized CORS origin must remain a valid header value");
    let cors = CorsLayer::new()
        .allow_origin(cors_origin)
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::HEAD,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::ACCEPT,
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static("idempotency-key"),
            HeaderName::from_static("x-talos-authority"),
        ])
        .expose_headers([
            HeaderName::from_static("x-request-id"),
            HeaderName::from_static("x-correlation-id"),
            header::RETRY_AFTER,
        ]);

    let app = Router::new()
        .merge(health::health_routes())
        .merge(integrations::integration_routes(
            state.integration_webhook_rate_limiter.clone(),
        ))
        .merge(metrics::metrics_routes())
        .merge(machine_api::machine_api_routes())
        .merge(auth::auth_routes())
        .merge(devices::device_routes())
        .merge(orders::order_routes())
        .merge(orders_v2::order_v2_routes())
        .merge(order_lifecycle_v2::order_lifecycle_v2_routes())
        .merge(r3_settlement::r3_settlement_routes())
        .merge(customers::customer_routes())
        .merge(audit::audit_routes())
        .merge(meta::meta_routes())
        .merge(tenant_memberships::tenant_membership_routes())
        .merge(platform_memberships::platform_membership_routes())
        .merge(pricing::pricing_routes())
        .merge(quotes::quote_routes())
        .merge(reservations_v2::reservation_v2_routes())
        .merge(device_models::device_model_routes())
        .merge(warehouses::warehouse_routes())
        .merge(tenants::tenant_routes())
        .merge(tenant_governance::tenant_governance_routes())
        .merge(tenant_preview::tenant_preview_routes())
        .merge(tenant_simulation::tenant_simulation_routes())
        .merge(reports::report_routes())
        .merge(dashboard::dashboard_routes())
        .merge(user_settings::user_settings_routes())
        .merge(api_keys::api_key_routes())
        .merge(damage::damage_routes())
        .merge(finance::finance_routes())
        .merge(finance_tax::finance_tax_routes())
        .merge(notify::notify_routes())
        .merge(assets::assets_routes())
        .merge(compliance::compliance_routes())
        .merge(credit::credit_routes())
        .merge(overdue::overdue_routes())
        .merge(contract::contract_routes())
        .merge(optical_sop::optical_sop_routes())
        .merge(backup::backup_routes())
        .merge(security::security_routes())
        .merge(barcode::barcode_routes())
        .merge(tasks::tasks_routes())
        .layer(Extension(metrics.clone()))
        .layer(Extension(metrics_scrape_authority))
        .layer(Extension(auth_rate_limiter))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            csrf::csrf_check,
        ));

    // Tenant extraction consumes the backend-selected tenant resolution
    // authority rather than choosing a persistence backend at the HTTP layer.
    let app = app.layer(axum::middleware::from_fn_with_state(
        state.tenant_resolution_repository().clone(),
        crate::middleware::tenant::extract_tenant_middleware,
    ));

    let app = app.layer(axum::middleware::from_fn_with_state(
        metrics,
        crate::middleware::observability::observability_middleware,
    ));

    let trusted_proxy_policy =
        Arc::new(crate::middleware::trusted_proxy::TrustedProxyPolicy::from_env());
    let api_governance_policy = Arc::new(api_governance::ApiGovernancePolicy::from_config(
        &state.config,
    ));

    Router::new()
        .merge(app)
        .fallback(not_found_handler)
        .with_state(state.clone())
        // CORS sits inside the common governance boundary so preflight and
        // CORS-generated responses still receive server request identity and
        // security headers. It remains outside CSRF/business handlers.
        .layer(cors)
        // R4-P4 ingress governance is a root HTTP boundary so matched routes,
        // preflight and API fallbacks receive the same request identity,
        // resource budget, stable transport errors and browser security headers.
        .layer(axum::middleware::from_fn_with_state(
            api_governance_policy,
            api_governance::api_governance,
        ))
        // Forwarded headers are sanitized only after the socket peer is proven
        // to belong to TRUSTED_PROXY_CIDRS. Direct clients cannot spoof IP,
        // scheme or host authority through forwarding headers.
        .layer(axum::middleware::from_fn_with_state(
            trusted_proxy_policy,
            crate::middleware::trusted_proxy::trusted_proxy_headers,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state,
            vite_proxy_guard,
        ))
}

async fn not_found_handler(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response {
    let path = req.uri().path().to_string();
    let is_api = api_governance::is_api_path(&path);

    if is_api {
        let body = ErrorBody {
            ok: false,
            code: Some("HTTP_ROUTE_NOT_FOUND".to_string()),
            error: "接口不存在".to_string(),
        };
        return (StatusCode::NOT_FOUND, Json(body)).into_response();
    }

    if let Some(vite_url) = &state.config.vite_dev_url {
        return axum::response::Redirect::temporary(vite_url).into_response();
    }

    let index_path = std::path::Path::new(&state.config.public_dir).join("index.html");
    if let Ok(html) = tokio::fs::read_to_string(&index_path).await {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        )
            .into_response();
    }

    let body = ErrorBody {
        ok: false,
        code: Some("WEB_ROUTE_NOT_FOUND".to_string()),
        error: "页面不存在".to_string(),
    };
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}
