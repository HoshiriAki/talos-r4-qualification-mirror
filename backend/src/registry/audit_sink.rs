use std::sync::Mutex;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use system_core::audit::AuditEvent;
use system_core::{ALL_PLATFORM_CAPABILITIES, AuthorityContext, TenantId};

/// Append-only boundary for execution audit records. Implementations must not
/// expose mutation or deletion of previously accepted events.
pub trait AuditSink: Send + Sync {
    fn append(&self, event: AuditEvent) -> Result<(), String>;
}

/// Production audit boundary backed by the tenant-scoped audit log table.
/// It owns no unbounded in-memory event buffer and survives process restarts.
pub struct SqliteAuditSink {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteAuditSink {
    pub fn new(pool: Pool<SqliteConnectionManager>) -> Result<Self, String> {
        let conn = pool
            .get()
            .map_err(|error| format!("audit pool unavailable: {error}"))?;
        let audit_table_exists: i64 = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'audit_events')",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("audit storage is not ready: {error}"))?;
        if audit_table_exists != 1 {
            return Err("audit storage is not ready: audit_events table is missing".into());
        }
        Ok(Self { pool })
    }
}

impl AuditSink for SqliteAuditSink {
    fn append(&self, event: AuditEvent) -> Result<(), String> {
        let conn = self
            .pool
            .get()
            .map_err(|error| format!("audit pool unavailable: {error}"))?;
        let detail = serde_json::to_string(&event)
            .map_err(|error| format!("audit event serialization failed: {error}"))?;
        let (authority_kind, tenant_membership_id, platform_membership_id, roles, capabilities) =
            match event.actor().authority() {
                Some(AuthorityContext::Tenant {
                    membership_id,
                    role,
                    ..
                }) => (
                    "tenant",
                    Some(membership_id.as_str()),
                    None,
                    serde_json::to_string(&[role])
                        .map_err(|error| format!("audit roles serialization failed: {error}"))?,
                    "[]".to_string(),
                ),
                Some(
                    authority @ AuthorityContext::Platform {
                        membership_id,
                        roles,
                    },
                ) => {
                    let capabilities: Vec<_> = ALL_PLATFORM_CAPABILITIES
                        .iter()
                        .copied()
                        .filter(|capability| authority.allows(*capability))
                        .collect();
                    (
                        "platform",
                        None,
                        Some(membership_id.as_str()),
                        serde_json::to_string(roles).map_err(|error| {
                            format!("audit roles serialization failed: {error}")
                        })?,
                        serde_json::to_string(&capabilities).map_err(|error| {
                            format!("audit capabilities serialization failed: {error}")
                        })?,
                    )
                }
                None => ("system", None, None, "[]".to_string(), "[]".to_string()),
            };
        conn.execute(
            "INSERT INTO audit_events
             (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
              platform_membership_id, roles_snapshot, capabilities_snapshot, action,
              resource_type, resource_id, correlation_id, detail_json, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                event.event_id(),
                event.actor().id(),
                authority_kind,
                event.data_scope().tenant_id_opt().map(TenantId::as_str),
                tenant_membership_id,
                platform_membership_id,
                roles,
                capabilities,
                event.command(),
                "command",
                event.command(),
                event.correlation_id().as_str(),
                detail,
                event.occurred_at(),
            ],
        )
        .map_err(|error| format!("audit insert failed: {error}"))?;
        Ok(())
    }
}

/// Process-local append-only sink for deterministic tests. Production assembly
/// uses `SqliteAuditSink` and must not use this volatile implementation.
#[derive(Default)]
pub struct InMemoryAuditSink {
    events: Mutex<Vec<AuditEvent>>,
}

impl InMemoryAuditSink {
    pub fn events(&self) -> Result<Vec<AuditEvent>, String> {
        self.events
            .lock()
            .map(|events| events.clone())
            .map_err(|_| "audit sink lock poisoned".to_string())
    }
}

impl AuditSink for InMemoryAuditSink {
    fn append(&self, event: AuditEvent) -> Result<(), String> {
        self.events
            .lock()
            .map_err(|_| "audit sink lock poisoned".to_string())?
            .push(event);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use rusqlite::params;
    use serde_json::json;
    use system_core::audit::{AuditEvent, AuditResult};
    use system_core::{
        ActorIdentity, AuditPayloadPolicy, AuthorityContext, DataScope, ExecutionContext,
        ExecutionMode, Namespace, NoopHttpClient, RequestId, Revision, TenantId,
        TenantMembershipId, TenantRole, TenantScope,
    };

    use super::{AuditSink, InMemoryAuditSink, SqliteAuditSink};

    fn sqlite_audit_pool() -> Pool<SqliteConnectionManager> {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        pool.get()
            .unwrap()
            .execute_batch(
                "CREATE TABLE audit_events (
                id TEXT PRIMARY KEY,
                actor_identity_id TEXT,
                authority_kind TEXT NOT NULL,
                tenant_id TEXT,
                tenant_membership_id TEXT,
                platform_membership_id TEXT,
                roles_snapshot TEXT NOT NULL,
                capabilities_snapshot TEXT NOT NULL,
                action TEXT NOT NULL,
                resource_type TEXT NOT NULL,
                resource_id TEXT,
                correlation_id TEXT NOT NULL,
                detail_json TEXT NOT NULL,
                occurred_at TEXT NOT NULL
            );",
            )
            .unwrap();
        pool
    }

    fn execution_context(tenant_id: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant_id).unwrap();
        let data_scope = DataScope::new(
            tenant_id.clone(),
            Namespace::production(),
            Revision::new("revision-audit-sqlite").unwrap(),
        )
        .unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                "tenant-owner-audit",
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new("membership-audit").unwrap(),
                    tenant_id: tenant_id.clone(),
                    role: TenantRole::Owner,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant_id),
            data_scope,
            ExecutionMode::Normal,
            RequestId::new("request-audit-sqlite").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn in_memory_sink_appends_events_without_replacing_prior_entries() {
        let sink = InMemoryAuditSink::default();
        let ctx = execution_context("tenant-memory");
        let first = AuditEvent::from_execution(
            "audit-1",
            &ctx,
            "device.list",
            AuditResult::Succeeded,
            "2026-07-16T12:00:00+08:00",
            AuditPayloadPolicy::ReferenceOnly,
            json!({}),
        )
        .unwrap();
        let second = AuditEvent::from_execution(
            "audit-2",
            &ctx,
            "device.get",
            AuditResult::Succeeded,
            "2026-07-16T12:01:00+08:00",
            AuditPayloadPolicy::ReferenceOnly,
            json!({}),
        )
        .unwrap();

        sink.append(first).unwrap();
        sink.append(second).unwrap();

        let events = sink.events().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_id(), "audit-1");
        assert_eq!(events[1].event_id(), "audit-2");
    }

    #[test]
    fn sqlite_sink_persists_tenant_scope_and_reference_only_payload() {
        let pool = sqlite_audit_pool();
        let sink = SqliteAuditSink::new(pool.clone()).unwrap();
        let event = AuditEvent::from_execution(
            "audit-sqlite-1",
            &execution_context("tenant-acme"),
            "device.list",
            AuditResult::Succeeded,
            "2026-07-16T12:00:00+08:00",
            AuditPayloadPolicy::ReferenceOnly,
            json!({ "module": "device", "command": "list" }),
        )
        .unwrap();

        sink.append(event).unwrap();

        let conn = pool.get().unwrap();
        let (tenant_id, detail): (String, String) = conn
            .query_row(
                "SELECT tenant_id, detail_json FROM audit_events WHERE id = ?1",
                params!["audit-sqlite-1"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let detail: serde_json::Value = serde_json::from_str(&detail).unwrap();

        assert_eq!(tenant_id, "tenant-acme");
        assert_eq!(detail["payload_policy"], "ReferenceOnly");
        assert_eq!(
            detail["payload"],
            json!({ "module": "device", "command": "list" })
        );
        assert_eq!(detail["data_scope"]["target"]["Tenant"], "tenant-acme");
    }
}
