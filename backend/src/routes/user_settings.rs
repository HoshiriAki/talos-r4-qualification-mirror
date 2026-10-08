use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Serialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::AuthUser;
use crate::registry::make_ctx;
use crate::state::AppState;
use crate::utils::http;

pub fn user_settings_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/user-settings", get(get_settings_handler))
        .route("/api/user-settings", put(save_settings_handler))
}

#[derive(Debug, Serialize)]
struct SettingsResponse {
    ok: bool,
    settings: serde_json::Value,
}

async fn get_settings_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Response, AppError> {
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    let settings = state
        .registry
        .execute(
            "user_settings",
            "get_settings",
            serde_json::json!({ "userId": auth.0.id }),
            &ctx,
        )
        .map_err(AppError::from_error_payload)?;
    let body = SettingsResponse { ok: true, settings };
    Ok(http::json_response(StatusCode::OK, &body))
}

async fn save_settings_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, AppError> {
    if !body.is_object() {
        return Err(AppError::BadRequest("请求体必须是 JSON 对象".to_string()));
    }
    let theme = body
        .get("theme")
        .and_then(|v| v.as_str())
        .filter(|t| matches!(*t, "dark" | "light" | "system"));

    let mut settings_obj = serde_json::json!({
        "sidebarOrder": body.get("sidebarOrder").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).map(String::from).collect::<Vec<_>>()).unwrap_or_default(),
        "hiddenPaths": body.get("hiddenPaths").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).map(String::from).collect::<Vec<_>>()).unwrap_or_default(),
        "homePage": body.get("homePage").and_then(|v| v.as_str()).unwrap_or("/").to_string(),
    });
    if let Some(t) = theme {
        settings_obj["theme"] = serde_json::Value::String(t.to_string());
    }
    let ctx = make_ctx(&auth.0, state.http_client.clone());
    state
        .registry
        .execute(
            "user_settings",
            "save_settings",
            serde_json::json!({
                "userId": auth.0.id,
                "settings": settings_obj.clone(),
            }),
            &ctx,
        )
        .map_err(AppError::from_error_payload)?;
    let response = SettingsResponse {
        ok: true,
        settings: settings_obj,
    };
    Ok(http::json_response(StatusCode::OK, &response))
}
