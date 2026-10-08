use std::collections::HashMap;
use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_core::audit::AuditResult;
use system_core::{
    ActorIdentity, AuditPayloadPolicy, AuthorityContext, DataScope, ExecutionContext,
    ExecutionMode, Namespace, NoopHttpClient, PlatformMembershipId, PlatformRole, PreviewSessionId,
    RequestId, Revision, SimulationId, SystemModule, TenantId, TenantMembershipId, TenantRole,
    TenantScope,
};

use crate::registry::ModuleRegistry;
use crate::registry::audit_sink::InMemoryAuditSink;
use crate::repositories::{RepositoryProvider, SqliteRepositoryProvider};

use super::CustomerModule;

fn pool() -> Pool<SqliteConnectionManager> {
    let pool = Pool::builder()
        .max_size(1)
        .build(SqliteConnectionManager::memory())
        .unwrap();
    let connection = pool.get().unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE customers (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL,
               legal_name TEXT NOT NULL, display_name TEXT NOT NULL,
               status TEXT NOT NULL, risk_status TEXT NOT NULL,
               version INTEGER NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
               UNIQUE(id, tenant_id)
             );
             CREATE TABLE customer_contacts (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, customer_id TEXT NOT NULL,
               kind TEXT NOT NULL, raw_value TEXT NOT NULL, normalized_value TEXT NOT NULL,
               is_primary INTEGER NOT NULL, classification TEXT NOT NULL, purpose TEXT NOT NULL,
               created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
               UNIQUE(tenant_id, customer_id, kind, normalized_value)
             );
             CREATE TABLE customer_external_identities (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, customer_id TEXT NOT NULL,
               provider TEXT NOT NULL, external_subject TEXT NOT NULL,
               created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
               UNIQUE(tenant_id, provider, external_subject)
             );
             CREATE TABLE customer_history (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, customer_id TEXT NOT NULL,
               event_type TEXT NOT NULL, detail_json TEXT NOT NULL,
               actor_identity_id TEXT, created_at TEXT NOT NULL
             );
             CREATE TABLE customer_migration_exceptions (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, source_table TEXT NOT NULL,
               source_row_id TEXT NOT NULL, reason TEXT NOT NULL, payload_json TEXT NOT NULL,
               status TEXT NOT NULL, resolved_customer_id TEXT, created_at TEXT NOT NULL,
               resolved_at TEXT, UNIQUE(tenant_id, source_table, source_row_id)
             );
             CREATE TABLE orders (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, customer_id TEXT, orderNo TEXT NOT NULL
             );
             CREATE TABLE blacklist (id INTEGER PRIMARY KEY, tenant_id TEXT NOT NULL, customer_id TEXT);
             CREATE TABLE violations (id INTEGER PRIMARY KEY, tenant_id TEXT NOT NULL, customer_id TEXT);
             CREATE TABLE credit_scores (id INTEGER PRIMARY KEY, tenant_id TEXT NOT NULL, customer_id TEXT);
             CREATE TABLE overdue_records (id INTEGER PRIMARY KEY, tenant_id TEXT NOT NULL, customer_id TEXT);
             CREATE TABLE contracts (id INTEGER PRIMARY KEY, tenant_id TEXT NOT NULL, customer_id TEXT);",
        )
        .unwrap();
    drop(connection);
    pool
}

fn tenant_context(tenant: &str, role: TenantRole, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            format!("actor-{tenant}"),
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new(format!("membership-{tenant}")).unwrap(),
                tenant_id: tenant_id.clone(),
                role,
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("production-current").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn preview_context(tenant: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "platform-owner",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("preview-revision").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("preview-customer").unwrap()),
        RequestId::new("request-preview").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn simulation_context(tenant: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    let simulation_id = SimulationId::new("simulation-customer").unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "platform-owner",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::new(
            tenant_id,
            Namespace::Simulation(simulation_id.clone()),
            Revision::new("simulation-revision").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Simulation(simulation_id),
        RequestId::new("request-simulation").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn registry(pool: Pool<SqliteConnectionManager>) -> (ModuleRegistry, Arc<InMemoryAuditSink>) {
    let provider: Arc<dyn RepositoryProvider> = Arc::new(SqliteRepositoryProvider::new(pool));
    let module: Arc<dyn SystemModule> = Arc::new(CustomerModule::new(provider));
    let sink = Arc::new(InMemoryAuditSink::default());
    (
        ModuleRegistry::new_with_audit_sink(
            HashMap::from([("customer".into(), module)]),
            sink.clone(),
        )
        .unwrap(),
        sink,
    )
}

fn create_customer(
    registry: &ModuleRegistry,
    ctx: &ExecutionContext,
    name: &str,
) -> serde_json::Value {
    registry
        .execute(
            "customer",
            "create_customer",
            serde_json::json!({
                "legalName": name,
                "contacts": [{
                    "kind": "phone",
                    "value": "+86 138-0013-8000",
                    "isPrimary": true
                }]
            }),
            ctx,
        )
        .unwrap()
}

#[test]
fn customer_is_tenant_scoped_and_pii_is_masked() {
    let pool = pool();
    let (registry, _) = registry(pool);
    let tenant_a = tenant_context("tenant-a", TenantRole::Staff, "request-a");
    let tenant_b = tenant_context("tenant-b", TenantRole::Staff, "request-b");

    let created_a = create_customer(&registry, &tenant_a, "Alice");
    assert_eq!(
        created_a["customer"]["contacts"][0]["maskedValue"],
        "***8000"
    );
    assert!(created_a.to_string().find("13800138000").is_none());
    assert_eq!(
        created_a["duplicateCandidates"].as_array().unwrap().len(),
        0
    );

    let duplicate_a = registry
        .execute(
            "customer",
            "find_duplicate_candidates",
            serde_json::json!({"kind": "phone", "value": "+86 138 0013 8000"}),
            &tenant_a,
        )
        .unwrap();
    assert_eq!(duplicate_a.as_array().unwrap().len(), 1);
    assert_eq!(duplicate_a[0]["matchedContact"], "***8000");

    let duplicate_b = registry
        .execute(
            "customer",
            "find_duplicate_candidates",
            serde_json::json!({"kind": "phone", "value": "+86 13800138000"}),
            &tenant_b,
        )
        .unwrap();
    assert_eq!(duplicate_b.as_array().unwrap().len(), 0);

    let list_b = registry
        .execute(
            "customer",
            "list_customers",
            serde_json::json!({"limit": 20}),
            &tenant_b,
        )
        .unwrap();
    assert_eq!(list_b.as_array().unwrap().len(), 0);
}

#[test]
fn preview_is_read_only_and_simulation_is_fail_closed() {
    let pool = pool();
    let (registry, sink) = registry(pool);
    let normal = tenant_context("tenant-a", TenantRole::Staff, "request-normal");
    create_customer(&registry, &normal, "Alice");

    let preview = preview_context("tenant-a");
    let list = registry
        .execute(
            "customer",
            "list_customers",
            serde_json::json!({"limit": 20}),
            &preview,
        )
        .unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    let preview_write = registry.execute(
        "customer",
        "create_customer",
        serde_json::json!({"legalName": "Preview Write"}),
        &preview,
    );
    assert!(preview_write.is_err());

    let simulation = simulation_context("tenant-a");
    assert!(
        registry
            .execute(
                "customer",
                "list_customers",
                serde_json::json!({"limit": 20}),
                &simulation,
            )
            .is_err()
    );

    let events = sink.events().unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.result() == &AuditResult::Attempted)
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event.result(), AuditResult::Failed { .. }))
    );
    assert!(
        events
            .iter()
            .all(|event| event.payload_policy() == AuditPayloadPolicy::ReferenceOnly)
    );
}

#[test]
fn migration_exception_resolution_is_tenant_bound_and_admin_only() {
    let pool = pool();
    {
        let connection = pool.get().unwrap();
        connection
            .execute(
                "INSERT INTO orders (id, tenant_id, orderNo) VALUES ('order-a', 'tenant-a', '1001')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO customer_migration_exceptions
                 (id, tenant_id, source_table, source_row_id, reason, payload_json, status, created_at)
                 VALUES ('exception-a', 'tenant-a', 'orders', 'order-a', 'legacy', '{}', 'pending', 'now')",
                [],
            )
            .unwrap();
    }
    let (registry, _) = registry(pool.clone());
    let tenant_a_admin = tenant_context("tenant-a", TenantRole::Admin, "request-admin-a");
    let tenant_b_admin = tenant_context("tenant-b", TenantRole::Admin, "request-admin-b");
    let tenant_a_staff = tenant_context("tenant-a", TenantRole::Staff, "request-staff-a");
    let created = create_customer(&registry, &tenant_a_admin, "Alice");
    let customer_id = created["customer"]["id"].as_str().unwrap();

    assert!(
        registry
            .execute(
                "customer",
                "resolve_migration_exception",
                serde_json::json!({"exceptionId": "exception-a", "customerId": customer_id}),
                &tenant_a_staff,
            )
            .is_err()
    );
    assert!(
        registry
            .execute(
                "customer",
                "resolve_migration_exception",
                serde_json::json!({"exceptionId": "exception-a", "customerId": customer_id}),
                &tenant_b_admin,
            )
            .is_err()
    );

    registry
        .execute(
            "customer",
            "resolve_migration_exception",
            serde_json::json!({"exceptionId": "exception-a", "customerId": customer_id}),
            &tenant_a_admin,
        )
        .unwrap();

    let connection = pool.get().unwrap();
    let associated: String = connection
        .query_row(
            "SELECT customer_id FROM orders WHERE tenant_id = 'tenant-a' AND id = 'order-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(associated, customer_id);
    let status: String = connection
        .query_row(
            "SELECT status FROM customer_migration_exceptions WHERE id = 'exception-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "resolved");
}

#[test]
fn anonymization_removes_contacts_but_preserves_customer_identity() {
    let pool = pool();
    let (registry, _) = registry(pool);
    let admin = tenant_context("tenant-a", TenantRole::Admin, "request-anonymize");
    let created = create_customer(&registry, &admin, "Alice");
    let customer_id = created["customer"]["id"].as_str().unwrap();

    let anonymized = registry
        .execute(
            "customer",
            "anonymize_customer",
            serde_json::json!({"customerId": customer_id}),
            &admin,
        )
        .unwrap();
    assert_eq!(anonymized["id"], customer_id);
    assert_eq!(anonymized["status"], "anonymized");
    assert_eq!(anonymized["contacts"].as_array().unwrap().len(), 0);
    assert!(
        anonymized["displayName"]
            .as_str()
            .unwrap()
            .starts_with("ANON-")
    );
}
