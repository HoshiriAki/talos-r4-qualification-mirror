use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use std::sync::Arc;

use crate::application::ModelAuthorityError;
use crate::error::AppError;
use crate::middleware::auth::{AdminUser, AuthUser};
use crate::registry::make_ctx;
use crate::state::AppState;

pub fn device_model_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/device-models", get(list_models).post(create_model))
        .route(
            "/api/device-models/{id}",
            get(get_model)
                .patch(update_model)
                .put(update_model_full)
                .delete(delete_model_route),
        )
        .route("/api/device-models/{id}/pricing", put(update_pricing))
}

// ── Handlers ────────────────────────────────────────────────────

async fn list_models(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    state
        .registry
        .execute("model", "list_models", serde_json::json!({}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn get_model(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_auth.0, state.http_client.clone());
    state
        .registry
        .execute("model", "get_model", serde_json::json!({"id": id}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn create_model(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    state
        .registry
        .execute("model", "create_model", body, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn update_model(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    let mut payload = body;
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("id".to_string(), serde_json::Value::String(id));
    }
    state
        .registry
        .execute("model", "update_model", payload, &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)
}

async fn update_model_full(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&admin.0, state.http_client.clone());
    let model = state
        .application_services()
        .model_authority()
        .update_full(&ctx, &id, &body, &admin.0.username)
        .map_err(model_authority_error)?;
    Ok(Json(serde_json::json!({ "ok": true, "model": model })))
}

async fn update_pricing(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&admin.0, state.http_client.clone());
    let model = state
        .application_services()
        .model_authority()
        .update_pricing(&ctx, &id, &body, &admin.0.username)
        .map_err(model_authority_error)?;
    Ok(Json(serde_json::json!({ "ok": true, "model": model })))
}

async fn delete_model_route(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&_admin.0, state.http_client.clone());
    state
        .registry
        .execute("model", "delete_model", serde_json::json!({"id": id}), &ctx)
        .map(Json)
        .map_err(AppError::from_error_payload)
}

fn model_authority_error(error: ModelAuthorityError) -> AppError {
    match error {
        ModelAuthorityError::InvalidInput(message) => AppError::BadRequest(message),
        ModelAuthorityError::NotFound => AppError::NotFound("型号不存在".into()),
        ModelAuthorityError::DuplicateName => AppError::Conflict("型号名称已存在".into()),
        ModelAuthorityError::Referenced(count) => {
            AppError::Conflict(format!("该型号已被 {count} 台设备使用，无法删除"))
        }
        ModelAuthorityError::Persistence { code } => AppError::ServiceError {
            code: code.into(),
            message: "model persistence unavailable".into(),
        },
    }
}
