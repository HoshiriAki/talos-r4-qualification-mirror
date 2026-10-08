use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use system_admin::staff::{
    CreateTenantMemberInput, DeleteResult, PublicTenantMember, ResetPasswordInput,
    ResetPasswordResult, RevokeTenantMemberInput, ToggleResult, ToggleTenantMemberInput,
    UpdateUsernameInput,
};
use system_core::TenantRole;
use uuid::Uuid;

use super::RepositoryError;

#[derive(Debug, Clone)]
pub(crate) struct TenantMembershipActor {
    pub tenant_id: String,
    pub membership_id: String,
    pub identity_id: String,
    pub role: TenantRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TenantOwnershipTransfer {
    pub tenant_id: String,
    pub previous_owner_membership_id: String,
    pub owner_membership_id: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TenantMembershipAuthorityError {
    #[error("tenant membership not found")]
    MembershipNotFound,
    #[error("tenant username already exists")]
    DuplicateUsername,
    #[error("tenant membership cannot mutate itself")]
    SelfReference,
    #[error("tenant membership cannot disable itself")]
    SelfDisable,
    #[error("tenant must retain one human governance member")]
    LastHumanGovernance,
    #[error("shared identity requires identity governance")]
    SharedIdentity,
    #[error("tenant owner governance flow required")]
    OwnerGovernanceRequired,
    #[error("tenant role escalation rejected")]
    RoleEscalation,
    #[error("tenant ownership target not found")]
    OwnershipTargetNotFound,
    #[error("machine identity cannot become tenant owner")]
    MachineOwnerForbidden,
    #[error("tenant ownership target must be active admin or staff")]
    InvalidOwnershipTarget,
    #[error("tenant owner state changed")]
    StaleOwner,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

#[derive(Clone)]
pub(crate) struct SqliteTenantMembershipAuthorityRepository {
    pool: Pool<SqliteConnectionManager>,
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

impl SqliteTenantMembershipAuthorityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn list(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<PublicTenantMember>, TenantMembershipAuthorityError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT tm.id, i.id, i.username, tm.role,
                        CASE WHEN i.status = 'active' AND tm.status = 'active' THEN 1 ELSE 0 END,
                        tm.created_at, tm.updated_at, i.last_login_at
                 FROM tenant_memberships tm
                 JOIN identities i ON i.id = tm.identity_id
                 WHERE tm.tenant_id = ?1 AND tm.status != 'revoked'
                 ORDER BY tm.created_at DESC",
            )
            .map_err(sqlite_storage)?;
        stmt.query_map([tenant_id], |row| {
            Ok(PublicTenantMember {
                membership_id: row.get(0)?,
                identity_id: row.get(1)?,
                username: row.get(2)?,
                role: row.get(3)?,
                is_enabled: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                last_login_at: row.get(7)?,
            })
        })
        .map_err(sqlite_storage)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_storage)
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

        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;
        validate_actor_sqlite(&tx, actor)?;

        let username_exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM identities WHERE username = ?1)",
                [input.username.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_storage)?;
        if username_exists {
            return Err(TenantMembershipAuthorityError::DuplicateUsername);
        }

        let identity_id = Uuid::new_v4().to_string();
        let membership_id = Uuid::new_v4().to_string();
        if let Err(error) = tx.execute(
            "INSERT INTO identities
             (id, username, email, password_hash, display_name, phone, totp_secret_ciphertext,
              totp_enabled, status, last_login_at, created_at, updated_at)
             VALUES (?1, ?2, NULL, ?3, ?2, NULL, NULL, 0, 'active', NULL, ?4, ?4)",
            params![
                identity_id.as_str(),
                input.username.as_str(),
                input.password_hash.as_str(),
                now,
            ],
        ) {
            if is_sqlite_constraint(&error) {
                return Err(TenantMembershipAuthorityError::DuplicateUsername);
            }
            return Err(sqlite_storage(error));
        }
        tx.execute(
            "INSERT INTO tenant_memberships
             (id, identity_id, tenant_id, role, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?5)",
            params![
                membership_id.as_str(),
                identity_id.as_str(),
                actor.tenant_id.as_str(),
                input.role.as_str(),
                now,
            ],
        )
        .map_err(sqlite_storage)?;
        append_tenant_authority_audit_sqlite(
            &tx,
            actor,
            "tenant_membership.created",
            &membership_id,
            &serde_json::json!({
                "identityId": identity_id,
                "username": input.username,
                "role": input.role,
            }),
            now,
        )?;
        tx.commit().map_err(sqlite_storage)?;

        Ok(PublicTenantMember {
            membership_id,
            identity_id,
            username: input.username.clone(),
            role: input.role.clone(),
            is_enabled: true,
            created_at: now.to_string(),
            updated_at: now.to_string(),
            last_login_at: None,
        })
    }

    pub(crate) fn delete(
        &self,
        actor: &TenantMembershipActor,
        input: &RevokeTenantMemberInput,
        now: &str,
    ) -> Result<DeleteResult, TenantMembershipAuthorityError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;
        validate_actor_sqlite(&tx, actor)?;
        let target = find_member_sqlite(&tx, &input.id, &actor.tenant_id)?
            .ok_or(TenantMembershipAuthorityError::MembershipNotFound)?;
        require_role_management(actor.role, &target.role)?;

        if actor.identity_id == target.identity_id {
            return Err(TenantMembershipAuthorityError::SelfReference);
        }
        if would_remove_last_human_governance_sqlite(&tx, &target, &actor.tenant_id)? {
            return Err(TenantMembershipAuthorityError::LastHumanGovernance);
        }

        tx.execute(
            "UPDATE tenant_memberships
             SET status = 'revoked', updated_at = ?1
             WHERE id = ?2 AND tenant_id = ?3",
            params![now, target.membership_id.as_str(), actor.tenant_id.as_str(),],
        )
        .map_err(sqlite_storage)?;
        append_tenant_authority_audit_sqlite(
            &tx,
            actor,
            "tenant_membership.revoked",
            &target.membership_id,
            &serde_json::json!({
                "identityId": target.identity_id,
                "role": target.role,
            }),
            now,
        )?;
        tx.commit().map_err(sqlite_storage)?;

        Ok(DeleteResult {
            user: legacy_mutation_projection(
                &target,
                target.username.clone(),
                target.is_enabled,
                now,
            ),
            revoked_session_count: 0,
        })
    }

    pub(crate) fn toggle(
        &self,
        actor: &TenantMembershipActor,
        input: &ToggleTenantMemberInput,
        now: &str,
    ) -> Result<ToggleResult, TenantMembershipAuthorityError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;
        validate_actor_sqlite(&tx, actor)?;
        let target = find_member_sqlite(&tx, &input.id, &actor.tenant_id)?
            .ok_or(TenantMembershipAuthorityError::MembershipNotFound)?;
        require_role_management(actor.role, &target.role)?;
        let new_enabled = input.enabled.unwrap_or(!target.is_enabled);

        if !new_enabled && actor.identity_id == target.identity_id {
            return Err(TenantMembershipAuthorityError::SelfDisable);
        }
        if !new_enabled
            && would_remove_last_human_governance_sqlite(&tx, &target, &actor.tenant_id)?
        {
            return Err(TenantMembershipAuthorityError::LastHumanGovernance);
        }

        tx.execute(
            "UPDATE tenant_memberships
             SET status = ?1, updated_at = ?2
             WHERE id = ?3 AND tenant_id = ?4 AND status != 'revoked'",
            params![
                if new_enabled { "active" } else { "suspended" },
                now,
                target.membership_id.as_str(),
                actor.tenant_id.as_str(),
            ],
        )
        .map_err(sqlite_storage)?;
        append_tenant_authority_audit_sqlite(
            &tx,
            actor,
            "tenant_membership.status_changed",
            &target.membership_id,
            &serde_json::json!({
                "identityId": target.identity_id,
                "enabled": new_enabled,
                "role": target.role,
            }),
            now,
        )?;
        tx.commit().map_err(sqlite_storage)?;

        Ok(ToggleResult {
            user: legacy_mutation_projection(&target, target.username.clone(), new_enabled, now),
            revoked_session_count: 0,
        })
    }

    pub(crate) fn reset_password(
        &self,
        actor: &TenantMembershipActor,
        input: &ResetPasswordInput,
        now: &str,
    ) -> Result<ResetPasswordResult, TenantMembershipAuthorityError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;
        validate_actor_sqlite(&tx, actor)?;
        let target = find_member_sqlite(&tx, &input.id, &actor.tenant_id)?
            .ok_or(TenantMembershipAuthorityError::MembershipNotFound)?;
        require_role_management(actor.role, &target.role)?;
        require_tenant_exclusive_identity_sqlite(&tx, &target.identity_id)?;

        let revoked_session_count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM auth_sessions
                 WHERE identity_id = ?1 AND revoked_at IS NULL",
                [target.identity_id.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_storage)?;
        let revoked_session_count = usize::try_from(revoked_session_count).map_err(|_| {
            TenantMembershipAuthorityError::Storage(RepositoryError::ContractViolation(
                "tenant password reset revoked-session count overflow".into(),
            ))
        })?;
        tx.execute(
            "UPDATE identities SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
            params![
                input.new_password_hash.as_str(),
                now,
                target.identity_id.as_str(),
            ],
        )
        .map_err(sqlite_storage)?;
        append_tenant_authority_audit_sqlite(
            &tx,
            actor,
            "tenant_membership.password_reset",
            &target.membership_id,
            &serde_json::json!({
                "identityId": target.identity_id,
                "revokedSessionCount": revoked_session_count,
            }),
            now,
        )?;
        tx.commit().map_err(sqlite_storage)?;

        Ok(ResetPasswordResult {
            user: legacy_mutation_projection(
                &target,
                target.username.clone(),
                target.is_enabled,
                now,
            ),
            revoked_session_count,
        })
    }

    pub(crate) fn update_username(
        &self,
        actor: &TenantMembershipActor,
        input: &UpdateUsernameInput,
        now: &str,
    ) -> Result<PublicTenantMember, TenantMembershipAuthorityError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;
        validate_actor_sqlite(&tx, actor)?;
        let target = find_member_sqlite(&tx, &input.id, &actor.tenant_id)?
            .ok_or(TenantMembershipAuthorityError::MembershipNotFound)?;
        require_role_management(actor.role, &target.role)?;
        require_tenant_exclusive_identity_sqlite(&tx, &target.identity_id)?;
        let normalized = input.new_username.trim();

        if normalized != target.username {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM identities WHERE username = ?1)",
                    [normalized],
                    |row| row.get(0),
                )
                .map_err(sqlite_storage)?;
            if exists {
                return Err(TenantMembershipAuthorityError::DuplicateUsername);
            }
        }

        if let Err(error) = tx.execute(
            "UPDATE identities SET username = ?1, updated_at = ?2 WHERE id = ?3",
            params![normalized, now, target.identity_id.as_str()],
        ) {
            if is_sqlite_constraint(&error) {
                return Err(TenantMembershipAuthorityError::DuplicateUsername);
            }
            return Err(sqlite_storage(error));
        }
        append_tenant_authority_audit_sqlite(
            &tx,
            actor,
            "tenant_membership.username_changed",
            &target.membership_id,
            &serde_json::json!({
                "identityId": target.identity_id,
                "username": normalized,
            }),
            now,
        )?;
        tx.commit().map_err(sqlite_storage)?;

        Ok(legacy_mutation_projection(
            &target,
            normalized.to_string(),
            target.is_enabled,
            now,
        ))
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

        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;
        validate_actor_sqlite(&tx, actor)?;

        let target = find_member_sqlite(&tx, target_membership_id, &actor.tenant_id)?
            .ok_or(TenantMembershipAuthorityError::OwnershipTargetNotFound)?;
        if target.is_machine {
            return Err(TenantMembershipAuthorityError::MachineOwnerForbidden);
        }
        if target.membership_status != "active"
            || !matches!(target.role.as_str(), "admin" | "staff")
        {
            return Err(TenantMembershipAuthorityError::InvalidOwnershipTarget);
        }

        let demoted = tx
            .execute(
                "UPDATE tenant_memberships
                 SET role = 'admin', updated_at = ?1
                 WHERE id = ?2 AND identity_id = ?3 AND tenant_id = ?4
                   AND role = 'owner' AND status = 'active'",
                params![
                    now,
                    actor.membership_id.as_str(),
                    actor.identity_id.as_str(),
                    actor.tenant_id.as_str(),
                ],
            )
            .map_err(sqlite_storage)?;
        if demoted != 1 {
            return Err(TenantMembershipAuthorityError::StaleOwner);
        }
        let promoted = tx
            .execute(
                "UPDATE tenant_memberships
                 SET role = 'owner', updated_at = ?1
                 WHERE id = ?2 AND tenant_id = ?3 AND status = 'active'",
                params![now, target.membership_id.as_str(), actor.tenant_id.as_str()],
            )
            .map_err(sqlite_storage)?;
        if promoted != 1 {
            return Err(TenantMembershipAuthorityError::OwnershipTargetNotFound);
        }
        append_tenant_authority_audit_sqlite(
            &tx,
            actor,
            "tenant_ownership.transferred",
            &target.membership_id,
            &serde_json::json!({
                "previousOwnerMembershipId": actor.membership_id,
                "ownerMembershipId": target.membership_id,
            }),
            now,
        )?;
        tx.commit().map_err(sqlite_storage)?;

        Ok(TenantOwnershipTransfer {
            tenant_id: actor.tenant_id.clone(),
            previous_owner_membership_id: actor.membership_id.clone(),
            owner_membership_id: target.membership_id,
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

fn validate_actor_sqlite(
    tx: &Transaction<'_>,
    actor: &TenantMembershipActor,
) -> Result<(), TenantMembershipAuthorityError> {
    let role: Option<String> = tx
        .query_row(
            "SELECT role
             FROM tenant_memberships
             WHERE id = ?1 AND identity_id = ?2 AND tenant_id = ?3 AND status = 'active'",
            params![
                actor.membership_id.as_str(),
                actor.identity_id.as_str(),
                actor.tenant_id.as_str(),
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_storage)?;
    if role.as_deref() != Some(tenant_role_name(actor.role)) {
        return Err(TenantMembershipAuthorityError::RoleEscalation);
    }
    Ok(())
}

fn find_member_sqlite(
    tx: &Transaction<'_>,
    membership_id: &str,
    tenant_id: &str,
) -> Result<Option<TenantMemberRecord>, TenantMembershipAuthorityError> {
    tx.query_row(
        "SELECT tm.id, i.id, i.username, tm.role,
                CASE WHEN i.status = 'active' AND tm.status = 'active' THEN 1 ELSE 0 END,
                tm.created_at, tm.updated_at, i.last_login_at, tm.status,
                EXISTS(SELECT 1 FROM machine_identities mi WHERE mi.identity_id = tm.identity_id)
         FROM tenant_memberships tm
         JOIN identities i ON i.id = tm.identity_id
         WHERE tm.id = ?1 AND tm.tenant_id = ?2
         LIMIT 1",
        params![membership_id, tenant_id],
        |row| {
            Ok(TenantMemberRecord {
                membership_id: row.get(0)?,
                identity_id: row.get(1)?,
                username: row.get(2)?,
                role: row.get(3)?,
                is_enabled: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                last_login_at: row.get(7)?,
                membership_status: row.get(8)?,
                is_machine: row.get::<_, i32>(9)? != 0,
            })
        },
    )
    .optional()
    .map_err(sqlite_storage)
}

fn would_remove_last_human_governance_sqlite(
    tx: &Transaction<'_>,
    target: &TenantMemberRecord,
    tenant_id: &str,
) -> Result<bool, TenantMembershipAuthorityError> {
    if !matches!(target.role.as_str(), "owner" | "admin") || target.is_machine {
        return Ok(false);
    }
    let count: i64 = tx
        .query_row(
            "SELECT COUNT(*)
             FROM tenant_memberships tm
             WHERE tm.role IN ('owner', 'admin')
               AND tm.status = 'active'
               AND tm.tenant_id = ?1
               AND NOT EXISTS (
                 SELECT 1 FROM machine_identities mi WHERE mi.identity_id = tm.identity_id
               )",
            [tenant_id],
            |row| row.get(0),
        )
        .map_err(sqlite_storage)?;
    Ok(count <= 1)
}

fn require_tenant_exclusive_identity_sqlite(
    tx: &Transaction<'_>,
    identity_id: &str,
) -> Result<(), TenantMembershipAuthorityError> {
    let authority_count: i64 = tx
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM tenant_memberships
                WHERE identity_id = ?1 AND status != 'revoked')
             + (SELECT COUNT(*) FROM platform_memberships
                WHERE identity_id = ?1 AND status != 'revoked')",
            [identity_id],
            |row| row.get(0),
        )
        .map_err(sqlite_storage)?;
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

fn append_tenant_authority_audit_sqlite(
    tx: &Transaction<'_>,
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
    tx.execute(
        "INSERT INTO audit_events
         (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
          platform_membership_id, roles_snapshot, capabilities_snapshot, action,
          resource_type, resource_id, correlation_id, detail_json, occurred_at)
         VALUES (?1, ?2, 'tenant', ?3, ?4, NULL, ?5, '[]', ?6,
                 'tenant_membership', ?7, ?1, ?8, ?9)",
        params![
            id,
            actor.identity_id,
            actor.tenant_id,
            actor.membership_id,
            roles_snapshot,
            action,
            resource_id,
            detail_json,
            occurred_at,
        ],
    )
    .map_err(sqlite_storage)?;
    Ok(())
}

fn tenant_role_name(role: TenantRole) -> &'static str {
    match role {
        TenantRole::Owner => "owner",
        TenantRole::Admin => "admin",
        TenantRole::Staff => "staff",
    }
}

fn is_sqlite_constraint(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

fn sqlite_storage(error: rusqlite::Error) -> TenantMembershipAuthorityError {
    TenantMembershipAuthorityError::Storage(RepositoryError::Sqlite(error.to_string()))
}
