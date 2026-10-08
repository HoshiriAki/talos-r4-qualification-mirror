//! Compliance REST endpoints
//! POST /api/compliance/consent/record|check|revoke|audit
//! POST /api/compliance/deletion/request|list|process|complete|reject
//! POST /api/compliance/2fa/generate|verify_enable|disable|status
//!
//! Permissions: consent record/check/revoke=AuthUser, audit=AdminUser
//!              deletion request=AuthUser, list/process/complete/reject=AdminUser
//!              2fa generate/verify_enable/status=AuthUser, disable=AdminUser

use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::routing::post;
use axum::{Extension, Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::{
    auth::{AdminUser, AuthUser},
    trusted_proxy::ResolvedClientIp,
};
use crate::registry::make_ctx;
use crate::services::auth_rate_limit::AuthRateLimiter;
use crate::state::AppState;

pub fn compliance_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/compliance/consent/record", post(consent_record))
        .route("/api/compliance/consent/check", post(consent_check))
        .route("/api/compliance/consent/revoke", post(consent_revoke))
        .route("/api/compliance/consent/audit", post(consent_audit))
        .route("/api/compliance/deletion/request", post(deletion_request))
        .route("/api/compliance/deletion/list", post(deletion_list))
        .route("/api/compliance/deletion/process", post(deletion_process))
        .route("/api/compliance/deletion/complete", post(deletion_complete))
        .route("/api/compliance/deletion/reject", post(deletion_reject))
        .route("/api/compliance/2fa/generate", post(two_fa_generate))
        .route(
            "/api/compliance/2fa/verify_enable",
            post(two_fa_verify_enable),
        )
        .route("/api/compliance/2fa/disable", post(two_fa_disable))
        .route("/api/compliance/2fa/status", post(two_fa_status))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentRecordBody {
    consent_type: String,
    version: String,
}

async fn consent_record(
    State(state): State<Arc<AppState>>,
    client_ip: Option<Extension<ResolvedClientIp>>,
    headers: HeaderMap,
    auth: AuthUser,
    Json(b): Json<ConsentRecordBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip_address = client_ip
        .as_ref()
        .map(|resolved| resolved.0.0.to_string())
        .unwrap_or_else(|| "unresolved".to_string());
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "consent",
            "record",
            serde_json::json!({
                "userId": auth.0.id,
                "consentType": b.consent_type,
                "version": b.version,
                "ipAddress": ip_address,
                "userAgent": user_agent,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentCheckBody {
    consent_type: String,
    version: String,
}

async fn consent_check(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<ConsentCheckBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "consent",
            "check",
            serde_json::json!({
                "userId": auth.0.id,
                "consentType": b.consent_type,
                "version": b.version,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentRevokeBody {
    consent_type: String,
}

async fn consent_revoke(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<ConsentRevokeBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "consent",
            "revoke",
            serde_json::json!({
                "userId": auth.0.id,
                "consentType": b.consent_type,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentAuditBody {
    user_id: String,
}

async fn consent_audit(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<ConsentAuditBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "consent",
            "audit",
            serde_json::json!({
                "userId": b.user_id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeletionRequestBody {
    request_type: String,
    reason: String,
}

async fn deletion_request(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(b): Json<DeletionRequestBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "deletion",
            "request",
            serde_json::json!({
                "userId": auth.0.id,
                "requestType": b.request_type,
                "reason": b.reason,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeletionListBody {
    status: Option<String>,
}

async fn deletion_list(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<DeletionListBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "deletion",
            "list",
            serde_json::json!({
                "status": b.status,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeletionAdminBody {
    request_id: String,
    admin_notes: String,
}

async fn deletion_process(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<DeletionAdminBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "deletion",
            "process",
            serde_json::json!({
                "requestId": b.request_id,
                "adminNotes": b.admin_notes,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

async fn deletion_complete(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<DeletionAdminBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "deletion",
            "complete",
            serde_json::json!({
                "requestId": b.request_id,
                "adminNotes": b.admin_notes,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

async fn deletion_reject(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<DeletionAdminBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "deletion",
            "reject",
            serde_json::json!({
                "requestId": b.request_id,
                "adminNotes": b.admin_notes,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

async fn two_fa_generate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "two_fa",
            "generate_secret",
            serde_json::json!({
                "userId": auth.0.id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TwoFaVerifyBody {
    code: String,
}

async fn two_fa_verify_enable(
    State(state): State<Arc<AppState>>,
    Extension(auth_rate_limiter): Extension<Arc<AuthRateLimiter>>,
    auth: AuthUser,
    Json(b): Json<TwoFaVerifyBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rate_key = format!("mfa-enrollment:{}", auth.0.id);
    if !auth_rate_limiter
        .check_custom(&rate_key, &state.config)?
        .allowed
    {
        return Err(AppError::RateLimited {
            retry_after_secs: 30,
        });
    }
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let result = state.registry.execute(
        "two_fa",
        "verify_and_enable",
        serde_json::json!({
            "userId": auth.0.id,
            "code": b.code,
        }),
        &ctx,
    );
    match result {
        Ok(value) => {
            auth_rate_limiter.record_custom_success(&rate_key)?;
            Ok(Json(value))
        }
        Err(error) => {
            auth_rate_limiter.record_custom_failure(&rate_key, &state.config)?;
            Err(AppError::from_error_payload(error))
        }
    }
}

async fn two_fa_status(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "two_fa",
            "status",
            serde_json::json!({
                "userId": auth.0.id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TwoFaDisableBody {
    user_id: String,
}

async fn two_fa_disable(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(b): Json<TwoFaDisableBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let r = state
        .registry
        .execute(
            "two_fa",
            "disable",
            serde_json::json!({
                "userId": b.user_id,
            }),
            &ctx,
        )
        .map(Json)
        .map_err(AppError::from_error_payload)?;
    Ok(r)
}
