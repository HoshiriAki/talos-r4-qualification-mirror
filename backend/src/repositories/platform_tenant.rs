use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use uuid::Uuid;

use super::RepositoryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlatformTenantProjection {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub status: String,
    pub plan: String,
    pub settings: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PlatformTenantAuditActor {
    pub identity_id: String,
    pub membership_id: String,
    pub roles_snapshot: String,
    pub capabilities_snapshot: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CreatePlatformTenant {
    pub name: String,
    pub slug: String,
    pub settings: Option<String>,
    pub owner_identity_id: String,
    pub actor: PlatformTenantAuditActor,
    pub now: String,
}

#[derive(Debug, Clone)]
pub(crate) struct UpdatePlatformTenant {
    pub tenant_id: String,
    pub name: Option<String>,
    pub slug: Option<String>,
    pub settings: Option<String>,
    pub actor: PlatformTenantAuditActor,
    pub now: String,
}

#[derive(Debug, Clone)]
pub(crate) struct UpdatePlatformTenantStatus {
    pub tenant_id: String,
    pub status: String,
    pub actor: PlatformTenantAuditActor,
    pub now: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ClosePlatformTenant {
    pub tenant_id: String,
    pub actor: PlatformTenantAuditActor,
    pub now: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PlatformTenantMutationError {
    #[error("tenant not found")]
    TenantNotFound,
    #[error("tenant not found or already closed")]
    TenantNotFoundOrClosed,
    #[error("tenant slug already exists")]
    SlugExists,
    #[error("tenant owner identity not found")]
    OwnerIdentityNotFound,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

#[derive(Clone)]
pub(crate) struct SqlitePlatformTenantRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqlitePlatformTenantRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn list(&self) -> Result<Vec<PlatformTenantProjection>, RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let mut statement = conn
            .prepare(
                "SELECT id, name, slug, status, plan, settings, created_at, updated_at
                 FROM tenants ORDER BY created_at DESC",
            )
            .map_err(sqlite_repository)?;
        statement
            .query_map([], map_tenant)
            .map_err(sqlite_repository)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_repository)
    }

    pub(crate) fn get(
        &self,
        tenant_id: &str,
    ) -> Result<Option<PlatformTenantProjection>, RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        conn.query_row(
            "SELECT id, name, slug, status, plan, settings, created_at, updated_at
             FROM tenants WHERE id = ?1",
            [tenant_id],
            map_tenant,
        )
        .optional()
        .map_err(sqlite_repository)
    }

    pub(crate) fn create(
        &self,
        command: CreatePlatformTenant,
    ) -> Result<PlatformTenantProjection, PlatformTenantMutationError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;

        let slug_exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM tenants WHERE slug = ?1)",
                [command.slug.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_storage)?;
        if slug_exists {
            return Err(PlatformTenantMutationError::SlugExists);
        }

        let owner_exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM identities WHERE id = ?1 AND status = 'active')",
                [command.owner_identity_id.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_storage)?;
        if !owner_exists {
            return Err(PlatformTenantMutationError::OwnerIdentityNotFound);
        }

        let tenant_id = format!("tenant_{}", Uuid::new_v4());
        if let Err(error) = tx.execute(
            "INSERT INTO tenants
             (id, name, slug, status, plan, settings, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'active', 'free', ?4, ?5, ?5)",
            params![
                tenant_id.as_str(),
                command.name.as_str(),
                command.slug.as_str(),
                command.settings.as_deref(),
                command.now.as_str(),
            ],
        ) {
            if is_sqlite_constraint(&error) {
                return Err(PlatformTenantMutationError::SlugExists);
            }
            return Err(sqlite_storage(error));
        }

        tx.execute(
            "INSERT INTO tenant_memberships
             (id, identity_id, tenant_id, role, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'owner', 'active', ?4, ?4)",
            params![
                format!("tm_{}", Uuid::new_v4()),
                command.owner_identity_id.as_str(),
                tenant_id.as_str(),
                command.now.as_str(),
            ],
        )
        .map_err(sqlite_storage)?;

        append_platform_tenant_audit_sqlite(
            &tx,
            &command.actor,
            Some(&tenant_id),
            "tenant.created",
            &tenant_id,
            &serde_json::json!({
                "slug": command.slug,
                "ownerIdentityId": command.owner_identity_id,
            }),
            &command.now,
        )?;

        let tenant = query_tenant_sqlite(&tx, &tenant_id)?
            .ok_or(PlatformTenantMutationError::TenantNotFound)?;
        tx.commit().map_err(sqlite_storage)?;
        Ok(tenant)
    }

    pub(crate) fn update(
        &self,
        command: UpdatePlatformTenant,
    ) -> Result<PlatformTenantProjection, PlatformTenantMutationError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;

        if let Some(slug) = command.slug.as_deref() {
            let slug_exists: bool = tx
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM tenants WHERE slug = ?1 AND id <> ?2
                     )",
                    params![slug, command.tenant_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(sqlite_storage)?;
            if slug_exists {
                return Err(PlatformTenantMutationError::SlugExists);
            }
        }

        let changed = tx
            .execute(
                "UPDATE tenants
                 SET name = COALESCE(?1, name),
                     slug = COALESCE(?2, slug),
                     settings = COALESCE(?3, settings),
                     updated_at = ?4
                 WHERE id = ?5",
                params![
                    command.name.as_deref(),
                    command.slug.as_deref(),
                    command.settings.as_deref(),
                    command.now.as_str(),
                    command.tenant_id.as_str(),
                ],
            )
            .map_err(|error| {
                if is_sqlite_constraint(&error) {
                    PlatformTenantMutationError::SlugExists
                } else {
                    sqlite_storage(error)
                }
            })?;
        if changed == 0 {
            return Err(PlatformTenantMutationError::TenantNotFound);
        }

        let mut fields = Vec::new();
        if command.name.is_some() {
            fields.push("name = ?");
        }
        if command.slug.is_some() {
            fields.push("slug = ?");
        }
        if command.settings.is_some() {
            fields.push("settings = ?");
        }
        fields.push("updated_at = ?");

        append_platform_tenant_audit_sqlite(
            &tx,
            &command.actor,
            Some(&command.tenant_id),
            "tenant.updated",
            &command.tenant_id,
            &serde_json::json!({ "fields": fields }),
            &command.now,
        )?;

        let tenant = query_tenant_sqlite(&tx, &command.tenant_id)?
            .ok_or(PlatformTenantMutationError::TenantNotFound)?;
        tx.commit().map_err(sqlite_storage)?;
        Ok(tenant)
    }

    pub(crate) fn update_status(
        &self,
        command: UpdatePlatformTenantStatus,
    ) -> Result<PlatformTenantProjection, PlatformTenantMutationError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;
        let changed = tx
            .execute(
                "UPDATE tenants SET status = ?1, updated_at = ?2
                 WHERE id = ?3 AND status != 'deleted'",
                params![
                    command.status.as_str(),
                    command.now.as_str(),
                    command.tenant_id.as_str(),
                ],
            )
            .map_err(sqlite_storage)?;
        if changed == 0 {
            return Err(PlatformTenantMutationError::TenantNotFound);
        }
        append_platform_tenant_audit_sqlite(
            &tx,
            &command.actor,
            Some(&command.tenant_id),
            "tenant.status_changed",
            &command.tenant_id,
            &serde_json::json!({ "status": command.status }),
            &command.now,
        )?;
        let tenant = query_tenant_sqlite(&tx, &command.tenant_id)?
            .ok_or(PlatformTenantMutationError::TenantNotFound)?;
        tx.commit().map_err(sqlite_storage)?;
        Ok(tenant)
    }

    pub(crate) fn close(
        &self,
        command: ClosePlatformTenant,
    ) -> Result<(), PlatformTenantMutationError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;
        let changed = tx
            .execute(
                "UPDATE tenants SET status = 'deleted', updated_at = ?1
                 WHERE id = ?2 AND status != 'deleted'",
                params![command.now.as_str(), command.tenant_id.as_str()],
            )
            .map_err(sqlite_storage)?;
        if changed == 0 {
            return Err(PlatformTenantMutationError::TenantNotFoundOrClosed);
        }
        append_platform_tenant_audit_sqlite(
            &tx,
            &command.actor,
            Some(&command.tenant_id),
            "tenant.closed",
            &command.tenant_id,
            &serde_json::json!({ "status": "deleted" }),
            &command.now,
        )?;
        tx.commit().map_err(sqlite_storage)?;
        Ok(())
    }
}

fn query_tenant_sqlite(
    tx: &Transaction<'_>,
    tenant_id: &str,
) -> Result<Option<PlatformTenantProjection>, PlatformTenantMutationError> {
    tx.query_row(
        "SELECT id, name, slug, status, plan, settings, created_at, updated_at
         FROM tenants WHERE id = ?1",
        [tenant_id],
        map_tenant,
    )
    .optional()
    .map_err(sqlite_storage)
}

fn map_tenant(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlatformTenantProjection> {
    Ok(PlatformTenantProjection {
        id: row.get(0)?,
        name: row.get(1)?,
        slug: row.get(2)?,
        status: row.get(3)?,
        plan: row.get(4)?,
        settings: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn append_platform_tenant_audit_sqlite(
    tx: &Transaction<'_>,
    actor: &PlatformTenantAuditActor,
    tenant_id: Option<&str>,
    action: &str,
    resource_id: &str,
    detail: &serde_json::Value,
    occurred_at: &str,
) -> Result<(), PlatformTenantMutationError> {
    let id = Uuid::new_v4().to_string();
    let detail_json = serde_json::to_string(detail).map_err(|error| {
        PlatformTenantMutationError::Storage(RepositoryError::ContractViolation(format!(
            "platform tenant audit serialization failed: {error}"
        )))
    })?;
    tx.execute(
        "INSERT INTO audit_events
         (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
          platform_membership_id, roles_snapshot, capabilities_snapshot, action,
          resource_type, resource_id, correlation_id, detail_json, occurred_at)
         VALUES (?1, ?2, 'platform', ?3, NULL, ?4, ?5, ?6, ?7,
                 'tenant', ?8, ?1, ?9, ?10)",
        params![
            id,
            actor.identity_id,
            tenant_id,
            actor.membership_id,
            actor.roles_snapshot,
            actor.capabilities_snapshot,
            action,
            resource_id,
            detail_json,
            occurred_at,
        ],
    )
    .map_err(sqlite_storage)?;
    Ok(())
}

fn is_sqlite_constraint(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

fn sqlite_repository(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn sqlite_storage(error: rusqlite::Error) -> PlatformTenantMutationError {
    PlatformTenantMutationError::Storage(RepositoryError::Sqlite(error.to_string()))
}
