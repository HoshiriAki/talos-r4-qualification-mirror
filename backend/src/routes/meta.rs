use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use std::sync::Arc;

use crate::middleware::auth::AuthUser;
use crate::state::AppState;
use crate::utils::constants;

pub fn meta_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/meta/options", get(meta_options))
        .route("/meta/modules", get(list_modules))
        .route("/meta/modules/openai-tools", get(list_openai_tools))
}

async fn meta_options() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "pickupMethods": constants::PICKUP_METHODS,
        "deviceStatus": constants::DEVICE_STATUS,
        "orderStatus": constants::ORDER_STATUS,
    }))
}

async fn list_modules(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Json<Vec<serde_json::Value>> {
    let schemas: Vec<serde_json::Value> = state
        .registry
        .all_schemas()
        .into_iter()
        .map(|s| serde_json::to_value(s).unwrap_or_default())
        .collect();
    Json(schemas)
}

async fn list_openai_tools(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Json<Vec<serde_json::Value>> {
    Json(state.registry.openai_tools())
}
