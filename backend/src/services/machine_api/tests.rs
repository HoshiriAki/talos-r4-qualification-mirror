#[cfg(test)]
mod cases {
    use super::super::*;
    use crate::registry::audit_sink::SqliteAuditSink;
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    use system_core::*;
    use uuid::Uuid;

    struct Probe(Arc<AtomicUsize>);
    impl SystemModule for Probe {
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                name: "probe".into(),
                version: "1".into(),
                description: "fixture".into(),
                author: "test".into(),
                wasm_compatible: false,
                storage: None,
            }
        }
        fn init(&mut self, _: Value) -> Result<(), String> {
            Ok(())
        }
        fn commands(&self) -> Vec<CommandMetadata> {
            vec![
                CommandMetadata::new(
                    "read",
                    AccessRequirement::Authenticated,
                    &[EffectClass::DatabaseRead],
                    SimulationSupport::Blocked,
                ),
                CommandMetadata::new(
                    "admin",
                    AccessRequirement::TenantAdmin,
                    &[EffectClass::DatabaseWrite],
                    SimulationSupport::Blocked,
                ),
            ]
        }
        fn schema(&self) -> ModuleSchema {
            ModuleSchema {
                name: "probe".into(),
                description: "fixture".into(),
                commands: ["read", "admin"]
                    .into_iter()
                    .map(|n| CommandSchema {
                        name: n.into(),
                        description: "fixture".into(),
                        version: "1".into(),
                        input_schema: None,
                        output_schema: None,
                    })
                    .collect(),
            }
        }
        fn execute(&self, _: &str, _: Value, ctx: &ExecutionContext) -> Result<Value, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(
                json!({"identity":ctx.user_id(),"tenant":ctx.data_scope().tenant_id().as_str(),"mode":"normal"}),
            )
        }
    }
    fn setup() -> (Db, ModuleRegistry, AuthUserInfo, Arc<AtomicUsize>) {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        {
            let conn = pool.get().unwrap();
            conn.execute_batch(crate::db::baseline::SQLITE_IDENTITY_AUTHORITY_BASELINE)
                .unwrap();
            conn.execute_batch(include_str!("../../db/migrations/066_r3_machine_api.sql"))
                .unwrap();
            conn.execute_batch(
                "PRAGMA foreign_keys=ON;
        INSERT INTO tenants VALUES ('tenant-a','A','a','active','free',NULL,'now','now');
        INSERT INTO tenants VALUES ('tenant-b','B','b','active','free',NULL,'now','now');
        INSERT INTO identities (id,username,password_hash,display_name,status,created_at,updated_at)
        VALUES ('owner','owner','hash','Owner','active','now','now');
        INSERT INTO tenant_memberships (id,identity_id,tenant_id,role,status,created_at,updated_at)
        VALUES ('member','owner','tenant-a','owner','active','now','now');",
            )
            .unwrap();
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let registry = ModuleRegistry::new_with_audit_sink(
            HashMap::from([(
                "probe".into(),
                Arc::new(Probe(calls.clone())) as Arc<dyn SystemModule>,
            )]),
            Arc::new(SqliteAuditSink::new(pool.clone()).unwrap()),
        )
        .unwrap();
        let auth = AuthUserInfo {
            id: "owner".into(),
            username: "owner".into(),
            session_id: "s".into(),
            display_name: "Owner".into(),
            email: "".into(),
            phone: "".into(),
            authority: AuthorityContext::Tenant {
                tenant_id: TenantId::new("tenant-a").unwrap(),
                membership_id: TenantMembershipId::new("member").unwrap(),
                role: TenantRole::Owner,
            },
        };
        (pool, registry, auth, calls)
    }
    fn create(pool: &Db, registry: &ModuleRegistry, auth: &AuthUserInfo, rpm: u16) -> Issued {
        provision(
            pool,
            registry,
            auth,
            Provision {
                name: "robot".into(),
                role: TenantRole::Staff,
                rate_limit_rpm: rpm,
                scopes: vec![
                    Scope {
                        module: "probe".into(),
                        command: "read".into(),
                    },
                    Scope {
                        module: "probe".into(),
                        command: "admin".into(),
                    },
                ],
            },
        )
        .unwrap()
    }
    fn execute(
        registry: &ModuleRegistry,
        pool: &Db,
        secret: &str,
        tenant: &str,
        command: &str,
    ) -> Result<Value, AppError> {
        registry.execute_machine(
            pool,
            secret,
            tenant,
            "v1",
            &Scope {
                module: "probe".into(),
                command: command.into(),
            },
            json!({}),
            Arc::new(NoopHttpClient),
            &Uuid::new_v4().to_string(),
            None,
        )
    }
    #[test]
    fn machine_is_distinct_tenant_bound_and_audited_without_secret() {
        let (pool, registry, auth, calls) = setup();
        let key = create(&pool, &registry, &auth, 60);
        let result = execute(&registry, &pool, &key.secret, "tenant-a", "read").unwrap();
        assert_ne!(result["identity"], "owner");
        assert_eq!(result["tenant"], "tenant-a");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let conn = pool.get().unwrap();
        let detail: String = conn
            .query_row(
                "SELECT group_concat(detail_json) FROM audit_events",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!detail.contains(&key.secret));
        let machine_audits: i64 = conn
            .query_row(
                "SELECT count(*) FROM audit_events WHERE actor_identity_id=?1",
                [result["identity"].as_str().unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(machine_audits, 3); // access plus Registry attempt/result
        drop(conn);
        let listed = list(&pool, &auth).unwrap();
        assert!(!listed.to_string().contains(&key.secret));
        assert!(listed["clients"][0]["last_used_at"].is_string());
    }
    #[test]
    fn unauthorized_wrong_tenant_out_of_scope_and_version_fail_closed() {
        let (pool, registry, auth, calls) = setup();
        let key = create(&pool, &registry, &auth, 60);
        assert!(matches!(
            execute(&registry, &pool, "invalid", "tenant-a", "read"),
            Err(AppError::Unauthorized)
        ));
        assert!(matches!(
            execute(&registry, &pool, &key.secret, "tenant-b", "read"),
            Err(AppError::Forbidden)
        ));
        assert!(matches!(
            execute(&registry, &pool, &key.secret, "tenant-a", "missing"),
            Err(AppError::Forbidden)
        ));
        assert!(
            registry
                .execute_machine(
                    &pool,
                    &key.secret,
                    "tenant-a",
                    "v2",
                    &Scope {
                        module: "probe".into(),
                        command: "read".into()
                    },
                    json!({}),
                    Arc::new(NoopHttpClient),
                    "v2",
                    None
                )
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn scope_never_elevates_staff_membership_to_admin() {
        let (pool, registry, auth, calls) = setup();
        let key = create(&pool, &registry, &auth, 60);
        assert!(execute(&registry, &pool, &key.secret, "tenant-a", "admin").is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn provisioner_disable_is_not_delegation_but_machine_disable_is_terminal_for_access() {
        let (pool, registry, auth, _) = setup();
        let key = create(&pool, &registry, &auth, 60);
        pool.get()
            .unwrap()
            .execute(
                "UPDATE identities SET status='disabled' WHERE id='owner'",
                [],
            )
            .unwrap();
        execute(&registry, &pool, &key.secret, "tenant-a", "read").unwrap();
        pool.get().unwrap().execute("UPDATE identities SET status='disabled' WHERE id IN (SELECT identity_id FROM machine_identities)",[]).unwrap();
        assert!(matches!(
            execute(&registry, &pool, &key.secret, "tenant-a", "read"),
            Err(AppError::Unauthorized)
        ));
    }
    #[test]
    fn revoked_membership_is_rechecked_each_request() {
        let (pool, registry, auth, _) = setup();
        let key = create(&pool, &registry, &auth, 60);
        pool.get().unwrap().execute("UPDATE tenant_memberships SET status='revoked' WHERE identity_id IN (SELECT identity_id FROM machine_identities)",[]).unwrap();
        assert!(matches!(
            execute(&registry, &pool, &key.secret, "tenant-a", "read"),
            Err(AppError::Unauthorized)
        ));
    }
    #[test]
    fn disable_rotate_and_revoke_are_immediate_and_no_physical_delete() {
        let (pool, registry, auth, _) = setup();
        let key = create(&pool, &registry, &auth, 60);
        lifecycle(&pool, &auth, &key.client_id, "disable").unwrap();
        assert!(execute(&registry, &pool, &key.secret, "tenant-a", "read").is_err());
        lifecycle(&pool, &auth, &key.client_id, "enable").unwrap();
        let next = lifecycle(&pool, &auth, &key.client_id, "rotate")
            .unwrap()
            .unwrap();
        assert!(execute(&registry, &pool, &key.secret, "tenant-a", "read").is_err());
        execute(&registry, &pool, &next.secret, "tenant-a", "read").unwrap();
        lifecycle(&pool, &auth, &key.client_id, "revoke").unwrap();
        assert!(execute(&registry, &pool, &next.secret, "tenant-a", "read").is_err());
        assert!(lifecycle(&pool, &auth, &key.client_id, "enable").is_err());
        assert!(
            pool.get()
                .unwrap()
                .execute("DELETE FROM api_clients WHERE id=?1", [key.client_id])
                .is_err()
        );
    }
    #[test]
    fn durable_rate_budget_survives_new_registry() {
        let (pool, registry, auth, calls) = setup();
        let key = create(&pool, &registry, &auth, 1);
        execute(&registry, &pool, &key.secret, "tenant-a", "read").unwrap();
        let fresh = ModuleRegistry::new_with_audit_sink(
            HashMap::from([(
                "probe".into(),
                Arc::new(Probe(calls.clone())) as Arc<dyn SystemModule>,
            )]),
            Arc::new(SqliteAuditSink::new(pool.clone()).unwrap()),
        )
        .unwrap();
        assert!(matches!(
            execute(&fresh, &pool, &key.secret, "tenant-a", "read"),
            Err(AppError::RateLimited { .. })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn expired_and_disabled_credentials_are_rejected() {
        for update in [
            "UPDATE api_credentials SET expires_at='2000-01-01T00:00:00Z'",
            "UPDATE api_credentials SET status='disabled'",
        ] {
            let (pool, registry, auth, calls) = setup();
            let key = create(&pool, &registry, &auth, 60);
            pool.get().unwrap().execute(update, []).unwrap();
            assert!(execute(&registry, &pool, &key.secret, "tenant-a", "read").is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
    }
    #[test]
    fn machine_cannot_login_get_password_or_platform_membership_or_retarget() {
        let (pool, registry, auth, _) = setup();
        let key = create(&pool, &registry, &auth, 60);
        let identity: String = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT identity_id FROM api_clients WHERE id=?1",
                [&key.client_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(crate::services::auth_service::create_session(&pool, &identity).is_err());
        assert!(
            crate::services::auth_service::update_identity_password(
                &pool,
                &identity,
                "new-password"
            )
            .is_err()
        );
        let conn = pool.get().unwrap();
        assert!(conn.execute("INSERT INTO platform_memberships (id,identity_id,status,created_at,updated_at) VALUES ('p',?1,'active','now','now')",[&identity]).is_err());
        assert!(
            conn.execute(
                "UPDATE api_clients SET tenant_id='tenant-b' WHERE id=?1",
                [&key.client_id]
            )
            .is_err()
        );
        assert!(
            conn.execute(
                "DELETE FROM machine_identities WHERE identity_id=?1",
                [&identity]
            )
            .is_err()
        );
        assert!(!crate::services::auth_service::verify_password(
            "!non-interactive",
            "!non-interactive"
        ));
    }

    #[test]
    fn direct_sql_machine_owner_promotion_is_rejected_but_human_transfer_is_legal() {
        let (pool, registry, auth, _) = setup();
        let key = create(&pool, &registry, &auth, 60);
        let conn = pool.get().unwrap();
        let machine_identity: String = conn
            .query_row(
                "SELECT identity_id FROM api_clients WHERE id=?1",
                [&key.client_id],
                |row| row.get(0),
            )
            .unwrap();

        // Direct role promotion and cross-tenant owner insertion are both
        // rejected by 066, independent of the HTTP transfer route.
        let update_error = conn
            .execute(
                "UPDATE tenant_memberships SET role='owner' WHERE identity_id=?1",
                [&machine_identity],
            )
            .unwrap_err();
        assert!(update_error.to_string().contains("MACHINE_AUTHORITY_GUARD"));
        let insert_error = conn
            .execute(
                "INSERT INTO tenant_memberships (id,identity_id,tenant_id,role,status,created_at,updated_at)
                 VALUES ('machine-owner-b',?1,'tenant-b','owner','active','now','now')",
                [&machine_identity],
            )
            .unwrap_err();
        assert!(insert_error.to_string().contains("MACHINE_AUTHORITY_GUARD"));

        conn.execute_batch(
            "INSERT INTO identities (id,username,password_hash,display_name,status,created_at,updated_at)
             VALUES ('human-staff','human-staff','hash','Human staff','active','now','now');
             INSERT INTO tenant_memberships (id,identity_id,tenant_id,role,status,created_at,updated_at)
             VALUES ('human-staff-member','human-staff','tenant-a','staff','active','now','now');",
        )
        .unwrap();
        conn.execute(
            "UPDATE tenant_memberships SET role='admin' WHERE id='member'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE tenant_memberships SET role='owner' WHERE id='human-staff-member'",
            [],
        )
        .unwrap();
    }

    #[test]
    fn direct_sql_machine_marker_for_existing_owner_is_rejected_and_not_persisted() {
        let (pool, _, _, _) = setup();
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "INSERT INTO identities (id,username,password_hash,display_name,status,created_at,updated_at)
             VALUES ('owner-before-machine','owner-before-machine','!non-interactive','Owner before machine','active','now','now');
             INSERT INTO tenant_memberships (id,identity_id,tenant_id,role,status,created_at,updated_at)
             VALUES ('owner-before-machine-membership','owner-before-machine','tenant-b','owner','active','now','now');",
        )
        .unwrap();

        let error = conn
            .execute(
                "INSERT INTO machine_identities (identity_id,created_at) VALUES ('owner-before-machine','now')",
                [],
            )
            .unwrap_err();
        assert!(error.to_string().contains("MACHINE_AUTHORITY_GUARD"));
        let marker_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM machine_identities WHERE identity_id='owner-before-machine'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(marker_count, 0);
    }

    #[test]
    fn migration_reapplication_and_audit_failure_are_fail_closed() {
        let (pool, registry, auth, calls) = setup();
        pool.get()
            .unwrap()
            .execute_batch(include_str!("../../db/migrations/066_r3_machine_api.sql"))
            .unwrap();
        let key = create(&pool, &registry, &auth, 60);
        pool.get().unwrap().execute_batch("CREATE TRIGGER break_audit BEFORE INSERT ON audit_events BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        assert!(execute(&registry, &pool, &key.secret, "tenant-a", "read").is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            pool.get()
                .unwrap()
                .query_row("SELECT count(*) FROM api_usage_windows", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn credential_lifecycle_is_independent_and_tenant_isolated() {
        let (pool, registry, auth, _) = setup();
        let key = create(&pool, &registry, &auth, 60);
        credential_lifecycle(&pool, &auth, &key.client_id, &key.credential_id, "disable").unwrap();
        assert!(execute(&registry, &pool, &key.secret, "tenant-a", "read").is_err());
        credential_lifecycle(&pool, &auth, &key.client_id, &key.credential_id, "enable").unwrap();
        execute(&registry, &pool, &key.secret, "tenant-a", "read").unwrap();
        credential_lifecycle(&pool, &auth, &key.client_id, &key.credential_id, "revoke").unwrap();
        assert!(
            credential_lifecycle(&pool, &auth, &key.client_id, &key.credential_id, "enable")
                .is_err()
        );
        assert!(execute(&registry, &pool, &key.secret, "tenant-a", "read").is_err());
        let mut other = auth.clone();
        other.authority = AuthorityContext::Tenant {
            tenant_id: TenantId::new("tenant-b").unwrap(),
            membership_id: TenantMembershipId::new("member").unwrap(),
            role: TenantRole::Owner,
        };
        assert!(list(&pool, &other).is_err());
        assert!(lifecycle(&pool, &other, &key.client_id, "revoke").is_err());
    }

    #[tokio::test]
    async fn http_bearer_path_is_reachable_cookie_and_plane_overrides_are_rejected() {
        use crate::application::{
            ApplicationServices, LegacyMaintenanceAdapter, MaintenanceWorkerRunner, SystemClock,
            WorkerContextFactory,
        };
        use axum::{
            body::{Body, to_bytes},
            extract::ConnectInfo,
            http::{Request, StatusCode},
        };
        use std::net::SocketAddr;
        use tower::ServiceExt;
        let (pool, registry, auth, calls) = setup();
        let key = create(&pool, &registry, &auth, 60);
        let registry = Arc::new(registry);
        let http: Arc<dyn system_core::transport::http_client::HttpClient> =
            Arc::new(NoopHttpClient);
        let worker = Arc::new(MaintenanceWorkerRunner::new(
            WorkerContextFactory::new(Arc::new(SystemClock), http.clone()),
            Arc::new(LegacyMaintenanceAdapter::new(pool.clone())),
        ));
        let services = Arc::new(ApplicationServices::production(
            registry.clone(),
            Arc::new(SystemClock),
            worker,
            Arc::new(crate::repositories::SqliteRepositoryProvider::new(
                pool.clone(),
            )),
        ));
        let state = Arc::new(crate::state::AppState::new_local(
            pool.clone(),
            crate::config::AppConfig::from_env(),
            registry,
            http,
            Arc::new(
                crate::integration::webhook::FixtureWebhookIngress::new_with_metrics(
                    crate::integration::store::IntegrationStore::new(pool),
                    Arc::new(crate::observability::RuntimeMetrics::default()),
                ),
            ),
            services,
        ));
        let app = crate::routes::machine_api::machine_api_routes()
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::middleware::csrf::csrf_check,
            ))
            .layer(axum::Extension(ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                443,
            )))))
            .with_state(state);
        for (extra, expected) in [
            (None, StatusCode::OK),
            (Some(("cookie", "session=ignored")), StatusCode::FORBIDDEN),
            (
                Some(("x-talos-authority", "platform")),
                StatusCode::FORBIDDEN,
            ),
            (
                Some(("x-talos-execution-mode", "simulation")),
                StatusCode::FORBIDDEN,
            ),
        ] {
            let mut req = Request::builder()
                .method("POST")
                .uri("/api/machine/v1/tenants/tenant-a/execute")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", key.secret));
            if let Some((k, v)) = extra {
                req = req.header(k, v);
            }
            let response = app
                .clone()
                .oneshot(
                    req.body(Body::from(
                        json!({"module":"probe","command":"read","payload":{}}).to_string(),
                    ))
                    .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            if expected == StatusCode::OK {
                assert_eq!(response.headers()["cache-control"], "no-store");
                assert!(response.headers().contains_key("x-correlation-id"));
            }
            let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains(&key.secret));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // Machine bearer does not authenticate the cookie-only provisioning path.
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/machine-clients")
                    .header("authorization", format!("Bearer {}", key.secret))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn machine_ingress_limiter_has_a_hard_bounded_peer_cache() {
        use std::net::{IpAddr, Ipv4Addr};

        let limiter = crate::routes::machine_api::MachineIngressRateLimiter::new(1, 2);
        assert!(limiter.allow(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!limiter.allow(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(limiter.allow(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2))));
        assert!(!limiter.allow(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 3))));
        assert_eq!(limiter.bucket_count(), 2);
    }

    #[tokio::test]
    async fn malformed_bearer_flood_is_limited_before_durable_authorization() {
        use crate::application::{
            ApplicationServices, LegacyMaintenanceAdapter, MaintenanceWorkerRunner, SystemClock,
            WorkerContextFactory,
        };
        use axum::{
            body::Body,
            extract::ConnectInfo,
            http::{Request, StatusCode},
        };
        use std::net::SocketAddr;
        use tower::ServiceExt;

        let (pool, registry, _auth, calls) = setup();
        let registry = Arc::new(registry);
        let http: Arc<dyn system_core::transport::http_client::HttpClient> =
            Arc::new(NoopHttpClient);
        let worker = Arc::new(MaintenanceWorkerRunner::new(
            WorkerContextFactory::new(Arc::new(SystemClock), http.clone()),
            Arc::new(LegacyMaintenanceAdapter::new(pool.clone())),
        ));
        let services = Arc::new(ApplicationServices::production(
            registry.clone(),
            Arc::new(SystemClock),
            worker,
            Arc::new(crate::repositories::SqliteRepositoryProvider::new(
                pool.clone(),
            )),
        ));
        let state = Arc::new(crate::state::AppState::new_local(
            pool.clone(),
            crate::config::AppConfig::from_env(),
            registry,
            http,
            Arc::new(
                crate::integration::webhook::FixtureWebhookIngress::new_with_metrics(
                    crate::integration::store::IntegrationStore::new(pool.clone()),
                    Arc::new(crate::observability::RuntimeMetrics::default()),
                ),
            ),
            services,
        ));
        let app = crate::routes::machine_api::machine_api_routes()
            .layer(axum::Extension(ConnectInfo(SocketAddr::from((
                [127, 0, 0, 9],
                443,
            )))))
            .with_state(state);

        let mut unauthorized = 0;
        let mut rate_limited = 0;
        for _ in 0..(crate::routes::machine_api::MACHINE_INGRESS_RATE_LIMIT + 2) {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/machine/v1/tenants/tenant-a/execute")
                        .header("content-type", "application/json")
                        .header("authorization", "Bearer malformed")
                        .body(Body::from(
                            json!({"module":"probe","command":"read","payload":{}}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            match response.status() {
                StatusCode::UNAUTHORIZED => unauthorized += 1,
                StatusCode::TOO_MANY_REQUESTS => rate_limited += 1,
                other => panic!("unexpected malformed bearer status: {other}"),
            }
        }
        assert_eq!(
            unauthorized,
            crate::routes::machine_api::MACHINE_INGRESS_RATE_LIMIT
        );
        assert_eq!(rate_limited, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let conn = pool.get().unwrap();
        let attempted_audits: i64 = conn
            .query_row(
                "SELECT count(*) FROM audit_events WHERE action='machine.access'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(attempted_audits, i64::from(unauthorized));
        let durable_windows: i64 = conn
            .query_row("SELECT count(*) FROM api_usage_windows", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(durable_windows, 0);
    }
}
