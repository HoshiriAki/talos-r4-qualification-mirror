use crate::application::ApplicationServices;
use crate::config::AppConfig;
use crate::integration::webhook::FixtureWebhookIngress;
use crate::registry::ModuleRegistry;
use crate::repositories::{
    AuditCompatibilityRepository, AuthSecurityRepository, IdentityAuthorityRepository,
    MachineAuthorityRepository, PlatformMembershipRepository, PlatformTenantRepository,
    TenantResolutionRepository,
};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use system_core::transport::http_client::HttpClient;

#[cfg(feature = "postgres")]
use sqlx::postgres::PgPool;

const FIXTURE_WEBHOOK_RATE_LIMIT: u32 = 60;
const FIXTURE_WEBHOOK_RATE_WINDOW: Duration = Duration::from_secs(60);
const FIXTURE_WEBHOOK_RATE_BUCKETS: usize = 4096;

struct WebhookRateBucket {
    window_started: Instant,
    requests: u32,
}

/// Process-local ingress pressure guard. The durable webhook inbox remains the
/// source of truth; this only prevents one endpoint capability from repeatedly
/// reaching verification and persistence during a short window. The socket peer
/// is deliberately not part of the key because a reverse proxy would otherwise
/// collapse every sender onto one transport address; P5 owns the future generic
/// provider-network policy.
pub struct FixtureWebhookRateLimiter {
    buckets: Mutex<BTreeMap<String, WebhookRateBucket>>,
}

impl Default for FixtureWebhookRateLimiter {
    fn default() -> Self {
        Self {
            buckets: Mutex::new(BTreeMap::new()),
        }
    }
}

impl FixtureWebhookRateLimiter {
    pub fn allow(&self, _peer_ip: IpAddr, endpoint_token: &str) -> bool {
        let key = hex::encode(Sha256::digest(endpoint_token.as_bytes()));
        let now = Instant::now();
        let Ok(mut buckets) = self.buckets.lock() else {
            // Fail closed if the process cannot safely maintain the bound.
            return false;
        };

        if let Some(bucket) = buckets.get_mut(&key) {
            if now.duration_since(bucket.window_started) >= FIXTURE_WEBHOOK_RATE_WINDOW {
                bucket.window_started = now;
                bucket.requests = 1;
                return true;
            }
            if bucket.requests >= FIXTURE_WEBHOOK_RATE_LIMIT {
                return false;
            }
            bucket.requests += 1;
            return true;
        }

        if buckets.len() >= FIXTURE_WEBHOOK_RATE_BUCKETS {
            buckets.retain(|_, bucket| {
                now.duration_since(bucket.window_started) < FIXTURE_WEBHOOK_RATE_WINDOW
            });
            if buckets.len() >= FIXTURE_WEBHOOK_RATE_BUCKETS {
                return false;
            }
        }
        buckets.insert(
            key,
            WebhookRateBucket {
                window_started: now,
                requests: 1,
            },
        );
        true
    }
}

#[derive(Clone)]
pub(crate) struct AppStateRepositories {
    audit_compatibility_repository: AuditCompatibilityRepository,
    auth_security_repository: AuthSecurityRepository,
    identity_authority_repository: IdentityAuthorityRepository,
    machine_authority_repository: MachineAuthorityRepository,
    platform_membership_repository: PlatformMembershipRepository,
    platform_tenant_repository: PlatformTenantRepository,
    tenant_resolution_repository: TenantResolutionRepository,
}

impl AppStateRepositories {
    pub(crate) fn new(
        audit_compatibility_repository: AuditCompatibilityRepository,
        auth_security_repository: AuthSecurityRepository,
        identity_authority_repository: IdentityAuthorityRepository,
        machine_authority_repository: MachineAuthorityRepository,
        platform_membership_repository: PlatformMembershipRepository,
        platform_tenant_repository: PlatformTenantRepository,
        tenant_resolution_repository: TenantResolutionRepository,
    ) -> Self {
        Self {
            audit_compatibility_repository,
            auth_security_repository,
            identity_authority_repository,
            machine_authority_repository,
            platform_membership_repository,
            platform_tenant_repository,
            tenant_resolution_repository,
        }
    }

    fn local(pool: &Pool<SqliteConnectionManager>) -> Self {
        Self::new(
            AuditCompatibilityRepository::new(pool.clone()),
            AuthSecurityRepository::new(pool.clone()),
            IdentityAuthorityRepository::new(pool.clone()),
            MachineAuthorityRepository::new(pool.clone()),
            PlatformMembershipRepository::new(pool.clone()),
            PlatformTenantRepository::new(pool.clone()),
            TenantResolutionRepository::new(pool.clone()),
        )
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub registry: Arc<ModuleRegistry>,
    pub http_client: Arc<dyn HttpClient>,
    pub integration_webhook_ingress: Arc<FixtureWebhookIngress>,
    pub integration_webhook_rate_limiter: Arc<FixtureWebhookRateLimiter>,
    application_services: Arc<ApplicationServices>,
    audit_compatibility_repository: AuditCompatibilityRepository,
    auth_security_repository: AuthSecurityRepository,
    identity_authority_repository: IdentityAuthorityRepository,
    machine_authority_repository: MachineAuthorityRepository,
    platform_membership_repository: PlatformMembershipRepository,
    platform_tenant_repository: PlatformTenantRepository,
    tenant_resolution_repository: TenantResolutionRepository,
    #[cfg(feature = "postgres")]
    pub pg_pool: Option<PgPool>,
}

impl AppState {
    pub(crate) fn new(
        config: AppConfig,
        registry: Arc<ModuleRegistry>,
        http_client: Arc<dyn HttpClient>,
        integration_webhook_ingress: Arc<FixtureWebhookIngress>,
        application_services: Arc<ApplicationServices>,
        repositories: AppStateRepositories,
    ) -> Self {
        AppState {
            config,
            registry,
            http_client,
            integration_webhook_ingress,
            integration_webhook_rate_limiter: Arc::new(FixtureWebhookRateLimiter::default()),
            application_services,
            audit_compatibility_repository: repositories.audit_compatibility_repository,
            auth_security_repository: repositories.auth_security_repository,
            identity_authority_repository: repositories.identity_authority_repository,
            machine_authority_repository: repositories.machine_authority_repository,
            platform_membership_repository: repositories.platform_membership_repository,
            platform_tenant_repository: repositories.platform_tenant_repository,
            tenant_resolution_repository: repositories.tenant_resolution_repository,
            #[cfg(feature = "postgres")]
            pg_pool: None,
        }
    }

    pub(crate) fn new_local(
        pool: Pool<SqliteConnectionManager>,
        config: AppConfig,
        registry: Arc<ModuleRegistry>,
        http_client: Arc<dyn HttpClient>,
        integration_webhook_ingress: Arc<FixtureWebhookIngress>,
        application_services: Arc<ApplicationServices>,
    ) -> Self {
        let repositories = AppStateRepositories::local(&pool);
        Self::new(
            config,
            registry,
            http_client,
            integration_webhook_ingress,
            application_services,
            repositories,
        )
    }

    pub fn application_services(&self) -> Arc<ApplicationServices> {
        self.application_services.clone()
    }

    pub(crate) fn audit_compatibility_repository(&self) -> &AuditCompatibilityRepository {
        &self.audit_compatibility_repository
    }

    pub(crate) fn auth_security_repository(&self) -> &AuthSecurityRepository {
        &self.auth_security_repository
    }

    pub(crate) fn identity_authority_repository(&self) -> &IdentityAuthorityRepository {
        &self.identity_authority_repository
    }

    pub(crate) fn machine_authority_repository(&self) -> &MachineAuthorityRepository {
        &self.machine_authority_repository
    }

    pub(crate) fn platform_membership_repository(&self) -> &PlatformMembershipRepository {
        &self.platform_membership_repository
    }

    pub(crate) fn platform_tenant_repository(&self) -> &PlatformTenantRepository {
        &self.platform_tenant_repository
    }

    pub(crate) fn tenant_resolution_repository(&self) -> &TenantResolutionRepository {
        &self.tenant_resolution_repository
    }

    #[cfg(feature = "postgres")]
    pub fn set_pg_pool(&mut self, pg: PgPool) {
        self.pg_pool = Some(pg);
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::{FIXTURE_WEBHOOK_RATE_LIMIT, FixtureWebhookRateLimiter};

    #[test]
    fn fixture_webhook_rate_limit_is_scoped_to_endpoint_token_digest() {
        let limiter = FixtureWebhookRateLimiter::default();
        let peer = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        for _ in 0..FIXTURE_WEBHOOK_RATE_LIMIT {
            assert!(limiter.allow(peer, "bearer-token-a"));
        }
        assert!(!limiter.allow(peer, "bearer-token-a"));
        assert!(limiter.allow(peer, "bearer-token-b"));
        assert!(!limiter.allow(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)), "bearer-token-a"));
    }
}
