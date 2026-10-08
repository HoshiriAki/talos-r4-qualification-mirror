//! Integration Fabric control-plane HTTP adapter.
//!
//! Routes never access integration authority tables. They obtain a trusted
//! execution context and dispatch typed DTO payloads through the Registry.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, FromRequest, FromRequestParts, Path, State};
use axum::http::{HeaderMap, StatusCode, request::Parts};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;
use system_core::PlatformCapability;

use crate::error::AppError;
use crate::integration::types::IntegrationError;
use crate::integration::webhook::WebhookReceiptInput;
use crate::middleware::tenant_extractors::{PlatformUser, TrustedTenantAdmin};
use crate::registry::make_platform_ctx;
use crate::state::{AppState, FixtureWebhookRateLimiter};

const MAX_FIXTURE_WEBHOOK_BODY_BYTES: usize = 256 * 1024;

/// Integration routes must preserve the repository's JSON error envelope even
/// when authentication or tenant-context extraction fails before a handler.
struct IntegrationPlatformUser(PlatformUser);

impl FromRequestParts<Arc<AppState>> for IntegrationPlatformUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        PlatformUser::from_request_parts(parts, state)
            .await
            .map(Self)
            .map_err(extractor_rejection)
    }
}

struct IntegrationTenantAdmin(TrustedTenantAdmin);

impl FromRequestParts<Arc<AppState>> for IntegrationTenantAdmin {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        TrustedTenantAdmin::from_request_parts(parts, state)
            .await
            .map(Self)
            .map_err(extractor_rejection)
    }
}

/// JSON parsing is part of the Integration API contract. Keep malformed input
/// in the same stable envelope as Registry command validation failures.
struct IntegrationJson(Value);

impl FromRequest<Arc<AppState>> for IntegrationJson {
    type Rejection = AppError;

    async fn from_request(
        request: axum::extract::Request,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        Json::<Value>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|_rejection: JsonRejection| AppError::CodedBadRequest {
                code: "VAL_INTEGRATION_INPUT".into(),
                message: "Invalid integration JSON payload".into(),
            })
    }
}

fn extractor_rejection(status: StatusCode) -> AppError {
    match status {
        StatusCode::UNAUTHORIZED => AppError::Unauthorized,
        StatusCode::FORBIDDEN => AppError::Forbidden,
        _ => AppError::Internal("integration request context resolution failed".into()),
    }
}

fn webhook_headers(headers: &HeaderMap) -> Result<(String, String), AppError> {
    let signature = headers
        .get("x-integration-fixture-signature")
        .map(|value| {
            value
                .to_str()
                .map(|value| value.to_owned())
                .map_err(|_| AppError::CodedBadRequest {
                    code: "WEBHOOK_REJECTED".into(),
                    message: "Webhook callback rejected".into(),
                })
        })
        .transpose()?;
    let verification_headers: BTreeMap<String, String> = signature
        .map(|signature| BTreeMap::from([("x-integration-fixture-signature".into(), signature)]))
        .unwrap_or_default();
    // Persist only receipt metadata. Authorization, cookies, API keys and
    // signature material never enter normal DB storage or audit payloads.
    let mut stored_headers = BTreeMap::new();
    for name in ["content-type", "x-integration-event-id"] {
        if let Some(value) = headers.get(name) {
            let value = value.to_str().map_err(|_| AppError::CodedBadRequest {
                code: "WEBHOOK_REJECTED".into(),
                message: "Webhook callback rejected".into(),
            })?;
            stored_headers.insert(name.to_owned(), value.to_owned());
        }
    }
    Ok((
        serde_json::to_string(&verification_headers)
            .map_err(|_| AppError::Internal("webhook header serialization failed".into()))?,
        serde_json::to_string(&stored_headers)
            .map_err(|_| AppError::Internal("webhook header serialization failed".into()))?,
    ))
}

fn webhook_rejection(_error: IntegrationError) -> AppError {
    // Do not distinguish unknown endpoints, disabled bindings or failed
    // verification to an unauthenticated caller.
    AppError::CodedBadRequest {
        code: "WEBHOOK_REJECTED".into(),
        message: "Webhook callback rejected".into(),
    }
}

pub fn integration_routes(
    webhook_rate_limiter: Arc<FixtureWebhookRateLimiter>,
) -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/integrations/manifests",
            get(list_manifests).post(register_manifest),
        )
        .route(
            "/api/integrations/instances",
            get(list_instances).post(upsert_instance),
        )
        .route(
            "/api/integrations/bindings",
            get(list_bindings).post(upsert_binding),
        )
        .route(
            "/api/integrations/bindings/{capability}",
            get(inspect_binding),
        )
        .route(
            "/api/integrations/webhooks/{endpoint_token}",
            post(receive_fixture_webhook).route_layer(axum::middleware::from_fn_with_state(
                webhook_rate_limiter,
                fixture_webhook_rate_limit,
            )),
        )
        .route(
            "/api/integrations/webhook-endpoints",
            get(list_webhook_endpoints).post(create_webhook_endpoint),
        )
        .route(
            "/api/integrations/webhook-endpoints/{endpoint_id}/enabled",
            post(set_webhook_endpoint_enabled),
        )
        .route(
            "/api/integrations/webhook-dead-letters",
            get(list_webhook_dead_letters),
        )
        .route(
            "/api/integrations/webhook-dead-letters/{inbox_id}/replay",
            post(replay_webhook),
        )
        .route("/api/integrations/deposits", post(create_deposit))
        .route(
            "/api/integrations/deposits/received",
            post(record_deposit_received),
        )
        .route("/api/integrations/refunds", post(request_refund))
        .route(
            "/api/integrations/refunds/operations",
            post(plan_refund_operation),
        )
        .route(
            "/api/integrations/refunds/reconcile",
            post(reconcile_refund),
        )
        .route("/api/integrations/operations", post(plan_operation))
}

/// Unauthenticated fixture callback ingress. The endpoint token resolves the
/// tenant and binding inside the runtime; this handler neither accepts a
/// tenant ID nor obtains a business repository.
///
/// Admission is rate-limited by `fixture_webhook_rate_limit` before Axum reads
/// the body into `Bytes`.
async fn fixture_webhook_rate_limit(
    State(rate_limiter): State<Arc<FixtureWebhookRateLimiter>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, AppError> {
    let endpoint_token = request
        .uri()
        .path()
        .strip_prefix("/api/integrations/webhooks/")
        .unwrap_or_default();
    if !rate_limiter.allow(peer_addr.ip(), endpoint_token) {
        return Err(AppError::CodedRateLimited {
            code: "WEBHOOK_RATE_LIMITED".into(),
            message: "Webhook callback rate limit exceeded".into(),
        });
    }
    Ok(next.run(request).await)
}

async fn receive_fixture_webhook(
    State(state): State<Arc<AppState>>,
    Path(endpoint_token): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, AppError> {
    if endpoint_token.trim().is_empty()
        || endpoint_token.len() > 512
        || body.is_empty()
        || body.len() > MAX_FIXTURE_WEBHOOK_BODY_BYTES
    {
        return Err(AppError::CodedBadRequest {
            code: "WEBHOOK_REJECTED".into(),
            message: "Webhook callback rejected".into(),
        });
    }
    let provider_event_id = headers
        .get("x-integration-event-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty() && value.len() <= 256)
        .ok_or_else(|| AppError::CodedBadRequest {
            code: "WEBHOOK_REJECTED".into(),
            message: "Webhook callback rejected".into(),
        })?
        .to_owned();
    let (verification_headers_json, stored_headers_json) = webhook_headers(&headers)?;
    let receipt = state
        .integration_webhook_ingress
        .receive(WebhookReceiptInput {
            endpoint_token: endpoint_token.into_bytes(),
            provider_event_id,
            verification_headers_json,
            stored_headers_json,
            raw_payload: body.to_vec(),
        })
        .await
        .map_err(webhook_rejection)?;
    Ok(Json(serde_json::json!({
        "accepted": true,
        "duplicate": receipt.duplicate,
        "mode": "fixture_only",
    })))
}

async fn register_manifest(
    State(state): State<Arc<AppState>>,
    platform_user: IntegrationPlatformUser,
    IntegrationJson(payload): IntegrationJson,
) -> Result<Json<Value>, AppError> {
    let admin = platform_user.0.0;
    if !admin.has_platform_capability(PlatformCapability::PlatformOperationsManage) {
        return Err(AppError::Forbidden);
    }
    let ctx =
        make_platform_ctx(&admin, state.http_client.clone()).map_err(|_| AppError::Forbidden)?;
    execute(&state, "register_manifest", payload, &ctx)
}

async fn list_manifests(
    State(state): State<Arc<AppState>>,
    platform_user: IntegrationPlatformUser,
) -> Result<Json<Value>, AppError> {
    let admin = platform_user.0.0;
    if !admin.has_platform_capability(PlatformCapability::PlatformOperationsManage) {
        return Err(AppError::Forbidden);
    }
    let ctx =
        make_platform_ctx(&admin, state.http_client.clone()).map_err(|_| AppError::Forbidden)?;
    execute(&state, "list_manifests", serde_json::json!({}), &ctx)
}

async fn upsert_instance(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
    IntegrationJson(payload): IntegrationJson,
) -> Result<Json<Value>, AppError> {
    execute(&state, "upsert_instance", payload, tenant_admin.0.context())
}

async fn list_instances(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "list_instances",
        serde_json::json!({}),
        tenant_admin.0.context(),
    )
}

async fn upsert_binding(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
    IntegrationJson(payload): IntegrationJson,
) -> Result<Json<Value>, AppError> {
    execute(&state, "upsert_binding", payload, tenant_admin.0.context())
}

async fn list_bindings(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "list_bindings",
        serde_json::json!({}),
        tenant_admin.0.context(),
    )
}

async fn create_webhook_endpoint(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
    IntegrationJson(payload): IntegrationJson,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "create_webhook_endpoint",
        payload,
        tenant_admin.0.context(),
    )
}

async fn list_webhook_endpoints(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "list_webhook_endpoints",
        serde_json::json!({}),
        tenant_admin.0.context(),
    )
}

async fn set_webhook_endpoint_enabled(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
    Path(endpoint_id): Path<String>,
    IntegrationJson(mut payload): IntegrationJson,
) -> Result<Json<Value>, AppError> {
    payload["endpointId"] = Value::String(endpoint_id);
    execute(
        &state,
        "set_webhook_endpoint_enabled",
        payload,
        tenant_admin.0.context(),
    )
}

async fn list_webhook_dead_letters(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "list_webhook_dead_letters",
        serde_json::json!({}),
        tenant_admin.0.context(),
    )
}

async fn replay_webhook(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
    Path(inbox_id): Path<String>,
    IntegrationJson(mut payload): IntegrationJson,
) -> Result<Json<Value>, AppError> {
    payload["inboxId"] = Value::String(inbox_id);
    execute(&state, "replay_webhook", payload, tenant_admin.0.context())
}

async fn inspect_binding(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
    Path(capability): Path<String>,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "inspect_binding",
        serde_json::json!({ "capability": capability }),
        tenant_admin.0.context(),
    )
}

async fn create_deposit(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
    IntegrationJson(payload): IntegrationJson,
) -> Result<Json<Value>, AppError> {
    execute(&state, "create_deposit", payload, tenant_admin.0.context())
}

async fn request_refund(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
    IntegrationJson(payload): IntegrationJson,
) -> Result<Json<Value>, AppError> {
    execute(&state, "request_refund", payload, tenant_admin.0.context())
}

async fn record_deposit_received(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
    IntegrationJson(payload): IntegrationJson,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "record_deposit_received",
        payload,
        tenant_admin.0.context(),
    )
}

async fn plan_refund_operation(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
    IntegrationJson(payload): IntegrationJson,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "plan_refund_operation",
        payload,
        tenant_admin.0.context(),
    )
}

async fn reconcile_refund(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
    IntegrationJson(payload): IntegrationJson,
) -> Result<Json<Value>, AppError> {
    execute(
        &state,
        "reconcile_refund",
        payload,
        tenant_admin.0.context(),
    )
}

async fn plan_operation(
    State(state): State<Arc<AppState>>,
    tenant_admin: IntegrationTenantAdmin,
    IntegrationJson(payload): IntegrationJson,
) -> Result<Json<Value>, AppError> {
    execute(&state, "plan_operation", payload, tenant_admin.0.context())
}

fn execute(
    state: &AppState,
    command: &str,
    payload: Value,
    context: &system_core::ExecutionContext,
) -> Result<Json<Value>, AppError> {
    state
        .registry
        .execute("integration", command, payload, context)
        .map(Json)
        .map_err(AppError::from_error_payload)
}
