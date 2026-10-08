use std::future::Future;

use sqlx::postgres::PgPool;
use system_core::audit::AuditEvent;
use system_core::{ALL_PLATFORM_CAPABILITIES, AuthorityContext, TenantId};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::audit_sink::AuditSink;

pub struct PostgresAuditSink {
    pool: PgPool,
}

impl PostgresAuditSink {
    pub fn new(pool: PgPool) -> Result<Self, String> {
        let probe = pool.clone();
        run_pg_audit(async move {
            let ready: bool = sqlx::query_scalar("SELECT to_regclass('audit_events') IS NOT NULL")
                .fetch_one(&probe)
                .await
                .map_err(|_| "audit storage readiness check failed".to_string())?;
            if !ready {
                return Err("audit storage is not ready".to_string());
            }
            Ok(())
        })?;
        Ok(Self { pool })
    }
}

impl AuditSink for PostgresAuditSink {
    fn append(&self, event: AuditEvent) -> Result<(), String> {
        let detail = serde_json::to_string(&event)
            .map_err(|_| "audit event serialization failed".to_string())?;
        let (authority_kind, tenant_membership_id, platform_membership_id, roles, capabilities) =
            match event.actor().authority() {
                Some(AuthorityContext::Tenant {
                    membership_id,
                    role,
                    ..
                }) => (
                    "tenant".to_string(),
                    Some(membership_id.as_str().to_string()),
                    None,
                    serde_json::to_string(&[role])
                        .map_err(|_| "audit roles serialization failed".to_string())?,
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
                        "platform".to_string(),
                        None,
                        Some(membership_id.as_str().to_string()),
                        serde_json::to_string(roles)
                            .map_err(|_| "audit roles serialization failed".to_string())?,
                        serde_json::to_string(&capabilities)
                            .map_err(|_| "audit capabilities serialization failed".to_string())?,
                    )
                }
                None => (
                    "system".to_string(),
                    None,
                    None,
                    "[]".to_string(),
                    "[]".to_string(),
                ),
            };

        let pool = self.pool.clone();
        let event_id = event.event_id().to_string();
        let actor_identity_id = event.actor().id().map(str::to_string);
        let tenant_id = event
            .data_scope()
            .tenant_id_opt()
            .map(TenantId::as_str)
            .map(str::to_string);
        let action = event.command().to_string();
        let resource_id = event.command().to_string();
        let correlation_id = event.correlation_id().as_str().to_string();
        let occurred_at = event.occurred_at().to_string();

        run_pg_audit(async move {
            sqlx::query(
                "INSERT INTO audit_events
                 (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
                  platform_membership_id, roles_snapshot, capabilities_snapshot, action,
                  resource_type, resource_id, correlation_id, detail_json, occurred_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8::jsonb, $9,
                         'command', $10, $11, $12::jsonb, $13)",
            )
            .bind(event_id)
            .bind(actor_identity_id)
            .bind(authority_kind)
            .bind(tenant_id)
            .bind(tenant_membership_id)
            .bind(platform_membership_id)
            .bind(roles)
            .bind(capabilities)
            .bind(action)
            .bind(resource_id)
            .bind(correlation_id)
            .bind(detail)
            .bind(occurred_at)
            .execute(&pool)
            .await
            .map_err(|_| "audit append failed".to_string())?;
            Ok(())
        })
    }
}

fn run_pg_audit<T, F>(future: F) -> Result<T, String>
where
    T: Send,
    F: Future<Output = Result<T, String>> + Send,
{
    let handle = Handle::try_current()
        .map_err(|_| "PostgreSQL audit sink requires an active Tokio runtime".to_string())?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err("PostgreSQL audit sink requires the multi-thread Tokio runtime".to_string());
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}
