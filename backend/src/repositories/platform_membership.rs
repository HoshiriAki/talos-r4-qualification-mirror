use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use uuid::Uuid;

use super::RepositoryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlatformMembershipProjection {
    pub id: String,
    pub identity_id: String,
    pub username: String,
    pub display_name: String,
    pub email: String,
    pub status: String,
    pub roles: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PlatformMembershipAuditActor {
    pub identity_id: String,
    pub membership_id: String,
    pub roles_snapshot: String,
    pub capabilities_snapshot: String,
}

#[derive(Debug, Clone)]
pub(crate) struct NewPlatformIdentity {
    pub username: String,
    pub email: Option<String>,
    pub password_hash: String,
    pub display_name: String,
    pub phone: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CreatePlatformMembership {
    pub existing_identity_id: Option<String>,
    pub new_identity: Option<NewPlatformIdentity>,
    pub roles: Vec<String>,
    pub actor: PlatformMembershipAuditActor,
    pub actor_is_platform_owner: bool,
    pub now: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreatedPlatformMembership {
    pub membership_id: String,
    pub identity_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct UpdatePlatformMembership {
    pub membership_id: String,
    pub status: Option<String>,
    pub roles: Option<Vec<String>>,
    pub actor: PlatformMembershipAuditActor,
    pub actor_is_platform_owner: bool,
    pub now: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RevokePlatformMembership {
    pub membership_id: String,
    pub actor: PlatformMembershipAuditActor,
    pub actor_is_platform_owner: bool,
    pub now: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PlatformMembershipMutationError {
    #[error("platform membership identity not found")]
    IdentityNotFound,
    #[error("platform identity already exists")]
    DuplicateIdentity,
    #[error("platform membership already exists")]
    MembershipExists,
    #[error("platform membership not found")]
    MembershipNotFound,
    #[error("platform owner authority required")]
    OwnerAuthorityRequired,
    #[error("platform must retain one active owner")]
    LastActiveOwner,
    #[error("platform membership cannot revoke itself")]
    SelfRevoke,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

#[derive(Clone)]
pub(crate) struct SqlitePlatformMembershipRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqlitePlatformMembershipRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn list(&self) -> Result<Vec<PlatformMembershipProjection>, RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT pm.id, pm.identity_id, i.username, i.display_name, COALESCE(i.email, ''),
                        pm.status, pm.created_at, pm.updated_at
                 FROM platform_memberships pm
                 JOIN identities i ON i.id = pm.identity_id
                 ORDER BY pm.created_at DESC",
            )
            .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
        let base = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .map_err(|error| RepositoryError::Sqlite(error.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;

        let mut memberships = Vec::with_capacity(base.len());
        for (id, identity_id, username, display_name, email, status, created_at, updated_at) in base
        {
            let mut role_stmt = conn
                .prepare(
                    "SELECT role FROM platform_role_grants
                     WHERE platform_membership_id = ?1 ORDER BY role",
                )
                .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
            let roles = role_stmt
                .query_map([&id], |row| row.get::<_, String>(0))
                .map_err(|error| RepositoryError::Sqlite(error.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
            memberships.push(PlatformMembershipProjection {
                id,
                identity_id,
                username,
                display_name,
                email,
                status,
                roles,
                created_at,
                updated_at,
            });
        }
        Ok(memberships)
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
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;

        if command.roles.iter().any(|role| role == "platform_owner")
            && !actor_has_active_owner_sqlite(&tx, &command.actor)?
        {
            return Err(PlatformMembershipMutationError::OwnerAuthorityRequired);
        }

        let identity_id = if let Some(identity_id) = command.existing_identity_id.as_deref() {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM identities WHERE id = ?1 AND status = 'active')",
                    [identity_id],
                    |row| row.get(0),
                )
                .map_err(sqlite_storage)?;
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
            if let Err(error) = tx.execute(
                "INSERT INTO identities
                 (id, username, email, password_hash, display_name, phone, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?7)",
                params![
                    identity_id.as_str(),
                    identity.username.as_str(),
                    identity.email.as_deref(),
                    identity.password_hash.as_str(),
                    identity.display_name.as_str(),
                    identity.phone.as_deref(),
                    command.now.as_str(),
                ],
            ) {
                if is_sqlite_constraint(&error) {
                    return Err(PlatformMembershipMutationError::DuplicateIdentity);
                }
                return Err(sqlite_storage(error));
            }
            identity_id
        };

        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM platform_memberships WHERE identity_id = ?1",
                [&identity_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_storage)?;
        if existing.is_some() {
            return Err(PlatformMembershipMutationError::MembershipExists);
        }

        let membership_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO platform_memberships
             (id, identity_id, status, created_at, updated_at)
             VALUES (?1, ?2, 'active', ?3, ?3)",
            params![
                membership_id.as_str(),
                identity_id.as_str(),
                command.now.as_str()
            ],
        )
        .map_err(sqlite_storage)?;
        replace_roles_sqlite(
            &tx,
            &membership_id,
            &command.roles,
            &command.actor.identity_id,
            &command.now,
            false,
        )?;
        append_platform_audit_sqlite(
            &tx,
            &command.actor,
            "platform_membership.created",
            &membership_id,
            &serde_json::json!({
                "identityId": identity_id,
                "roles": command.roles,
            }),
            &command.now,
        )?;
        tx.commit().map_err(sqlite_storage)?;

        Ok(CreatedPlatformMembership {
            membership_id,
            identity_id,
        })
    }

    pub(crate) fn update(
        &self,
        command: UpdatePlatformMembership,
    ) -> Result<(), PlatformMembershipMutationError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;

        let owner_grant = membership_has_owner_grant_sqlite(&tx, &command.membership_id)?
            .ok_or(PlatformMembershipMutationError::MembershipNotFound)?;
        let changes_owner_grant = owner_grant
            || command
                .roles
                .as_ref()
                .is_some_and(|roles| roles.iter().any(|role| role == "platform_owner"));
        if changes_owner_grant
            && (!command.actor_is_platform_owner
                || !actor_has_active_owner_sqlite(&tx, &command.actor)?)
        {
            return Err(PlatformMembershipMutationError::OwnerAuthorityRequired);
        }
        if let Some(status) = command.status.as_deref() {
            tx.execute(
                "UPDATE platform_memberships SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![status, command.now.as_str(), command.membership_id.as_str()],
            )
            .map_err(sqlite_storage)?;
        }
        if let Some(roles) = command.roles.as_ref() {
            replace_roles_sqlite(
                &tx,
                &command.membership_id,
                roles,
                &command.actor.identity_id,
                &command.now,
                true,
            )?;
        }
        ensure_active_owner_sqlite(&tx)?;
        append_platform_audit_sqlite(
            &tx,
            &command.actor,
            "platform_membership.updated",
            &command.membership_id,
            &serde_json::json!({
                "status": command.status,
                "roles": command.roles,
            }),
            &command.now,
        )?;
        tx.commit().map_err(sqlite_storage)?;
        Ok(())
    }

    pub(crate) fn revoke(
        &self,
        command: RevokePlatformMembership,
    ) -> Result<(), PlatformMembershipMutationError> {
        if command.actor.membership_id == command.membership_id {
            return Err(PlatformMembershipMutationError::SelfRevoke);
        }
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;

        let owner_grant = membership_has_owner_grant_sqlite(&tx, &command.membership_id)?
            .ok_or(PlatformMembershipMutationError::MembershipNotFound)?;
        if owner_grant
            && (!command.actor_is_platform_owner
                || !actor_has_active_owner_sqlite(&tx, &command.actor)?)
        {
            return Err(PlatformMembershipMutationError::OwnerAuthorityRequired);
        }
        let changed = tx
            .execute(
                "UPDATE platform_memberships SET status = 'revoked', updated_at = ?1
                 WHERE id = ?2 AND status != 'revoked'",
                params![command.now.as_str(), command.membership_id.as_str()],
            )
            .map_err(sqlite_storage)?;
        if changed == 0 {
            return Err(PlatformMembershipMutationError::MembershipNotFound);
        }
        ensure_active_owner_sqlite(&tx)?;
        append_platform_audit_sqlite(
            &tx,
            &command.actor,
            "platform_membership.revoked",
            &command.membership_id,
            &serde_json::json!({ "status": "revoked" }),
            &command.now,
        )?;
        tx.commit().map_err(sqlite_storage)?;
        Ok(())
    }
}

fn actor_has_active_owner_sqlite(
    tx: &Transaction<'_>,
    actor: &PlatformMembershipAuditActor,
) -> Result<bool, PlatformMembershipMutationError> {
    tx.query_row(
        "SELECT EXISTS(
             SELECT 1
             FROM platform_memberships pm
             JOIN platform_role_grants prg ON prg.platform_membership_id = pm.id
             WHERE pm.id = ?1
               AND pm.identity_id = ?2
               AND pm.status = 'active'
               AND prg.role = 'platform_owner'
         )",
        params![actor.membership_id, actor.identity_id],
        |row| row.get(0),
    )
    .map_err(sqlite_storage)
}

fn membership_has_owner_grant_sqlite(
    tx: &Transaction<'_>,
    membership_id: &str,
) -> Result<Option<bool>, PlatformMembershipMutationError> {
    tx.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM platform_role_grants
             WHERE platform_membership_id = ?1 AND role = 'platform_owner'
         )
         FROM platform_memberships WHERE id = ?1",
        [membership_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(sqlite_storage)
}

fn replace_roles_sqlite(
    tx: &Transaction<'_>,
    membership_id: &str,
    roles: &[String],
    granted_by_identity_id: &str,
    now: &str,
    delete_existing: bool,
) -> Result<(), PlatformMembershipMutationError> {
    if delete_existing {
        tx.execute(
            "DELETE FROM platform_role_grants WHERE platform_membership_id = ?1",
            [membership_id],
        )
        .map_err(sqlite_storage)?;
    }
    for role in roles {
        tx.execute(
            "INSERT INTO platform_role_grants
             (id, platform_membership_id, role, granted_by_identity_id, granted_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                Uuid::new_v4().to_string(),
                membership_id,
                role,
                granted_by_identity_id,
                now,
            ],
        )
        .map_err(sqlite_storage)?;
    }
    Ok(())
}

fn ensure_active_owner_sqlite(tx: &Transaction<'_>) -> Result<(), PlatformMembershipMutationError> {
    let owner_count: i64 = tx
        .query_row(
            "SELECT COUNT(*)
             FROM platform_memberships pm
             JOIN platform_role_grants prg ON prg.platform_membership_id = pm.id
             WHERE pm.status = 'active' AND prg.role = 'platform_owner'",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_storage)?;
    if owner_count == 0 {
        Err(PlatformMembershipMutationError::LastActiveOwner)
    } else {
        Ok(())
    }
}

fn append_platform_audit_sqlite(
    tx: &Transaction<'_>,
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
    tx.execute(
        "INSERT INTO audit_events
         (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
          platform_membership_id, roles_snapshot, capabilities_snapshot, action,
          resource_type, resource_id, correlation_id, detail_json, occurred_at)
         VALUES (?1, ?2, 'platform', NULL, NULL, ?3, ?4, ?5, ?6,
                 'platform_membership', ?7, ?1, ?8, ?9)",
        params![
            id,
            actor.identity_id,
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

fn sqlite_storage(error: rusqlite::Error) -> PlatformMembershipMutationError {
    PlatformMembershipMutationError::Storage(RepositoryError::Sqlite(error.to_string()))
}
