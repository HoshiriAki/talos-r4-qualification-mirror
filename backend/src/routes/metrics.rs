use axum::Extension;
use axum::Router;
use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::{StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use system_core::PlatformCapability;

use crate::error::AppError;
use crate::middleware::tenant_extractors::PlatformUser;
use crate::observability::RuntimeMetrics;

#[derive(Clone, Default)]
pub struct MetricsScrapeAuthority {
    peer: Option<IpAddr>,
    token: Option<Vec<u8>>,
}

impl MetricsScrapeAuthority {
    pub fn from_env() -> Self {
        let token_path = std::env::var("METRICS_SCRAPE_TOKEN_FILE").ok();
        let peer = std::env::var("METRICS_SCRAPE_PEER").ok();

        match (token_path, peer) {
            (None, None) => Self::default(),
            (Some(token_path), Some(peer)) => {
                let Ok(peer) = peer.parse::<IpAddr>() else {
                    tracing::warn!("metrics scrape authority disabled: invalid peer address");
                    return Self::default();
                };
                let Ok(raw) = std::fs::read_to_string(&token_path) else {
                    tracing::warn!("metrics scrape authority disabled: token file unavailable");
                    return Self::default();
                };
                let token = raw.trim();
                if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    tracing::warn!(
                        "metrics scrape authority disabled: token must be 64 hexadecimal characters"
                    );
                    return Self::default();
                }
                Self {
                    peer: Some(peer),
                    token: Some(token.as_bytes().to_vec()),
                }
            }
            _ => {
                tracing::warn!(
                    "metrics scrape authority disabled: token file and peer must be configured together"
                );
                Self::default()
            }
        }
    }

    #[cfg(test)]
    fn for_test(peer: IpAddr, token: &str) -> Self {
        Self {
            peer: Some(peer),
            token: Some(token.as_bytes().to_vec()),
        }
    }

    fn allows(&self, parts: &Parts) -> bool {
        let (Some(expected_peer), Some(expected_token)) = (self.peer, self.token.as_deref()) else {
            return false;
        };
        let Some(peer) = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip())
        else {
            return false;
        };
        if peer != expected_peer {
            return false;
        }
        let Some(candidate) = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
        else {
            return false;
        };
        candidate.len() == expected_token.len()
            && bool::from(candidate.as_bytes().ct_eq(expected_token))
    }
}

struct MetricsAccess;

impl FromRequestParts<Arc<crate::state::AppState>> for MetricsAccess {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<crate::state::AppState>,
    ) -> Result<Self, Self::Rejection> {
        if parts
            .extensions
            .get::<Arc<MetricsScrapeAuthority>>()
            .is_some_and(|authority| authority.allows(parts))
        {
            return Ok(Self);
        }

        let user = PlatformUser::from_request_parts(parts, state).await?;
        if !user
            .0
            .has_platform_capability(PlatformCapability::PlatformOperationsManage)
        {
            return Err(StatusCode::FORBIDDEN);
        }
        Ok(Self)
    }
}

pub fn metrics_routes() -> Router<Arc<crate::state::AppState>> {
    Router::new().route("/metrics", get(metrics))
}

async fn metrics(
    Extension(metrics): Extension<Arc<RuntimeMetrics>>,
    _access: MetricsAccess,
) -> Result<Response, AppError> {
    Ok((
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        metrics.render(),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::time::Duration;

    use axum::Extension;
    use axum::body::{Body, to_bytes};
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::NoopHttpClient;
    use tower::ServiceExt;

    use crate::application::{
        ApplicationServices, LegacyMaintenanceAdapter, MaintenanceWorkerRunner, SystemClock,
        WorkerContextFactory,
    };
    use crate::config::AppConfig;
    use crate::db::migrations::run_migrations;
    use crate::integration::store::IntegrationStore;
    use crate::integration::webhook::FixtureWebhookIngress;
    use crate::middleware::tenant::Tenant;
    use crate::observability::{MetricsSink, RuntimeMetrics};
    use crate::registry::ModuleRegistry;
    use crate::repositories::{RepositoryProvider, SqliteRepositoryProvider};
    use crate::services::auth_service;
    use crate::state::AppState;

    use super::{MetricsScrapeAuthority, metrics_routes};

    fn config() -> AppConfig {
        AppConfig {
            host: "127.0.0.1".into(),
            port: 0,
            db_path: ":memory:".into(),
            is_production: true,
            public_https: false,
            auth_cookie_name: "talos_session".into(),
            session_ttl_days: 7,
            auth_cookie_secure: true,
            auth_bootstrap_on_start: false,
            auth_bootstrap_admin_username: String::new(),
            auth_bootstrap_admin_password: String::new(),
            auth_login_rate_window_ms: 60_000,
            auth_login_rate_max_attempts: 5,
            auth_login_rate_block_ms: 300_000,
            cors_allowed_origin: None,
            public_dir: String::new(),
            vite_dev_url: None,
        }
    }

    fn tenant() -> Tenant {
        Tenant {
            id: "tenant-metrics".into(),
            name: "Metrics Tenant".into(),
            slug: "metrics".into(),
            status: "active".into(),
            plan: "test".into(),
            settings: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn insert_identity(pool: &Pool<SqliteConnectionManager>, id: &str) {
        pool.get()
            .unwrap()
            .execute(
                "INSERT INTO identities
                 (id, username, email, password_hash, display_name, phone, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'not-used', ?2, '', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                rusqlite::params![id, id, format!("{id}@example.test")],
            )
            .unwrap();
    }

    fn app_and_tokens() -> (axum::Router, Arc<RuntimeMetrics>, String, String, String) {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        run_migrations(&pool.get().unwrap()).unwrap();
        let selected_tenant = tenant();
        pool.get()
            .unwrap()
            .execute(
                "INSERT INTO tenants
                 (id, name, slug, status, plan, settings, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7)",
                rusqlite::params![
                    selected_tenant.id,
                    selected_tenant.name,
                    selected_tenant.slug,
                    selected_tenant.status,
                    selected_tenant.plan,
                    selected_tenant.created_at,
                    selected_tenant.updated_at,
                ],
            )
            .unwrap();
        insert_identity(&pool, "tenant-staff");
        insert_identity(&pool, "tenant-admin");
        insert_identity(&pool, "platform-operator");
        let connection = pool.get().unwrap();
        connection
            .execute(
                "INSERT INTO tenant_memberships
                 (id, identity_id, tenant_id, role, status, created_at, updated_at)
                 VALUES ('membership-staff', 'tenant-staff', 'tenant-metrics', 'staff', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO tenant_memberships
                 (id, identity_id, tenant_id, role, status, created_at, updated_at)
                 VALUES ('membership-admin', 'tenant-admin', 'tenant-metrics', 'admin', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO platform_memberships
                 (id, identity_id, status, created_at, updated_at)
                 VALUES ('membership-operator', 'platform-operator', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO platform_role_grants
                 (id, platform_membership_id, role, granted_by_identity_id, granted_at)
                 VALUES ('grant-operator', 'membership-operator', 'platform_operator', NULL, '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        drop(connection);

        let staff_token = auth_service::create_session(&pool, "tenant-staff")
            .unwrap()
            .id;
        let admin_token = auth_service::create_session(&pool, "tenant-admin")
            .unwrap()
            .id;
        let operator_token = auth_service::create_session(&pool, "platform-operator")
            .unwrap()
            .id;
        let metrics = Arc::new(RuntimeMetrics::default());
        let registry = Arc::new(ModuleRegistry::new(HashMap::new()).unwrap());
        let http_client: Arc<dyn system_core::transport::http_client::HttpClient> =
            Arc::new(NoopHttpClient);
        let worker_contexts = WorkerContextFactory::new(Arc::new(SystemClock), http_client.clone());
        let worker = Arc::new(MaintenanceWorkerRunner::new(
            worker_contexts,
            Arc::new(LegacyMaintenanceAdapter::new(pool.clone())),
        ));
        let repository_provider: Arc<dyn RepositoryProvider> =
            Arc::new(SqliteRepositoryProvider::new(pool.clone()));
        let services = Arc::new(ApplicationServices::production(
            registry.clone(),
            Arc::new(SystemClock),
            worker,
            repository_provider,
        ));
        let state = Arc::new(AppState::new_local(
            pool.clone(),
            config(),
            registry,
            http_client,
            Arc::new(FixtureWebhookIngress::new_with_metrics(
                IntegrationStore::new(pool),
                metrics.clone(),
            )),
            services,
        ));
        (
            metrics_routes()
                .layer(Extension(metrics.clone()))
                .layer(Extension(Arc::new(MetricsScrapeAuthority::for_test(
                    IpAddr::V4(Ipv4Addr::new(172, 29, 0, 40)),
                    &"ab".repeat(32),
                ))))
                .with_state(state),
            metrics,
            staff_token,
            admin_token,
            operator_token,
        )
    }

    fn request(token: Option<&str>, tenant_authority: bool) -> Request<Body> {
        let mut builder = Request::builder().uri("/metrics");
        if let Some(token) = token {
            builder = builder.header("cookie", format!("talos_session={token}"));
        }
        if !tenant_authority {
            builder = builder.header("x-talos-authority", "platform");
        }
        let mut request = builder.body(Body::empty()).unwrap();
        if tenant_authority {
            request.extensions_mut().insert(tenant());
        }
        request
    }

    fn scrape_request(token: &str, peer: Ipv4Addr) -> Request<Body> {
        let mut request = Request::builder()
            .uri("/metrics")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((peer, 43123))));
        request
    }

    #[tokio::test]
    async fn metrics_scrape_authority_requires_exact_peer_and_constant_time_token() {
        let (app, _, _, _, _) = app_and_tokens();
        let valid = "ab".repeat(32);

        let accepted = app
            .clone()
            .oneshot(scrape_request(&valid, Ipv4Addr::new(172, 29, 0, 40)))
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::OK);

        let nginx_peer = app
            .clone()
            .oneshot(scrape_request(&valid, Ipv4Addr::new(172, 29, 0, 30)))
            .await
            .unwrap();
        assert_eq!(nginx_peer.status(), StatusCode::UNAUTHORIZED);

        let wrong_token = app
            .oneshot(scrape_request(
                &"cd".repeat(32),
                Ipv4Addr::new(172, 29, 0, 40),
            ))
            .await
            .unwrap();
        assert_eq!(wrong_token.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn metrics_route_denies_untrusted_tenant_authorities_and_allows_platform_operations() {
        let (app, metrics, staff_token, admin_token, operator_token) = app_and_tokens();
        metrics.http_request("GET", "/metrics", 200, Duration::ZERO);

        let unauthenticated = app.clone().oneshot(request(None, false)).await.unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
        let staff = app
            .clone()
            .oneshot(request(Some(&staff_token), true))
            .await
            .unwrap();
        assert_eq!(staff.status(), StatusCode::FORBIDDEN);
        let tenant_admin = app
            .clone()
            .oneshot(request(Some(&admin_token), true))
            .await
            .unwrap();
        assert_eq!(tenant_admin.status(), StatusCode::FORBIDDEN);

        let authorized = app
            .oneshot(request(Some(&operator_token), false))
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::OK);
        assert_eq!(
            authorized.headers().get("content-type").unwrap(),
            "text/plain; version=0.0.4"
        );
        let body = to_bytes(authorized.into_body(), 64 * 1024).await.unwrap();
        let rendered = String::from_utf8(body.to_vec()).unwrap();
        assert!(rendered.contains(
            "talos_http_requests_total{method=\"get\",route=\"metrics\",status=\"2xx\"} 1"
        ));
        assert!(!rendered.contains(&staff_token));
        assert!(!rendered.contains(&operator_token));
        assert!(!rendered.contains("tenant-metrics"));
    }
}
