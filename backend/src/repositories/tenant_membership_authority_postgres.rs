use std::future::Future;

use sqlx::postgres::PgPool;
use sqlx::{Postgres, Row, Transaction};
use system_admin::staff::{
    CreateTenantMemberInput, DeleteResult, PublicTenantMember, ResetPasswordInput,
    ResetPasswordResult, RevokeTenantMemberInput, ToggleResult, ToggleTenantMemberInput,
    UpdateUsernameInput,
};
use system_core::TenantRole;
use tokio::runtime::{Handle, RuntimeFlavor};
use uuid::Uuid;

use super::RepositoryError;
use super::tenant_membership_authority::{
    TenantMembershipActor, TenantMembershipAuthorityError, TenantOwnershipTransfer,
};

#[derive(Clone)]
pub(crate) struct PostgresTenantMembershipAuthorityRepository {
    pool: PgPool,
}

#[derive(Debug)]
struct TenantMemberRecord {
    membership_id: String,
    identity_id: String,
    username: String,
    role: String,
    is_enabled: bool,
    created_at: String,
    updated_at: String,
    last_login_at: Option<String>,
    membership_status: String,
    is_machine: bool,
}

impl PostgresTenantMembershipAuthorityRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn list(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<PublicTenantMember>, TenantMembershipAuthorityError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        run_pg_tenant_membership(async move {
            let rows = sqlx::query(
                "SELECT tm.id, i.id, i.username, tm.role,
                        (i.status = 'active' AND tm.status = 'active') AS is_enabled,
                        tm.created_at, tm.updated_at, i.last_login_at
                 FROM tenant_memberships tm
                 JOIN identities i ON i.id = tm.identity_id
                 WHERE tm.tenant_id = $1 AND tm.status != 'revoked'
                 ORDER BY tm.created_at DESC",
            )
            .bind(tenant_id)
            .fetch_all(&pool)
            .await
            .map_err(pg_storage)?;
            rows.into_iter()
                .map(|row| {
                    Ok(PublicTenantMember {
                        membership_id: row.try_get(0).map_err(pg_storage)?,
                        identity_id: row.try_get(1).map_err(pg_storage)?,
                        username: row.try_get(2).map_err(pg_storage)?,
                        role: row.try_get(3).map_err(pg_storage)?,
                        is_enabled: row.try_get(4).map_err(pg_storage)?,
                        created_at: row.try_get(5).map_err(pg_storage)?,
                        updated_at: row.try_get(6).map_err(pg_storage)?,
                        last_login_at: row.try_get(7).map_err(pg_storage)?,
                    })
                })
                .collect()
        })
    }

    pub(crate) fn create(
        &self,
        actor: &TenantMembershipActor,
        input: &CreateTenantMemberInput,
        now: &str,
    ) -> Result<PublicTenantMember, TenantMembershipAuthorityError> {
        require_role_management(actor.role, &input.role)?;
        if !matches!(input.role.as_str(), "admin" | "staff") {
            return Err(TenantMembershipAuthorityError::OwnerGovernanceRequired);
        }

        let pool = self.pool.clone();
        let actor = actor.clone();
        let input = input.clone();
        let now = now.to_owned();
        run_pg_tenant_membership(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            begin_tenant_governance_pg(&mut tx, &actor.tenant_id).await?;
            validate_actor_pg(&mut tx, &actor).await?;

            let username_exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM identities WHERE username = $1)")
                    .bind(&input.username)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(pg_storage)?;
            if username_exists {
                return Err(TenantMembershipAuthorityError::DuplicateUsername);
            }

            let identity_id = Uuid::new_v4().to_string();
            let membership_id = Uuid::new_v4().to_string();
            if let Err(error) = sqlx::query(
                "INSERT INTO identities
                 (id, username, email, password_hash, display_name, phone, totp_secret_ciphertext,
                  totp_enabled, status, last_login_at, created_at, updated_at)
                 VALUES ($1, $2, NULL, $3, $2, NULL, NULL, FALSE, 'active', NULL, $4, $4)",
            )
            .bind(&identity_id)
            .bind(&input.username)
            .bind(&input.password_hash)
            .bind(&now)
            .execute(&mut *tx)
            .await
            {
                if is_pg_unique_violation(&error) {
                    return Err(TenantMembershipAuthorityError::DuplicateUsername);
                }
                return Err(pg_storage(error));
            }
            sqlx::query(
                "INSERT INTO tenant_memberships
                 (id, identity_id, tenant_id, role, status, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, 'active', $5, $5)",
            )
            .bind(&membership_id)
            .bind(&identity_id)
            .bind(&actor.tenant_id)
            .bind(&input.role)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?;
            append_tenant_authority_audit_pg(
                &mut tx,
                &actor,
                "tenant_membership.created",
                &membership_id,
                &serde_json::json!({
                    "identityId": identity_id,
                    "username": input.username,
                    "role": input.role,
                }),
                &now,
            )
            .await?;
            tx.commit().await.map_err(pg_storage)?;

            Ok(PublicTenantMember {
                membership_id,
                identity_id,
                username: input.username,
                role: input.role,
                is_enabled: true,
                created_at: now.clone(),
                updated_at: now,
                last_login_at: None,
            })
        })
    }

    pub(crate) fn delete(
        &self,
        actor: &TenantMembershipActor,
        input: &RevokeTenantMemberInput,
        now: &str,
    ) -> Result<DeleteResult, TenantMembershipAuthorityError> {
        let pool = self.pool.clone();
        let actor = actor.clone();
        let input = input.clone();
        let now = now.to_owned();
        run_pg_tenant_membership(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            begin_tenant_governance_pg(&mut tx, &actor.tenant_id).await?;
            validate_actor_pg(&mut tx, &actor).await?;
            let target = find_member_pg(&mut tx, &input.id, &actor.tenant_id)
                .await?
                .ok_or(TenantMembershipAuthorityError::MembershipNotFound)?;
            require_role_management(actor.role, &target.role)?;

            if actor.identity_id == target.identity_id {
                return Err(TenantMembershipAuthorityError::SelfReference);
            }
            if would_remove_last_human_governance_pg(&mut tx, &target, &actor.tenant_id).await? {
                return Err(TenantMembershipAuthorityError::LastHumanGovernance);
            }

            sqlx::query(
                "UPDATE tenant_memberships
                 SET status = 'revoked', updated_at = $1
                 WHERE id = $2 AND tenant_id = $3",
            )
            .bind(&now)
            .bind(&target.membership_id)
            .bind(&actor.tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?;
            append_tenant_authority_audit_pg(
                &mut tx,
                &actor,
                "tenant_membership.revoked",
                &target.membership_id,
                &serde_json::json!({
                    "identityId": target.identity_id,
                    "role": target.role,
                }),
                &now,
            )
            .await?;
            tx.commit().await.map_err(pg_storage)?;

            Ok(DeleteResult {
                user: legacy_mutation_projection(
                    &target,
                    target.username.clone(),
                    target.is_enabled,
                    &now,
                ),
                revoked_session_count: 0,
            })
        })
    }

    pub(crate) fn toggle(
        &self,
        actor: &TenantMembershipActor,
        input: &ToggleTenantMemberInput,
        now: &str,
    ) -> Result<ToggleResult, TenantMembershipAuthorityError> {
        let pool = self.pool.clone();
        let actor = actor.clone();
        let input = input.clone();
        let now = now.to_owned();
        run_pg_tenant_membership(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            begin_tenant_governance_pg(&mut tx, &actor.tenant_id).await?;
            validate_actor_pg(&mut tx, &actor).await?;
            let target = find_member_pg(&mut tx, &input.id, &actor.tenant_id)
                .await?
                .ok_or(TenantMembershipAuthorityError::MembershipNotFound)?;
            require_role_management(actor.role, &target.role)?;
            let new_enabled = input.enabled.unwrap_or(!target.is_enabled);

            if !new_enabled && actor.identity_id == target.identity_id {
                return Err(TenantMembershipAuthorityError::SelfDisable);
            }
            if !new_enabled
                && would_remove_last_human_governance_pg(&mut tx, &target, &actor.tenant_id).await?
            {
                return Err(TenantMembershipAuthorityError::LastHumanGovernance);
            }

            sqlx::query(
                "UPDATE tenant_memberships
                 SET status = $1, updated_at = $2
                 WHERE id = $3 AND tenant_id = $4 AND status != 'revoked'",
            )
            .bind(if new_enabled { "active" } else { "suspended" })
            .bind(&now)
            .bind(&target.membership_id)
            .bind(&actor.tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?;
            append_tenant_authority_audit_pg(
                &mut tx,
                &actor,
                "tenant_membership.status_changed",
                &target.membership_id,
                &serde_json::json!({
                    "identityId": target.identity_id,
                    "enabled": new_enabled,
                    "role": target.role,
                }),
                &now,
            )
            .await?;
            tx.commit().await.map_err(pg_storage)?;

            Ok(ToggleResult {
                user: legacy_mutation_projection(
                    &target,
                    target.username.clone(),
                    new_enabled,
                    &now,
                ),
                revoked_session_count: 0,
            })
        })
    }

    pub(crate) fn reset_password(
        &self,
        actor: &TenantMembershipActor,
        input: &ResetPasswordInput,
        now: &str,
    ) -> Result<ResetPasswordResult, TenantMembershipAuthorityError> {
        let pool = self.pool.clone();
        let actor = actor.clone();
        let input = input.clone();
        let now = now.to_owned();
        run_pg_tenant_membership(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            begin_tenant_governance_pg(&mut tx, &actor.tenant_id).await?;
            validate_actor_pg(&mut tx, &actor).await?;
            let target = find_member_pg(&mut tx, &input.id, &actor.tenant_id)
                .await?
                .ok_or(TenantMembershipAuthorityError::MembershipNotFound)?;
            require_role_management(actor.role, &target.role)?;
            require_tenant_exclusive_identity_pg(&mut tx, &target.identity_id).await?;

            let revoked_session_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM auth_sessions
                 WHERE identity_id = $1 AND revoked_at IS NULL",
            )
            .bind(&target.identity_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(pg_storage)?;
            let revoked_session_count = usize::try_from(revoked_session_count).map_err(|_| {
                TenantMembershipAuthorityError::Storage(RepositoryError::ContractViolation(
                    "tenant password reset revoked-session count overflow".into(),
                ))
            })?;
            sqlx::query("UPDATE identities SET password_hash = $1, updated_at = $2 WHERE id = $3")
                .bind(&input.new_password_hash)
                .bind(&now)
                .bind(&target.identity_id)
                .execute(&mut *tx)
                .await
                .map_err(pg_storage)?;
            append_tenant_authority_audit_pg(
                &mut tx,
                &actor,
                "tenant_membership.password_reset",
                &target.membership_id,
                &serde_json::json!({
                    "identityId": target.identity_id,
                    "revokedSessionCount": revoked_session_count,
                }),
                &now,
            )
            .await?;
            tx.commit().await.map_err(pg_storage)?;

            Ok(ResetPasswordResult {
                user: legacy_mutation_projection(
                    &target,
                    target.username.clone(),
                    target.is_enabled,
                    &now,
                ),
                revoked_session_count,
            })
        })
    }

    pub(crate) fn update_username(
        &self,
        actor: &TenantMembershipActor,
        input: &UpdateUsernameInput,
        now: &str,
    ) -> Result<PublicTenantMember, TenantMembershipAuthorityError> {
        let pool = self.pool.clone();
        let actor = actor.clone();
        let input = input.clone();
        let now = now.to_owned();
        run_pg_tenant_membership(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            begin_tenant_governance_pg(&mut tx, &actor.tenant_id).await?;
            validate_actor_pg(&mut tx, &actor).await?;
            let target = find_member_pg(&mut tx, &input.id, &actor.tenant_id)
                .await?
                .ok_or(TenantMembershipAuthorityError::MembershipNotFound)?;
            require_role_management(actor.role, &target.role)?;
            require_tenant_exclusive_identity_pg(&mut tx, &target.identity_id).await?;
            let normalized = input.new_username.trim().to_string();

            if normalized != target.username {
                let exists: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM identities WHERE username = $1)",
                )
                .bind(&normalized)
                .fetch_one(&mut *tx)
                .await
                .map_err(pg_storage)?;
                if exists {
                    return Err(TenantMembershipAuthorityError::DuplicateUsername);
                }
            }

            if let Err(error) =
                sqlx::query("UPDATE identities SET username = $1, updated_at = $2 WHERE id = $3")
                    .bind(&normalized)
                    .bind(&now)
                    .bind(&target.identity_id)
                    .execute(&mut *tx)
                    .await
            {
                if is_pg_unique_violation(&error) {
                    return Err(TenantMembershipAuthorityError::DuplicateUsername);
                }
                return Err(pg_storage(error));
            }
            append_tenant_authority_audit_pg(
                &mut tx,
                &actor,
                "tenant_membership.username_changed",
                &target.membership_id,
                &serde_json::json!({
                    "identityId": target.identity_id,
                    "username": normalized,
                }),
                &now,
            )
            .await?;
            tx.commit().await.map_err(pg_storage)?;

            Ok(legacy_mutation_projection(
                &target,
                normalized,
                target.is_enabled,
                &now,
            ))
        })
    }

    pub(crate) fn transfer_ownership(
        &self,
        actor: &TenantMembershipActor,
        target_membership_id: &str,
        now: &str,
    ) -> Result<TenantOwnershipTransfer, TenantMembershipAuthorityError> {
        if actor.role != TenantRole::Owner {
            return Err(TenantMembershipAuthorityError::RoleEscalation);
        }
        if actor.membership_id == target_membership_id {
            return Err(TenantMembershipAuthorityError::InvalidOwnershipTarget);
        }

        let pool = self.pool.clone();
        let actor = actor.clone();
        let target_membership_id = target_membership_id.to_owned();
        let now = now.to_owned();
        run_pg_tenant_membership(async move {
            let mut tx = pool.begin().await.map_err(pg_storage)?;
            begin_tenant_governance_pg(&mut tx, &actor.tenant_id).await?;
            validate_actor_pg(&mut tx, &actor).await?;
            let target = find_member_pg(&mut tx, &target_membership_id, &actor.tenant_id)
                .await?
                .ok_or(TenantMembershipAuthorityError::OwnershipTargetNotFound)?;
            if target.is_machine {
                return Err(TenantMembershipAuthorityError::MachineOwnerForbidden);
            }
            if target.membership_status != "active"
                || !matches!(target.role.as_str(), "admin" | "staff")
            {
                return Err(TenantMembershipAuthorityError::InvalidOwnershipTarget);
            }

            let demoted = sqlx::query(
                "UPDATE tenant_memberships
                 SET role = 'admin', updated_at = $1
                 WHERE id = $2 AND identity_id = $3 AND tenant_id = $4
                   AND role = 'owner' AND status = 'active'",
            )
            .bind(&now)
            .bind(&actor.membership_id)
            .bind(&actor.identity_id)
            .bind(&actor.tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?
            .rows_affected();
            if demoted != 1 {
                return Err(TenantMembershipAuthorityError::StaleOwner);
            }
            let promoted = sqlx::query(
                "UPDATE tenant_memberships
                 SET role = 'owner', updated_at = $1
                 WHERE id = $2 AND tenant_id = $3 AND status = 'active'",
            )
            .bind(&now)
            .bind(&target.membership_id)
            .bind(&actor.tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_storage)?
            .rows_affected();
            if promoted != 1 {
                return Err(TenantMembershipAuthorityError::OwnershipTargetNotFound);
            }
            append_tenant_authority_audit_pg(
                &mut tx,
                &actor,
                "tenant_ownership.transferred",
                &target.membership_id,
                &serde_json::json!({
                    "previousOwnerMembershipId": actor.membership_id,
                    "ownerMembershipId": target.membership_id,
                }),
                &now,
            )
            .await?;
            tx.commit().await.map_err(pg_storage)?;

            Ok(TenantOwnershipTransfer {
                tenant_id: actor.tenant_id,
                previous_owner_membership_id: actor.membership_id,
                owner_membership_id: target.membership_id,
            })
        })
    }
}

fn legacy_mutation_projection(
    target: &TenantMemberRecord,
    username: String,
    is_enabled: bool,
    updated_at: &str,
) -> PublicTenantMember {
    PublicTenantMember {
        membership_id: target.membership_id.clone(),
        identity_id: target.identity_id.clone(),
        username,
        role: target.role.clone(),
        is_enabled,
        created_at: String::new(),
        updated_at: updated_at.to_owned(),
        last_login_at: None,
    }
}

async fn begin_tenant_governance_pg(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: &str,
) -> Result<(), TenantMembershipAuthorityError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut **tx)
        .await
        .map_err(pg_storage)?;
    let locked = sqlx::query_scalar::<_, String>(
        "SELECT id FROM tenants WHERE id = $1 AND status != 'deleted' FOR UPDATE",
    )
    .bind(tenant_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(pg_storage)?;
    if locked.is_none() {
        return Err(TenantMembershipAuthorityError::Storage(
            RepositoryError::ContractViolation("tenant governance scope unavailable".into()),
        ));
    }
    Ok(())
}

async fn validate_actor_pg(
    tx: &mut Transaction<'_, Postgres>,
    actor: &TenantMembershipActor,
) -> Result<(), TenantMembershipAuthorityError> {
    let role = sqlx::query_scalar::<_, String>(
        "SELECT role
         FROM tenant_memberships
         WHERE id = $1 AND identity_id = $2 AND tenant_id = $3 AND status = 'active'",
    )
    .bind(&actor.membership_id)
    .bind(&actor.identity_id)
    .bind(&actor.tenant_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(pg_storage)?;
    if role.as_deref() != Some(tenant_role_name(actor.role)) {
        return Err(TenantMembershipAuthorityError::RoleEscalation);
    }
    Ok(())
}

async fn find_member_pg(
    tx: &mut Transaction<'_, Postgres>,
    membership_id: &str,
    tenant_id: &str,
) -> Result<Option<TenantMemberRecord>, TenantMembershipAuthorityError> {
    let row = sqlx::query(
        "SELECT tm.id, i.id, i.username, tm.role,
                (i.status = 'active' AND tm.status = 'active') AS is_enabled,
                tm.created_at, tm.updated_at, i.last_login_at, tm.status,
                EXISTS(SELECT 1 FROM machine_identities mi WHERE mi.identity_id = tm.identity_id)
         FROM tenant_memberships tm
         JOIN identities i ON i.id = tm.identity_id
         WHERE tm.id = $1 AND tm.tenant_id = $2
         LIMIT 1",
    )
    .bind(membership_id)
    .bind(tenant_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(pg_storage)?;
    row.map(|row| {
        Ok(TenantMemberRecord {
            membership_id: row.try_get(0).map_err(pg_storage)?,
            identity_id: row.try_get(1).map_err(pg_storage)?,
            username: row.try_get(2).map_err(pg_storage)?,
            role: row.try_get(3).map_err(pg_storage)?,
            is_enabled: row.try_get(4).map_err(pg_storage)?,
            created_at: row.try_get(5).map_err(pg_storage)?,
            updated_at: row.try_get(6).map_err(pg_storage)?,
            last_login_at: row.try_get(7).map_err(pg_storage)?,
            membership_status: row.try_get(8).map_err(pg_storage)?,
            is_machine: row.try_get(9).map_err(pg_storage)?,
        })
    })
    .transpose()
}

async fn would_remove_last_human_governance_pg(
    tx: &mut Transaction<'_, Postgres>,
    target: &TenantMemberRecord,
    tenant_id: &str,
) -> Result<bool, TenantMembershipAuthorityError> {
    if !matches!(target.role.as_str(), "owner" | "admin") || target.is_machine {
        return Ok(false);
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM tenant_memberships tm
         WHERE tm.role IN ('owner', 'admin')
           AND tm.status = 'active'
           AND tm.tenant_id = $1
           AND NOT EXISTS (
             SELECT 1 FROM machine_identities mi WHERE mi.identity_id = tm.identity_id
           )",
    )
    .bind(tenant_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(pg_storage)?;
    Ok(count <= 1)
}

async fn require_tenant_exclusive_identity_pg(
    tx: &mut Transaction<'_, Postgres>,
    identity_id: &str,
) -> Result<(), TenantMembershipAuthorityError> {
    let authority_count: i64 = sqlx::query_scalar(
        "SELECT
           (SELECT COUNT(*) FROM tenant_memberships
            WHERE identity_id = $1 AND status != 'revoked')
         + (SELECT COUNT(*) FROM platform_memberships
            WHERE identity_id = $1 AND status != 'revoked')",
    )
    .bind(identity_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(pg_storage)?;
    if authority_count != 1 {
        Err(TenantMembershipAuthorityError::SharedIdentity)
    } else {
        Ok(())
    }
}

fn require_role_management(
    actor_role: TenantRole,
    target_role: &str,
) -> Result<(), TenantMembershipAuthorityError> {
    match (actor_role, target_role) {
        (_, "owner") => Err(TenantMembershipAuthorityError::OwnerGovernanceRequired),
        (TenantRole::Owner, "admin" | "staff") | (TenantRole::Admin, "staff") => Ok(()),
        _ => Err(TenantMembershipAuthorityError::RoleEscalation),
    }
}

async fn append_tenant_authority_audit_pg(
    tx: &mut Transaction<'_, Postgres>,
    actor: &TenantMembershipActor,
    action: &str,
    resource_id: &str,
    detail: &serde_json::Value,
    occurred_at: &str,
) -> Result<(), TenantMembershipAuthorityError> {
    let id = Uuid::new_v4().to_string();
    let roles_snapshot =
        serde_json::to_string(&[tenant_role_name(actor.role)]).map_err(|error| {
            TenantMembershipAuthorityError::Storage(RepositoryError::ContractViolation(format!(
                "tenant membership role snapshot serialization failed: {error}"
            )))
        })?;
    let detail_json = serde_json::to_string(detail).map_err(|error| {
        TenantMembershipAuthorityError::Storage(RepositoryError::ContractViolation(format!(
            "tenant membership audit serialization failed: {error}"
        )))
    })?;
    sqlx::query(
        "INSERT INTO audit_events
         (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
          platform_membership_id, roles_snapshot, capabilities_snapshot, action,
          resource_type, resource_id, correlation_id, detail_json, occurred_at)
         VALUES ($1, $2, 'tenant', $3, $4, NULL, $5::jsonb, '[]'::jsonb, $6,
                 'tenant_membership', $7, $1, $8::jsonb, $9)",
    )
    .bind(id)
    .bind(&actor.identity_id)
    .bind(&actor.tenant_id)
    .bind(&actor.membership_id)
    .bind(roles_snapshot)
    .bind(action)
    .bind(resource_id)
    .bind(detail_json)
    .bind(occurred_at)
    .execute(&mut **tx)
    .await
    .map_err(pg_storage)?;
    Ok(())
}

fn tenant_role_name(role: TenantRole) -> &'static str {
    match role {
        TenantRole::Owner => "owner",
        TenantRole::Admin => "admin",
        TenantRole::Staff => "staff",
    }
}

fn is_pg_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.is_unique_violation())
}

fn pg_storage(error: sqlx::Error) -> TenantMembershipAuthorityError {
    if matches!(
        &error,
        sqlx::Error::Database(database)
            if database.code().as_deref() == Some("40001")
    ) {
        return TenantMembershipAuthorityError::StaleOwner;
    }
    TenantMembershipAuthorityError::Storage(RepositoryError::Postgres(error.to_string()))
}

fn run_pg_tenant_membership<T, F>(future: F) -> Result<T, TenantMembershipAuthorityError>
where
    F: Future<Output = Result<T, TenantMembershipAuthorityError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        TenantMembershipAuthorityError::Storage(RepositoryError::AdapterUnavailable(
            "tenant membership runtime unavailable".into(),
        ))
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(TenantMembershipAuthorityError::Storage(
            RepositoryError::AdapterUnavailable(
                "tenant membership requires the multi-thread runtime".into(),
            ),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}
