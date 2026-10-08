#[cfg(feature = "postgres")]
use std::future::Future;

use sqlx::postgres::PgPool;
use sqlx::{Postgres, Row, Transaction};
use tokio::runtime::{Handle, RuntimeFlavor};
use uuid::Uuid;

use super::RepositoryError;
use super::platform_membership::{
    CreatePlatformMembership, CreatedPlatformMembership, PlatformMembershipAuditActor,
    PlatformMembershipMutationError, PlatformMembershipProjection, RevokePlatformMembership,
    UpdatePlatformMembership,
};

#[derive(Clone)]
pub(crate) struct PostgresPlatformMembershipRepository {
    pool: PgPool,
}

impl PostgresPlatformMembershipRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn list(&self) -> Result<Vec<PlatformMembershipProjection>, RepositoryError> {
        let pool = self.pool.clone();
        run_pg_platform_membership(async move {
            let rows = sqlx::query(
                "SELECT pm.id, pm.identity_id, i.username, i.display_name, COALESCE(i.email, ''),
                        pm.status, pm.created_at, pm.updated_at
                 FROM platform_memberships pm
                 JOIN identities i ON i.id = pm.identity_id
                 ORDER BY pm.created_at DESC",
            )
            .fetch_all(&pool)
            .await
            .map_err(|error| RepositoryError::Postgres(error.to_string()))?;

            let mut memberships = Vec::with_capacity(rows.len());
            for row in rows {
                let id: String = row.try_get(0).map_err(pg_decode)?;
                let role_rows = sqlx::query(
                    "SELECT role FROM platform_role_grants
                     WHERE platform_membership_id = $1 ORDER BY role",
                )
                .bind(&id)
                .fetch_all(&pool)
                .await
                .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
                let roles = role_rows
                    .into_iter()
                    .map(|row| row.try_get(0).map_err(pg_decode))
                    .collect::<Result<Vec<String>, RepositoryError>>()?;
                memberships.push(PlatformMembershipProjection {
                    id,
                    identity_id: row.try_get(1).map_err(pg_decode)?,
                    username: row.try_get(2).map_err(pg_decode)?,
                    display_name: row.try_get(3).map_err(pg_decode)?,
                    email: row.try_get(4).map_err(pg_decode)?,
                    status: row.try_get(5).map_err(pg_decode)?,
                    roles,
                    created_at: row.try_get(6).map_err(pg_decode)?,
                    updated_at: row.try_get(7).map_err(pg_decode)?,
                });
            }
            Ok(memberships)
        })
    }

    pub(crate) fn create(
        &self,
        command: CreatePlatformMembership,
    ) -> Result<CreatedPlatformMembership, PlatformMembershipMutationError> {
        if command.roles.iter().any(|role| role == "platform_owner")
            && !command.actor_is_platform_owner
        {
            return Err(PlatformMembershipMutationError::OwnerAuthorityRequired);
        }
        let pool = self.pool.clone();
        run_pg_platform_membership_mutation(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            begin_platform_membership_mutation_pg(&mut tx).await?;

            if command.roles.iter().any(|role| role == "platform_owner")
                && !actor_has_active_owner_pg(&mut tx, &command.actor).await?
            {
                return Err(PlatformMembershipMutationError::OwnerAuthorityRequired);
            }

            let identity_id = if let Some(identity_id) = command.existing_identity_id.as_deref() {
                let exists: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM identities WHERE id = $1 AND status = 'active')",
                )
                .bind(identity_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(pg_storage)?;
                if !exists {
                    return Err(PlatformMembershipMutationError::IdentityNotFound);
                }
                identity_id.to_string()
            } else {
                let identity = command.new_identity.as_ref().ok_or_else(|| {
                    PlatformMembershipMutationError::Storage(RepositoryError::ContractViolation(
                        "new platform identity payload required".into(),
                    ))
                })?;
                let identity_id = Uuid::new_v4().to_string();
                if let Err(error) = sqlx::query(
                    "INSERT INTO identities
                     (id, username, email, password_hash, display_name, phone, status, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, 'active', $7, $7)",
                )
                .bind(&identity_id)
                .bind(identity.username.as_str())
                .bind(identity.email.as_deref())
                .bind(identity.password_hash.as_str())
                .bind(identity.display_name.as_str())
                .bind(identity.phone.as_deref())
                .bind(&command.now)
                .execute(&mut *tx)
                .await
                {
                    if is_pg_unique_violation(&error) {
                        return Err(PlatformMembershipMutationError::DuplicateIdentity);
                    }
                    return Err(pg_storage(error));
                }
                identity_id
            };

            let existing: Option<String> =
                sqlx::query_scalar("SELECT id FROM platform_memberships WHERE identity_id = $1")
                    .bind(&identity_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(pg_storage)?;
            if existing.is_some() {
                return Err(PlatformMembershipMutationError::MembershipExists);
            }

            let membership_id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO platform_memberships
                 (id, identity_id, status, created_at, updated_at)
                 VALUES ($1, $2, 'active', $3, $3)",
            )
            .bind(&membership_id)
            .bind(&identity_id)
            .bind(&command.now)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?;
            replace_roles_pg(
                &mut tx,
                &membership_id,
                &command.roles,
                &command.actor.identity_id,
                &command.now,
                false,
            )
            .await?;
            append_platform_audit_pg(
                &mut tx,
                &command.actor,
                "platform_membership.created",
                &membership_id,
                &serde_json::json!({
                    "identityId": identity_id,
                    "roles": command.roles,
                }),
                &command.now,
            )
            .await?;
            tx.commit().await.map_err(pg_storage)?;

            Ok(CreatedPlatformMembership {
                membership_id,
                identity_id,
            })
        })
    }

    pub(crate) fn update(
        &self,
        command: UpdatePlatformMembership,
    ) -> Result<(), PlatformMembershipMutationError> {
        let pool = self.pool.clone();
        run_pg_platform_membership_mutation(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            begin_platform_membership_mutation_pg(&mut tx).await?;

            let owner_grant = membership_has_owner_grant_pg(&mut tx, &command.membership_id)
                .await?
                .ok_or(PlatformMembershipMutationError::MembershipNotFound)?;
            let changes_owner_grant = owner_grant
                || command
                    .roles
                    .as_ref()
                    .is_some_and(|roles| roles.iter().any(|role| role == "platform_owner"));
            if changes_owner_grant
                && (!command.actor_is_platform_owner
                    || !actor_has_active_owner_pg(&mut tx, &command.actor).await?)
            {
                return Err(PlatformMembershipMutationError::OwnerAuthorityRequired);
            }

            if let Some(status) = command.status.as_deref() {
                sqlx::query(
                    "UPDATE platform_memberships SET status = $1, updated_at = $2 WHERE id = $3",
                )
                .bind(status)
                .bind(&command.now)
                .bind(&command.membership_id)
                .execute(&mut *tx)
                .await
                .map_err(pg_storage)?;
            }
            if let Some(roles) = command.roles.as_ref() {
                replace_roles_pg(
                    &mut tx,
                    &command.membership_id,
                    roles,
                    &command.actor.identity_id,
                    &command.now,
                    true,
                )
                .await?;
            }
            ensure_active_owner_pg(&mut tx).await?;
            append_platform_audit_pg(
                &mut tx,
                &command.actor,
                "platform_membership.updated",
                &command.membership_id,
                &serde_json::json!({
                    "status": command.status,
                    "roles": command.roles,
                }),
                &command.now,
            )
            .await?;
            tx.commit().await.map_err(pg_storage)?;
            Ok(())
        })
    }

    pub(crate) fn revoke(
        &self,
        command: RevokePlatformMembership,
    ) -> Result<(), PlatformMembershipMutationError> {
        if command.actor.membership_id == command.membership_id {
            return Err(PlatformMembershipMutationError::SelfRevoke);
        }
        let pool = self.pool.clone();
        run_pg_platform_membership_mutation(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            begin_platform_membership_mutation_pg(&mut tx).await?;

            let owner_grant = membership_has_owner_grant_pg(&mut tx, &command.membership_id)
                .await?
                .ok_or(PlatformMembershipMutationError::MembershipNotFound)?;
            if owner_grant
                && (!command.actor_is_platform_owner
                    || !actor_has_active_owner_pg(&mut tx, &command.actor).await?)
            {
                return Err(PlatformMembershipMutationError::OwnerAuthorityRequired);
            }

            let changed = sqlx::query(
                "UPDATE platform_memberships SET status = 'revoked', updated_at = $1
                 WHERE id = $2 AND status != 'revoked'",
            )
            .bind(&command.now)
            .bind(&command.membership_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?
            .rows_affected();
            if changed == 0 {
                return Err(PlatformMembershipMutationError::MembershipNotFound);
            }
            ensure_active_owner_pg(&mut tx).await?;
            append_platform_audit_pg(
                &mut tx,
                &command.actor,
                "platform_membership.revoked",
                &command.membership_id,
                &serde_json::json!({ "status": "revoked" }),
                &command.now,
            )
            .await?;
            tx.commit().await.map_err(pg_storage)?;
            Ok(())
        })
    }
}

async fn begin_platform_membership_mutation_pg(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), PlatformMembershipMutationError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut **tx)
        .await
        .map_err(pg_storage)?;
    // Platform governance is low-volume. Serializing membership/role writers
    // makes the last-active-owner invariant explicit rather than relying on two
    // concurrent transactions to discover a serialization anomaly after the fact.
    sqlx::query(
        "LOCK TABLE platform_memberships, platform_role_grants IN SHARE ROW EXCLUSIVE MODE",
    )
    .execute(&mut **tx)
    .await
    .map_err(pg_storage)?;
    Ok(())
}

async fn actor_has_active_owner_pg(
    tx: &mut Transaction<'_, Postgres>,
    actor: &PlatformMembershipAuditActor,
) -> Result<bool, PlatformMembershipMutationError> {
    sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1
             FROM platform_memberships pm
             JOIN platform_role_grants prg ON prg.platform_membership_id = pm.id
             WHERE pm.id = $1
               AND pm.identity_id = $2
               AND pm.status = 'active'
               AND prg.role = 'platform_owner'
         )",
    )
    .bind(&actor.membership_id)
    .bind(&actor.identity_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(pg_storage)
}

async fn membership_has_owner_grant_pg(
    tx: &mut Transaction<'_, Postgres>,
    membership_id: &str,
) -> Result<Option<bool>, PlatformMembershipMutationError> {
    sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM platform_role_grants
             WHERE platform_membership_id = $1 AND role = 'platform_owner'
         )
         FROM platform_memberships WHERE id = $1",
    )
    .bind(membership_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(pg_storage)
}

async fn replace_roles_pg(
    tx: &mut Transaction<'_, Postgres>,
    membership_id: &str,
    roles: &[String],
    granted_by_identity_id: &str,
    now: &str,
    delete_existing: bool,
) -> Result<(), PlatformMembershipMutationError> {
    if delete_existing {
        sqlx::query("DELETE FROM platform_role_grants WHERE platform_membership_id = $1")
            .bind(membership_id)
            .execute(&mut **tx)
            .await
            .map_err(pg_storage)?;
    }
    for role in roles {
        sqlx::query(
            "INSERT INTO platform_role_grants
             (id, platform_membership_id, role, granted_by_identity_id, granted_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(membership_id)
        .bind(role)
        .bind(granted_by_identity_id)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(pg_storage)?;
    }
    Ok(())
}

async fn ensure_active_owner_pg(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), PlatformMembershipMutationError> {
    let owner_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM platform_memberships pm
         JOIN platform_role_grants prg ON prg.platform_membership_id = pm.id
         WHERE pm.status = 'active' AND prg.role = 'platform_owner'",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(pg_storage)?;
    if owner_count == 0 {
        Err(PlatformMembershipMutationError::LastActiveOwner)
    } else {
        Ok(())
    }
}

async fn append_platform_audit_pg(
    tx: &mut Transaction<'_, Postgres>,
    actor: &PlatformMembershipAuditActor,
    action: &str,
    resource_id: &str,
    detail: &serde_json::Value,
    occurred_at: &str,
) -> Result<(), PlatformMembershipMutationError> {
    let id = Uuid::new_v4().to_string();
    let detail_json = serde_json::to_string(detail).map_err(|error| {
        PlatformMembershipMutationError::Storage(RepositoryError::ContractViolation(format!(
            "platform membership audit serialization failed: {error}"
        )))
    })?;
    sqlx::query(
        "INSERT INTO audit_events
         (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
          platform_membership_id, roles_snapshot, capabilities_snapshot, action,
          resource_type, resource_id, correlation_id, detail_json, occurred_at)
         VALUES ($1, $2, 'platform', NULL, NULL, $3, $4::jsonb, $5::jsonb, $6,
                 'platform_membership', $7, $1, $8::jsonb, $9)",
    )
    .bind(id)
    .bind(&actor.identity_id)
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

fn pg_storage(error: sqlx::Error) -> PlatformMembershipMutationError {
    PlatformMembershipMutationError::Storage(RepositoryError::Postgres(error.to_string()))
}

fn run_pg_platform_membership<T, F>(future: F) -> Result<T, RepositoryError>
where
    F: Future<Output = Result<T, RepositoryError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        RepositoryError::AdapterUnavailable("platform membership runtime unavailable".into())
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(RepositoryError::AdapterUnavailable(
            "platform membership requires the multi-thread runtime".into(),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

fn run_pg_platform_membership_mutation<T, F>(
    future: F,
) -> Result<T, PlatformMembershipMutationError>
where
    F: Future<Output = Result<T, PlatformMembershipMutationError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        PlatformMembershipMutationError::Storage(RepositoryError::AdapterUnavailable(
            "platform membership runtime unavailable".into(),
        ))
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(PlatformMembershipMutationError::Storage(
            RepositoryError::AdapterUnavailable(
                "platform membership requires the multi-thread runtime".into(),
            ),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

fn pg_decode<T>(error: T) -> RepositoryError
where
    T: std::fmt::Display,
{
    RepositoryError::Postgres(error.to_string())
}
