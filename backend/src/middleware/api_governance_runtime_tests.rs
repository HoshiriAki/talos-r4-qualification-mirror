use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use axum::middleware;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use tower::ServiceExt;
use tower_http::cors::CorsLayer;

use super::api_governance::{ApiGovernancePolicy, api_governance};
use crate::config::AppConfig;

fn test_config(public_https: bool) -> AppConfig {
    AppConfig {
        host: "127.0.0.1".into(),
        port: 3000,
        db_path: ":memory:".into(),
        is_production: public_https,
        public_https,
        auth_cookie_name: "talos_session".into(),
        session_ttl_days: 7,
        auth_cookie_secure: public_https,
        auth_bootstrap_on_start: false,
        auth_bootstrap_admin_username: String::new(),
        auth_bootstrap_admin_password: String::new(),
        auth_login_rate_window_ms: 60_000,
        auth_login_rate_max_attempts: 5,
        auth_login_rate_block_ms: 300_000,
        cors_allowed_origin: Some(if public_https {
            "https://talos.example.test".into()
        } else {
            "http://127.0.0.1:3000".into()
        }),
        public_dir: "../public".into(),
        vite_dev_url: None,
    }
}

fn governed_router(public_https: bool) -> Router {
    let config = test_config(public_https);
    let policy = Arc::new(ApiGovernancePolicy::from_config(&config));
    let cors = CorsLayer::new()
        .allow_origin(
            config
                .cors_allowed_origin
                .as_ref()
                .unwrap()
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        )
        .allow_credentials(true)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE]);

    Router::new()
        .route(
            "/api/test",
            post(|Json(payload): Json<Value>| async move {
                Json(json!({"ok": true, "payload": payload}))
            }),
        )
        .route("/app/test", get(|| async { "ui" }))
        .route("/health", get(|| async { Json(json!({"ok": true})) }))
        // Match production composition: CORS is inside governance so a
        // preflight response still receives request identity/security headers.
        .layer(cors)
        .layer(middleware::from_fn_with_state(policy, api_governance))
}

#[tokio::test]
async fn oversized_json_is_stable_and_client_request_id_is_replaced() {
    let app = governed_router(false);
    let oversized = vec![b'x'; 1024 * 1024 + 1];
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/test")
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-request-id", "caller-controlled")
                .body(Body::from(oversized))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap();
    assert_ne!(request_id, "caller-controlled");
    assert!(uuid::Uuid::parse_str(request_id).is_ok());
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    assert!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );

    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["code"], "HTTP_PAYLOAD_TOO_LARGE");
}

#[tokio::test]
async fn framework_unsupported_media_type_uses_stable_json_envelope() {
    let app = governed_router(false);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/test")
                .header(header::CONTENT_TYPE, "text/plain")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["code"], "HTTP_UNSUPPORTED_MEDIA_TYPE");
}

#[tokio::test]
async fn public_diagnostic_is_no_store_and_normalizes_method_rejection() {
    let response = governed_router(false)
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap();
    assert!(uuid::Uuid::parse_str(request_id).is_ok());
    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["code"], "HTTP_METHOD_NOT_ALLOWED");
}

#[tokio::test]
async fn cors_preflight_is_inside_governance_and_receives_request_id() {
    let response = governed_router(false)
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/api/test")
                .header(header::ORIGIN, "http://127.0.0.1:3000")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap();
    assert!(uuid::Uuid::parse_str(request_id).is_ok());
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "http://127.0.0.1:3000"
    );
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
}

#[tokio::test]
async fn web_ui_receives_same_origin_csp_but_plain_http_has_no_hsts() {
    let response = governed_router(false)
        .oneshot(
            Request::builder()
                .uri("/app/test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let csp = response
        .headers()
        .get("content-security-policy")
        .and_then(|value| value.to_str().ok())
        .unwrap();
    assert!(csp.contains("frame-src 'self'"));
    assert!(csp.contains("frame-ancestors 'self'"));
    assert_eq!(
        response.headers().get("x-frame-options").unwrap(),
        "SAMEORIGIN"
    );
    assert!(
        response
            .headers()
            .get("strict-transport-security")
            .is_none()
    );
}

#[tokio::test]
async fn public_https_profile_emits_hsts() {
    let response = governed_router(true)
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("strict-transport-security").unwrap(),
        "max-age=31536000; includeSubDomains"
    );
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
}
