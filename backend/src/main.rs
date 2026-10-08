#![allow(dead_code)]
mod application;
mod auth_contract;
mod config;
mod crypto;
mod db;
mod domain;
mod error;
mod http;
mod integration;
mod middleware;
mod observability;
mod registry;
mod repositories;
mod routes;
mod services;
mod state;
mod utils;

use crate::config::{AppConfig, DatabaseProfile, database_profile_from_env, normalize_cors_origin};
use crate::db::pool::create_pool;
use crate::state::{AppState, AppStateRepositories};
use application::{
    ApplicationServices, AuditCompatibilityModule, ConsentCompatibilityModule,
    DeletionCompatibilityModule, DurableRentalWorkflowWorker, LegacyMaintenanceAdapter,
    MaintenanceWorkerRunner, ReportCompatibilityModule, SystemClock,
    TenantGovernanceCompatibilityModule, TenantMembershipCompatibilityModule,
    TenantPreviewCompatibilityModule, TenantSimulationCompatibilityModule,
    TwoFaCompatibilityModule, UserSettingsCompatibilityModule, WorkerContextFactory, WorkerRunner,
};
#[cfg(feature = "postgres")]
use repositories::PostgresRepositoryProvider;
use repositories::{
    AuditCompatibilityRepository, AuthSecurityRepository, BootstrapAuthorityRepository,
    ConsentCompatibilityRepository, DeletionCompatibilityRepository, IdentityAuthorityRepository,
    MachineAuthorityRepository, MaintenanceCompatibilityRepository, PlatformMembershipRepository,
    PlatformTenantRepository, ReportCompatibilityRepository, RepositoryProvider,
    SqliteRepositoryProvider, TenantGovernanceRepository, TenantMembershipAuthorityRepository,
    TenantPreviewRepository, TenantResolutionRepository, UserSettingsProfileRepository,
    WorkflowWorkerTenantSource,
};
use std::net::SocketAddr;
use std::sync::Arc;
use system_core::NoopHttpClient;
use system_core::transport::http_client::HttpClient;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mut config = AppConfig::from_env();

    // Validate credential CORS before database migration, worker startup or
    // other process side effects. Production must declare one explicit origin;
    // development may derive the local server origin. Store only the normalized
    // origin tuple in AppConfig so create_router can preserve its established R3
    // composition signature while placing CORS inside the P4 governance layer.
    let raw_cors_origin = match config.cors_allowed_origin.clone() {
        Some(origin) => origin,
        None if config.is_production => {
            anyhow::bail!("CORS_ALLOWED_ORIGIN is required in production");
        }
        None => format!("http://{}:{}", config.host, config.port),
    };
    let cors_origin = normalize_cors_origin(&raw_cors_origin, config.public_https)
        .map_err(|reason| anyhow::anyhow!("invalid CORS_ALLOWED_ORIGIN: {reason}"))?;
    config.cors_allowed_origin = Some(cors_origin);

    // R4-P8: resolve one explicit database runtime profile before opening any
    // database. Production defaults to PostgreSQL 18 and rejects SQLite or an
    // unknown backend instead of silently falling back to the local profile.
    let database_profile = database_profile_from_env(config.is_production)
        .map_err(|reason| anyhow::anyhow!("invalid database profile: {reason}"))?;
    tracing::info!("Database profile: {:?}", database_profile);

    #[cfg(feature = "postgres")]
    let pg_pool: Option<sqlx::postgres::PgPool> = if database_profile == DatabaseProfile::Postgres18
    {
        match db::migrations_pg::create_pg_pool().await {
            Ok(pool) => {
                tracing::info!("PostgreSQL connection established");
                match db::run_all_pg_migrations(&pool).await {
                    Ok(executed) => {
                        if executed.is_empty() {
                            tracing::info!("No pending PG migrations");
                        } else {
                            tracing::info!("Applied PG migrations: {:?}", executed);
                        }
                    }
                    Err(e) => {
                        tracing::error!("PG migration error: {}", e);
                        return Err(e);
                    }
                }
                Some(pool)
            }
            Err(e) => {
                tracing::error!("Failed to connect to PostgreSQL: {}", e);
                return Err(e);
            }
        }
    } else {
        None
    };

    #[cfg(not(feature = "postgres"))]
    if database_profile == DatabaseProfile::Postgres18 {
        anyhow::bail!("PostgreSQL 18 database profile requires a postgres-enabled build");
    }

    // R4-P5 production durability is a startup admission fact, not a late
    // worker concern. Reject an invalid production profile before SQLite
    // migrations, admin bootstrap, telemetry initialization or worker startup
    // can create side effects that make a failed process look partially live.
    #[cfg(feature = "postgres")]
    if config.is_production && pg_pool.is_none() {
        anyhow::bail!("R4-P5 production webhook Event Lane requires PostgreSQL 18 durable driver");
    }
    #[cfg(not(feature = "postgres"))]
    if config.is_production {
        anyhow::bail!("R4-P5 production webhook Event Lane requires a postgres-enabled build");
    }

    // R4-P8 production admission is now open for PostgreSQL 18. The selected
    // profile has already connected and migrated above, and production still
    // fails closed if that PostgreSQL authority is unavailable. SQLite remains
    // an explicit local/dev/test capability only; it is never manufactured as
    // a fallback for the PostgreSQL profile.

    // Sentry is platform telemetry rather than provider/application egress and
    // remains a separately governed platform-infrastructure surface for P7/P8.
    // It is intentionally initialized only after production database admission
    // has succeeded.
    if let Ok(dsn) = std::env::var("SENTRY_DSN") {
        let _guard = sentry::init((
            dsn,
            sentry::ClientOptions {
                release: sentry::release_name!(),
                traces_sample_rate: 0.1,
                ..Default::default()
            },
        ));
        tracing::info!("Sentry initialized");
    }

    // SQLite is an explicit local/dev/test capability, not an ambient process
    // requirement. Keep it absent for the PostgreSQL profile so later removal of
    // the transitional admission barrier cannot silently manufacture a SQLite
    // business fallback merely to satisfy startup composition.
    let sqlite_pool = if database_profile == DatabaseProfile::SqliteLocal {
        tracing::info!("SQLite local database path: {}", config.db_path);
        Some(create_pool(&config.db_path)?)
    } else {
        None
    };
    let require_sqlite_pool = |consumer: &'static str| {
        sqlite_pool.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "CFG_SQLITE_CAPABILITY_REQUIRED: {consumer} requires the local SQLite profile"
            )
        })
    };

    // P8-D authority composition is selected once at the process composition root.
    // PostgreSQL production is admitted here only because every reachable
    // persistence consumer has a PostgreSQL owner. Missing PostgreSQL authority
    // must never silently restore SQLite for auth, identity, machine or business state.
    #[cfg(feature = "postgres")]
    let (
        audit_compatibility_repository,
        auth_security_repository,
        bootstrap_authority_repository,
        consent_compatibility_repository,
        deletion_compatibility_repository,
        identity_authority_repository,
        machine_authority_repository,
        platform_membership_repository,
        platform_tenant_repository,
        report_compatibility_repository,
        tenant_governance_repository,
        tenant_preview_repository,
        tenant_membership_authority_repository,
        user_settings_profile_repository,
        tenant_resolution_repository,
    ) = match pg_pool.clone() {
        Some(pg) => (
            AuditCompatibilityRepository::postgres(pg.clone()),
            AuthSecurityRepository::postgres(pg.clone()),
            BootstrapAuthorityRepository::postgres(pg.clone()),
            ConsentCompatibilityRepository::postgres(pg.clone()),
            DeletionCompatibilityRepository::postgres(pg.clone()),
            IdentityAuthorityRepository::postgres(pg.clone()),
            MachineAuthorityRepository::postgres(pg.clone()),
            PlatformMembershipRepository::postgres(pg.clone()),
            PlatformTenantRepository::postgres(pg.clone()),
            ReportCompatibilityRepository::postgres(pg.clone()),
            TenantGovernanceRepository::postgres(pg.clone()),
            TenantPreviewRepository::postgres(pg.clone()),
            TenantMembershipAuthorityRepository::postgres(pg.clone()),
            UserSettingsProfileRepository::postgres(pg.clone()),
            TenantResolutionRepository::postgres(pg),
        ),
        None => {
            let pool = require_sqlite_pool("local compatibility repositories")?;
            (
                AuditCompatibilityRepository::new(pool.clone()),
                AuthSecurityRepository::new(pool.clone()),
                BootstrapAuthorityRepository::new(pool.clone()),
                ConsentCompatibilityRepository::new(pool.clone()),
                DeletionCompatibilityRepository::new(pool.clone()),
                IdentityAuthorityRepository::new(pool.clone()),
                MachineAuthorityRepository::new(pool.clone()),
                PlatformMembershipRepository::new(pool.clone()),
                PlatformTenantRepository::new(pool.clone()),
                ReportCompatibilityRepository::new(pool.clone()),
                TenantGovernanceRepository::new(pool.clone()),
                TenantPreviewRepository::new(pool.clone()),
                TenantMembershipAuthorityRepository::new(pool.clone()),
                UserSettingsProfileRepository::new(pool.clone()),
                TenantResolutionRepository::new(pool.clone()),
            )
        }
    };
    #[cfg(not(feature = "postgres"))]
    let (
        audit_compatibility_repository,
        auth_security_repository,
        bootstrap_authority_repository,
        consent_compatibility_repository,
        deletion_compatibility_repository,
        identity_authority_repository,
        machine_authority_repository,
        platform_membership_repository,
        platform_tenant_repository,
        report_compatibility_repository,
        tenant_governance_repository,
        tenant_preview_repository,
        tenant_membership_authority_repository,
        user_settings_profile_repository,
        tenant_resolution_repository,
    ) = {
        let pool = require_sqlite_pool("local compatibility repositories")?;
        (
            AuditCompatibilityRepository::new(pool.clone()),
            AuthSecurityRepository::new(pool.clone()),
            BootstrapAuthorityRepository::new(pool.clone()),
            ConsentCompatibilityRepository::new(pool.clone()),
            DeletionCompatibilityRepository::new(pool.clone()),
            IdentityAuthorityRepository::new(pool.clone()),
            MachineAuthorityRepository::new(pool.clone()),
            PlatformMembershipRepository::new(pool.clone()),
            PlatformTenantRepository::new(pool.clone()),
            ReportCompatibilityRepository::new(pool.clone()),
            TenantGovernanceRepository::new(pool.clone()),
            TenantPreviewRepository::new(pool.clone()),
            TenantMembershipAuthorityRepository::new(pool.clone()),
            UserSettingsProfileRepository::new(pool.clone()),
            TenantResolutionRepository::new(pool.clone()),
        )
    };

    #[cfg(feature = "postgres")]
    let maintenance_compatibility_repository = match pg_pool.clone() {
        Some(pg) => MaintenanceCompatibilityRepository::postgres(pg),
        None => MaintenanceCompatibilityRepository::new(require_sqlite_pool(
            "maintenance compatibility",
        )?),
    };
    #[cfg(not(feature = "postgres"))]
    let maintenance_compatibility_repository =
        MaintenanceCompatibilityRepository::new(require_sqlite_pool("maintenance compatibility")?);

    // Run SQLite migrations only when the local SQLite capability was selected.
    #[cfg(feature = "sqlite")]
    if let Some(pool) = sqlite_pool.as_ref() {
        let conn = pool.get()?;
        let executed = db::run_all_sqlite_migrations(&conn)?;
        if executed.is_empty() {
            tracing::info!("No pending SQLite migrations");
        } else {
            tracing::info!("Applied SQLite migrations: {:?}", executed);
        }
    }

    // Bootstrap the first platform owner through the selected database authority.
    services::auth_service::ensure_initial_admin_with_repository(
        &bootstrap_authority_repository,
        &config,
    );

    let metrics = Arc::new(crate::observability::RuntimeMetrics::default());

    // P8-D selects the scoped repository family and workflow tenant discovery
    // from the same backend authority. Handlers/workers consume only these
    // composed interfaces and never choose a database independently.
    #[cfg(feature = "postgres")]
    let (repository_provider, workflow_worker_tenant_source): (
        Arc<dyn RepositoryProvider>,
        WorkflowWorkerTenantSource,
    ) = match pg_pool.clone() {
        Some(pg) => (
            Arc::new(PostgresRepositoryProvider::new_with_metrics(
                pg.clone(),
                metrics.clone(),
            )),
            WorkflowWorkerTenantSource::postgres(pg),
        ),
        None => {
            let pool = require_sqlite_pool("repository provider and workflow tenant source")?;
            (
                Arc::new(SqliteRepositoryProvider::new_with_metrics(
                    pool.clone(),
                    metrics.clone(),
                )),
                WorkflowWorkerTenantSource::new(pool),
            )
        }
    };
    #[cfg(not(feature = "postgres"))]
    let (repository_provider, workflow_worker_tenant_source): (
        Arc<dyn RepositoryProvider>,
        WorkflowWorkerTenantSource,
    ) = {
        let pool = require_sqlite_pool("repository provider and workflow tenant source")?;
        (
            Arc::new(SqliteRepositoryProvider::new_with_metrics(
                pool.clone(),
                metrics.clone(),
            )),
            WorkflowWorkerTenantSource::new(pool),
        )
    };

    // One configured KeyStore authority is shared by whichever Integration
    // persistence profile is selected. PostgreSQL production must not construct
    // the SQLite compatibility store merely to satisfy Registry assembly.
    let integration_key_store = crate::integration::keystore::configured_fixture_key_store()?;
    #[cfg(feature = "postgres")]
    let integration_store = sqlite_pool.clone().map(|pool| {
        crate::integration::store::IntegrationStore::with_key_store_and_metrics(
            pool,
            integration_key_store.clone(),
            metrics.clone(),
        )
    });
    #[cfg(not(feature = "postgres"))]
    let integration_store = Some(
        crate::integration::store::IntegrationStore::with_key_store_and_metrics(
            require_sqlite_pool("integration compatibility store")?,
            integration_key_store.clone(),
            metrics.clone(),
        ),
    );
    let local_integration_store = || {
        integration_store
            .clone()
            .expect("local IntegrationStore required without PostgreSQL persistence")
    };
    #[cfg(feature = "postgres")]
    let integration_module = match pg_pool.clone() {
        Some(pg) => crate::integration::IntegrationModule::new_with_postgres_persistence(
            pg,
            integration_key_store.clone(),
            metrics.clone(),
        ),
        None => crate::integration::IntegrationModule::new(local_integration_store()),
    };
    #[cfg(not(feature = "postgres"))]
    let integration_module = crate::integration::IntegrationModule::new(local_integration_store());
    #[cfg(feature = "postgres")]
    let integration_webhook_ingress = Arc::new(match pg_pool.clone() {
        Some(pg) => crate::integration::webhook::FixtureWebhookIngress::from_postgres_with_metrics(
            pg,
            metrics.clone(),
        ),
        None => crate::integration::webhook::FixtureWebhookIngress::new_with_metrics(
            local_integration_store(),
            metrics.clone(),
        ),
    });
    #[cfg(not(feature = "postgres"))]
    let integration_webhook_ingress = Arc::new(
        crate::integration::webhook::FixtureWebhookIngress::new_with_metrics(
            local_integration_store(),
            metrics.clone(),
        ),
    );

    // R4-P5: the legacy ExecutionContext HTTP slot is retained only for ABI
    // compatibility. It is intentionally fail-closed at the production
    // composition root. Any real outbound network operation must be admitted
    // through an explicit EgressGrant and GovernedEgressTransport instead of
    // inheriting ambient process network privilege.
    let http_client: Arc<dyn HttpClient> = Arc::new(NoopHttpClient);
    let clock = Arc::new(SystemClock);
    let worker_contexts = WorkerContextFactory::new(clock.clone(), http_client.clone());
    let maintenance = Arc::new(LegacyMaintenanceAdapter::new_with_repositories(
        maintenance_compatibility_repository,
        auth_security_repository.clone(),
    ));
    let rental_workflow = Arc::new(
        DurableRentalWorkflowWorker::new_with_tenant_source_and_metrics(
            workflow_worker_tenant_source,
            worker_contexts.clone(),
            repository_provider.clone(),
            clock.clone(),
            metrics.clone(),
        ),
    );
    let worker_runner: Arc<dyn WorkerRunner> = Arc::new(
        MaintenanceWorkerRunner::new(worker_contexts, maintenance)
            .with_rental_workflow(rental_workflow),
    );

    // Compose the durable Registry audit authority before startup workers run.
    // A missing PostgreSQL audit authority must fail admission before worker
    // side effects begin; production must never fall back to the SQLite audit sink.
    #[cfg(feature = "postgres")]
    let registry_audit_sink: Arc<dyn crate::registry::audit_sink::AuditSink> = match pg_pool.clone()
    {
        Some(pg) => Arc::new(
            crate::registry::audit_sink_postgres::PostgresAuditSink::new(pg)
                .map_err(anyhow::Error::msg)?,
        ),
        None => Arc::new(
            crate::registry::audit_sink::SqliteAuditSink::new(require_sqlite_pool(
                "registry audit sink",
            )?)
            .map_err(anyhow::Error::msg)?,
        ),
    };
    #[cfg(not(feature = "postgres"))]
    let registry_audit_sink: Arc<dyn crate::registry::audit_sink::AuditSink> = Arc::new(
        crate::registry::audit_sink::SqliteAuditSink::new(require_sqlite_pool(
            "registry audit sink",
        )?)
        .map_err(anyhow::Error::msg)?,
    );

    let audit_module = AuditCompatibilityModule::new(audit_compatibility_repository.clone());
    let consent_module = ConsentCompatibilityModule::new(consent_compatibility_repository);
    let deletion_module = DeletionCompatibilityModule::new(deletion_compatibility_repository);
    let report_module = ReportCompatibilityModule::new(report_compatibility_repository);
    let two_fa_module = TwoFaCompatibilityModule::new(auth_security_repository.clone());
    let staff_module =
        TenantMembershipCompatibilityModule::new(tenant_membership_authority_repository.clone());
    let user_settings_module =
        UserSettingsCompatibilityModule::new(user_settings_profile_repository);
    let tenant_governance_module =
        TenantGovernanceCompatibilityModule::new(tenant_governance_repository);
    let tenant_preview_module = TenantPreviewCompatibilityModule::new(tenant_preview_repository);
    #[cfg(feature = "postgres")]
    let tenant_simulation_module = match pg_pool.clone() {
        Some(pg) => TenantSimulationCompatibilityModule::with_postgres(pg),
        None => TenantSimulationCompatibilityModule::with_pool(require_sqlite_pool(
            "tenant simulation compatibility",
        )?),
    };
    #[cfg(not(feature = "postgres"))]
    let tenant_simulation_module = TenantSimulationCompatibilityModule::with_pool(
        require_sqlite_pool("tenant simulation compatibility")?,
    );

    let registry = Arc::new(
        crate::registry::ModuleRegistry::assemble_with_metrics_audit_sink_integration_and_staff_module(
            sqlite_pool.clone(),
            http_client.clone(),
            integration_module,
            audit_module,
            consent_module,
            deletion_module,
            report_module,
            two_fa_module,
            staff_module,
            user_settings_module,
            tenant_governance_module,
            tenant_preview_module,
            tenant_simulation_module,
            repository_provider.clone(),
            registry_audit_sink,
            metrics.clone(),
        )
        .expect("ModuleRegistry assembly failed"),
    );

    // Startup maintenance records each failure independently and never blocks
    // the next job or HTTP server startup.
    worker_runner.run_startup();

    // R4-P6: the codebase now has one complete PluginRuntimeServices composition
    // boundary, but this executable has no production Plugin authority profile
    // yet (publisher trust roots, principal permission policy, EgressGrant map,
    // secret bindings and storage quotas). Do not manufacture authority from the
    // fixture identities used by conformance tests. ApplicationServices therefore
    // starts with plugin runtime disabled; a later deployment Profile must attach
    // the complete runtime atomically through the trusted composition root.
    let application_services = Arc::new(ApplicationServices::production(
        registry.clone(),
        clock,
        worker_runner.clone(),
        repository_provider,
    ));

    // R4-P5 webhook completion is gated on normalized Event Lane admission.
    // PostgreSQL 18 is the only durable P3 Event driver. In-process is retained
    // solely as an explicit development/reference profile; admitted production
    // uses the PostgreSQL Event Lane.
    #[cfg(feature = "postgres")]
    let webhook_event_lane = match pg_pool.clone() {
        Some(pg) => crate::integration::webhook_event::WebhookEventLane::postgres(pg),
        None => crate::integration::webhook_event::WebhookEventLane::development(),
    };
    #[cfg(not(feature = "postgres"))]
    let webhook_event_lane = crate::integration::webhook_event::WebhookEventLane::development();

    #[cfg(feature = "postgres")]
    let integration_worker = Arc::new(match pg_pool.clone() {
        Some(pg) => crate::integration::GovernedIntegrationWorker::from_postgres_with_metrics(
            pg,
            webhook_event_lane,
            metrics.clone(),
        )?,
        None => crate::integration::GovernedIntegrationWorker::new_with_metrics(
            local_integration_store(),
            webhook_event_lane,
            metrics.clone(),
        )?,
    });
    #[cfg(not(feature = "postgres"))]
    let integration_worker = Arc::new(
        crate::integration::GovernedIntegrationWorker::new_with_metrics(
            local_integration_store(),
            webhook_event_lane,
            metrics.clone(),
        )?,
    );

    let maintenance_worker = worker_runner.spawn_periodic();
    let integration_worker = integration_worker.spawn_periodic();
    let app_state_repositories = AppStateRepositories::new(
        audit_compatibility_repository,
        auth_security_repository,
        identity_authority_repository,
        machine_authority_repository,
        platform_membership_repository,
        platform_tenant_repository,
        tenant_resolution_repository,
    );
    let mut state = AppState::new(
        config.clone(),
        registry,
        http_client,
        integration_webhook_ingress,
        application_services,
        app_state_repositories,
    );
    #[cfg(feature = "postgres")]
    if let Some(pg) = pg_pool {
        state.set_pg_pool(pg);
    }
    let state = Arc::new(state);

    // Security + observability middleware are applied inside create_router (routes/mod.rs).
    // Keep the established R3 composition call stable; P4 consumes the already
    // normalized CORS origin from AppConfig inside that boundary.
    let router = routes::create_router(state, metrics);

    // In dev mode (Vite proxy), bind to loopback so the backend is never
    // reachable directly from the browser — only the Vite proxy can hit it.
    let bind_host = if config.vite_dev_url.is_some() {
        "127.0.0.1"
    } else {
        &config.host
    };
    let addr: SocketAddr = format!("{}:{}", bind_host, config.port).parse()?;
    tracing::info!(
        "Listening on {} (dev={}, public_https={})",
        addr,
        config.vite_dev_url.is_some(),
        config.public_https,
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    // Periodic workers are application-owned tasks. Once HTTP admission has
    // stopped and in-flight requests have drained, explicitly cancel and join
    // them before flushing the final audit buffer. Runtime drop is not treated
    // as a clean-shutdown mechanism.
    maintenance_worker.abort();
    integration_worker.abort();
    let _ = maintenance_worker.await;
    let _ = integration_worker.await;
    tracing::info!("background workers stopped");

    services::audit_service::shutdown_audit_buffer();
    tracing::info!("graceful shutdown complete");

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install SIGINT handler");
    };

    #[cfg(unix)]
    {
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler")
                .recv()
                .await;
        };

        tokio::select! {
            _ = ctrl_c => tracing::info!(signal = "SIGINT", "shutdown signal received"),
            _ = terminate => tracing::info!(signal = "SIGTERM", "shutdown signal received"),
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await;
        tracing::info!(signal = "SIGINT", "shutdown signal received");
    }
}
