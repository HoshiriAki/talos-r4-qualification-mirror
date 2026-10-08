#![cfg(feature = "postgres")]

use std::future::Future;

use sqlx::postgres::PgPool;
use sqlx::{Postgres, Row, Transaction};
use tokio::runtime::{Handle, RuntimeFlavor};
use uuid::Uuid;

use super::RepositoryError;
use super::platform_tenant::{
    ClosePlatformTenant, CreatePlatformTenant, PlatformTenantAuditActor,
    PlatformTenantMutationError, PlatformTenantProjection, UpdatePlatformTenant,
    UpdatePlatformTenantStatus,
};

#[derive(Clone)]
pub(crate) struct PostgresPlatformTenantRepository {
    pool: PgPool,
}

impl PostgresPlatformTenantRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn list(&self) -> Result<Vec<PlatformTenantProjection>, RepositoryError> {
        let pool = self.pool.clone();
        run_pg_platform_tenant(async move {
            sqlx::query(
                "SELECT id, name, slug, status, plan, settings, created_at, updated_at
                 FROM tenants ORDER BY created_at DESC",
            )
            .fetch_all(&pool)
            .await
            .map_err(pg_repository)?
            .into_iter()
            .map(map_tenant_pg)
            .collect()
        })
    }

    pub(crate) fn get(
        &self,
        tenant_id: &str,
    ) -> Result<Option<PlatformTenantProjection>, RepositoryError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        run_pg_platform_tenant(async move {
            sqlx::query(
                "SELECT id, name, slug, status, plan, settings, created_at, updated_at
                 FROM tenants WHERE id = $1",
            )
            .bind(tenant_id)
            .fetch_optional(&pool)
            .await
            .map_err(pg_repository)?
            .map(map_tenant_pg)
            .transpose()
        })
    }

    pub(crate) fn create(
        &self,
        command: CreatePlatformTenant,
    ) -> Result<PlatformTenantProjection, PlatformTenantMutationError> {
        let pool = self.pool.clone();
        run_pg_platform_tenant_mutation(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;

            let slug_exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenants WHERE slug = $1)")
                    .bind(&command.slug)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(pg_storage)?;
            if slug_exists {
                return Err(PlatformTenantMutationError::SlugExists);
            }

            let owner_exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM identities WHERE id = $1 AND status = 'active')",
            )
            .bind(&command.owner_identity_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(pg_storage)?;
            if !owner_exists {
                return Err(PlatformTenantMutationError::OwnerIdentityNotFound);
            }

            let tenant_id = format!("tenant_{}", Uuid::new_v4());
            if let Err(error) = sqlx::query(
                "INSERT INTO tenants
                 (id, name, slug, status, plan, settings, created_at, updated_at)
                 VALUES ($1, $2, $3, 'active', 'free', $4, $5, $5)",
            )
            .bind(&tenant_id)
            .bind(&command.name)
            .bind(&command.slug)
            .bind(command.settings.as_deref())
            .bind(&command.now)
            .execute(&mut *tx)
            .await
            {
                if is_pg_unique_violation(&error) {
                    return Err(PlatformTenantMutationError::SlugExists);
                }
                return Err(pg_storage(error));
            }

            sqlx::query(
                "INSERT INTO tenant_memberships
                 (id, identity_id, tenant_id, role, status, created_at, updated_at)
                 VALUES ($1, $2, $3, 'owner', 'active', $4, $4)",
            )
            .bind(format!("tm_{}", Uuid::new_v4()))
            .bind(&command.owner_identity_id)
            .bind(&tenant_id)
            .bind(&command.now)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?;

            append_platform_tenant_audit_pg(
                &mut tx,
                &command.actor,
                Some(&tenant_id),
                "tenant.created",
                &tenant_id,
                &serde_json::json!({
                    "slug": command.slug,
                    "ownerIdentityId": command.owner_identity_id,
                }),
                &command.now,
            )
            .await?;

            let tenant = query_tenant_pg(&mut tx, &tenant_id)
                .await?
                .ok_or(PlatformTenantMutationError::TenantNotFound)?;
            tx.commit().await.map_err(pg_storage)?;
            Ok(tenant)
        })
    }

    pub(crate) fn update(
        &self,
        command: UpdatePlatformTenant,
    ) -> Result<PlatformTenantProjection, PlatformTenantMutationError> {
        let pool = self.pool.clone();
        run_pg_platform_tenant_mutation(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;

            if let Some(slug) = command.slug.as_deref() {
                let slug_exists: bool = sqlx::query_scalar(
                    "SELECT EXISTS(
                         SELECT 1 FROM tenants WHERE slug = $1 AND id <> $2
                     )",
                )
                .bind(slug)
                .bind(&command.tenant_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(pg_storage)?;
                if slug_exists {
                    return Err(PlatformTenantMutationError::SlugExists);
                }
            }

            let row = match sqlx::query(
                "UPDATE tenants
                 SET name = COALESCE($1, name),
                     slug = COALESCE($2, slug),
                     settings = COALESCE($3, settings),
                     updated_at = $4
                 WHERE id = $5
                 RETURNING id, name, slug, status, plan, settings, created_at, updated_at",
            )
            .bind(command.name.as_deref())
            .bind(command.slug.as_deref())
            .bind(command.settings.as_deref())
            .bind(&command.now)
            .bind(&command.tenant_id)
            .fetch_optional(&mut *tx)
            .await
            {
                Ok(row) => row,
                Err(error) if is_pg_unique_violation(&error) => {
                    return Err(PlatformTenantMutationError::SlugExists);
                }
                Err(error) => return Err(pg_storage(error)),
            }
            .ok_or(PlatformTenantMutationError::TenantNotFound)?;

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

            append_platform_tenant_audit_pg(
                &mut tx,
                &command.actor,
                Some(&command.tenant_id),
                "tenant.updated",
                &command.tenant_id,
                &serde_json::json!({ "fields": fields }),
                &command.now,
            )
            .await?;
            let tenant = map_tenant_pg(row).map_err(PlatformTenantMutationError::Storage)?;
            tx.commit().await.map_err(pg_storage)?;
            Ok(tenant)
        })
    }

    pub(crate) fn update_status(
        &self,
        command: UpdatePlatformTenantStatus,
    ) -> Result<PlatformTenantProjection, PlatformTenantMutationError> {
        let pool = self.pool.clone();
        run_pg_platform_tenant_mutation(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            let row = sqlx::query(
                "UPDATE tenants SET status = $1, updated_at = $2
                 WHERE id = $3 AND status != 'deleted'
                 RETURNING id, name, slug, status, plan, settings, created_at, updated_at",
            )
            .bind(&command.status)
            .bind(&command.now)
            .bind(&command.tenant_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(pg_storage)?
            .ok_or(PlatformTenantMutationError::TenantNotFound)?;

            append_platform_tenant_audit_pg(
                &mut tx,
                &command.actor,
                Some(&command.tenant_id),
                "tenant.status_changed",
                &command.tenant_id,
                &serde_json::json!({ "status": command.status }),
                &command.now,
            )
            .await?;
            let tenant = map_tenant_pg(row).map_err(PlatformTenantMutationError::Storage)?;
            tx.commit().await.map_err(pg_storage)?;
            Ok(tenant)
        })
    }

    pub(crate) fn close(
        &self,
        command: ClosePlatformTenant,
    ) -> Result<(), PlatformTenantMutationError> {
        let pool = self.pool.clone();
        run_pg_platform_tenant_mutation(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            let changed = sqlx::query(
                "UPDATE tenants SET status = 'deleted', updated_at = $1
                 WHERE id = $2 AND status != 'deleted'",
            )
            .bind(&command.now)
            .bind(&command.tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?
            .rows_affected();
            if changed == 0 {
                return Err(PlatformTenantMutationError::TenantNotFoundOrClosed);
            }

            append_platform_tenant_audit_pg(
                &mut tx,
                &command.actor,
                Some(&command.tenant_id),
                "tenant.closed",
                &command.tenant_id,
                &serde_json::json!({ "status": "deleted" }),
                &command.now,
            )
            .await?;
            tx.commit().await.map_err(pg_storage)?;
            Ok(())
        })
    }
}

async fn query_tenant_pg(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
) -> Result<Option<PlatformTenantProjection>, PlatformTenantMutationError> {
    sqlx::query(
        "SELECT id, name, slug, status, plan, settings, created_at, updated_at
         FROM tenants WHERE id = $1",
    )
    .bind(tenant_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(pg_storage)?
    .map(map_tenant_pg)
    .transpose()
    .map_err(PlatformTenantMutationError::Storage)
}

fn map_tenant_pg(row: sqlx::postgres::PgRow) -> Result<PlatformTenantProjection, RepositoryError> {
    Ok(PlatformTenantProjection {
        id: row.try_get(0).map_err(pg_decode)?,
        name: row.try_get(1).map_err(pg_decode)?,
        slug: row.try_get(2).map_err(pg_decode)?,
        status: row.try_get(3).map_err(pg_decode)?,
        plan: row.try_get(4).map_err(pg_decode)?,
        settings: row.try_get(5).map_err(pg_decode)?,
        created_at: row.try_get(6).map_err(pg_decode)?,
        updated_at: row.try_get(7).map_err(pg_decode)?,
    })
}

async fn append_platform_tenant_audit_pg(
    tx: &mut Transaction<'_, Postgres>,
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
    sqlx::query(
        "INSERT INTO audit_events
         (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
          platform_membership_id, roles_snapshot, capabilities_snapshot, action,
          resource_type, resource_id, correlation_id, detail_json, occurred_at)
         VALUES ($1, $2, 'platform', $3, NULL, $4, $5::jsonb, $6::jsonb, $7,
                 'tenant', $8, $1, $9::jsonb, $10)",
    )
    .bind(id)
    .bind(&actor.identity_id)
    .bind(tenant_id)
    .bind(&actor.membership_id)
    .bind(&actor.roles_snapshot)
    .bind(&actor.capabilities_snapshot)
    .bind(action)
    .bind(resource_id)
    .bind(detail_json)
    .bind(occurred_at)
    .execute(&mut **tx)
    .await
    .map_err(pg_storage)?;
    Ok(())
}

fn is_pg_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.is_unique_violation())
}

fn pg_repository(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

fn pg_storage(error: sqlx::Error) -> PlatformTenantMutationError {
    PlatformTenantMutationError::Storage(RepositoryError::Postgres(error.to_string()))
}

fn pg_decode<T>(error: T) -> RepositoryError
where
    T: std::fmt::Display,
{
    RepositoryError::Postgres(error.to_string())
}

fn run_pg_platform_tenant<T, F>(future: F) -> Result<T, RepositoryError>
where
    F: Future<Output = Result<T, RepositoryError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        RepositoryError::AdapterUnavailable("platform tenant runtime unavailable".into())
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(RepositoryError::AdapterUnavailable(
            "platform tenant requires the multi-thread runtime".into(),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

fn run_pg_platform_tenant_mutation<T, F>(future: F) -> Result<T, PlatformTenantMutationError>
where
    F: Future<Output = Result<T, PlatformTenantMutationError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        PlatformTenantMutationError::Storage(RepositoryError::AdapterUnavailable(
            "platform tenant runtime unavailable".into(),
        ))
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(PlatformTenantMutationError::Storage(
            RepositoryError::AdapterUnavailable(
                "platform tenant requires the multi-thread runtime".into(),
            ),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}
