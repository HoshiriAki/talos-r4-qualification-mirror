use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use std::sync::Arc;

use crate::state::AppState;

pub fn health_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
}

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn readiness_check(State(state): State<Arc<AppState>>) -> Response {
    #[cfg(feature = "postgres")]
    if let Some(pool) = state.pg_pool.as_ref() {
        let database_ready = sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(pool)
            .await
            .is_ok_and(|value| value == 1);
        if database_ready {
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "ready",
                    "database": "postgres"
                })),
            )
                .into_response();
        }
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "not_ready",
                "database": "postgres"
            })),
        )
            .into_response();
    }

    if state.config.is_production {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "not_ready",
                "database": "unavailable"
            })),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ready",
            "database": "local"
        })),
    )
        .into_response()
}
