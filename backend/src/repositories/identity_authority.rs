use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
use system_core::{
    AuthorityContext, DataScope, PlatformMembershipId, PlatformRole, TenantId, TenantMembershipId,
    TenantRole,
};

use crate::auth_contract::{AuthUserInfo, IdentityAccount};
use crate::error::AppError;
use crate::utils::time::{shanghai_now_epoch_ms, shanghai_now_iso};

#[derive(Clone)]
pub(crate) struct SqliteIdentityAuthorityRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteIdentityAuthorityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn find_user_by_username(
        &self,
        scope: &DataScope,
        username: &str,
    ) -> Result<Option<IdentityAccount>, AppError> {
        let conn = self.pool.get()?;
        let result = conn
            .query_row(
                "SELECT i.id, i.username, i.password_hash, tm.role, i.status,
                        i.display_name, COALESCE(i.email, ''), COALESCE(i.phone, ''), tm.tenant_id
                 FROM identities i
                 JOIN tenant_memberships tm ON tm.identity_id = i.id
                 WHERE i.username = ?1 AND tm.tenant_id = ?2
                   AND tm.status = 'active'
                 LIMIT 1",
                params![username, scope.tenant_id().as_str()],
                |row| {
                    let role: String = row.get(3)?;
                    parse_tenant_role(&role).map_err(|_| rusqlite::Error::InvalidQuery)?;
                    Ok(IdentityAccount {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        password_hash: row.get(2)?,
                        authority_role: role,
                        is_enabled: row.get::<_, String>(4)? == "active",
                        display_name: row.get(5)?,
                        email: row.get(6)?,
                        phone: row.get(7)?,
                        tenant_scope_id: row.get(8)?,
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    pub(crate) fn find_platform_identity_by_username(
        &self,
        username: &str,
    ) -> Result<Option<IdentityAccount>, AppError> {
        let conn = self.pool.get()?;
        let identity: Option<(
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
        )> = conn
            .query_row(
                "SELECT i.id, i.username, i.password_hash, i.status, i.display_name,
                        COALESCE(i.email, ''), COALESCE(i.phone, ''), pm.id
                 FROM identities i
                 JOIN platform_memberships pm ON pm.identity_id = i.id
                 WHERE i.username = ?1 AND pm.status = 'active'
                 LIMIT 1",
                params![username],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, username, password_hash, status, display_name, email, phone, membership_id)) =
            identity
        else {
            return Ok(None);
        };
        let roles = platform_roles_sqlite(&conn, &membership_id)?;
        let Some(primary) = roles.first().copied() else {
            return Ok(None);
        };
        Ok(Some(IdentityAccount {
            id,
            username,
            password_hash,
            authority_role: platform_role_name(primary).to_owned(),
            is_enabled: status == "active",
            display_name,
            email,
            phone,
            tenant_scope_id: None,
        }))
    }

    pub(crate) fn find_user_by_id(
        &self,
        scope: &DataScope,
        user_id: &str,
    ) -> Result<Option<IdentityAccount>, AppError> {
        let conn = self.pool.get()?;
        let result = conn
            .query_row(
                "SELECT i.id, i.username, i.password_hash, tm.role, i.status,
                        i.display_name, COALESCE(i.email, ''), COALESCE(i.phone, ''), tm.tenant_id
                 FROM identities i
                 JOIN tenant_memberships tm ON tm.identity_id = i.id
                 WHERE i.id = ?1 AND tm.tenant_id = ?2
                   AND tm.status = 'active'
                 LIMIT 1",
                params![user_id, scope.tenant_id().as_str()],
                |row| {
                    let role: String = row.get(3)?;
                    parse_tenant_role(&role).map_err(|_| rusqlite::Error::InvalidQuery)?;
                    Ok(IdentityAccount {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        password_hash: row.get(2)?,
                        authority_role: role,
                        is_enabled: row.get::<_, String>(4)? == "active",
                        display_name: row.get(5)?,
                        email: row.get(6)?,
                        phone: row.get(7)?,
                        tenant_scope_id: row.get(8)?,
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    pub(crate) fn find_identity_by_id(
        &self,
        identity_id: &str,
    ) -> Result<Option<IdentityAccount>, AppError> {
        let conn = self.pool.get()?;
        let result = conn
            .query_row(
                "SELECT id, username, password_hash, status, display_name,
                        COALESCE(email, ''), COALESCE(phone, '')
                 FROM identities
                 WHERE id = ?1
                 LIMIT 1",
                [identity_id],
                |row| {
                    Ok(IdentityAccount {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        password_hash: row.get(2)?,
                        authority_role: String::new(),
                        is_enabled: row.get::<_, String>(3)? == "active",
                        display_name: row.get(4)?,
                        email: row.get(5)?,
                        phone: row.get(6)?,
                        tenant_scope_id: None,
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    pub(crate) fn record_successful_login(
        &self,
        identity_id: &str,
        at: &str,
    ) -> Result<(), AppError> {
        let conn = self.pool.get()?;
        let changed = conn.execute(
            "UPDATE identities
             SET last_login_at = ?1, updated_at = ?1
             WHERE id = ?2 AND status = 'active'",
            params![at, identity_id],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound("Identity 不存在或不可用".into()));
        }
        Ok(())
    }

    pub(crate) fn update_identity_profile(
        &self,
        identity_id: &str,
        display_name: Option<&str>,
        email: Option<&str>,
        phone: Option<&str>,
        updated_at: &str,
    ) -> Result<IdentityAccount, AppError> {
        let conn = self.pool.get()?;
        let changed = conn.execute(
            "UPDATE identities
             SET display_name = COALESCE(?1, display_name),
                 email = COALESCE(?2, email),
                 phone = COALESCE(?3, phone),
                 updated_at = ?4
             WHERE id = ?5 AND status = 'active'",
            params![display_name, email, phone, updated_at, identity_id],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound("Identity 不存在或不可用".into()));
        }
        drop(conn);
        self.find_identity_by_id(identity_id)?
            .ok_or_else(|| AppError::NotFound("Identity 不存在".into()))
    }

    pub(crate) fn get_auth_user(
        &self,
        session_token: &str,
        tenant_id: Option<&str>,
    ) -> Result<Option<AuthUserInfo>, AppError> {
        if session_token.is_empty() {
            return Ok(None);
        }
        let token_hash = session_token_hash(session_token);
        let conn = self.pool.get()?;
        let identity: Option<(String, String, String, String, String, String, String)> = conn
            .query_row(
                "SELECT i.id, i.username, i.status, i.display_name,
                        COALESCE(i.email, ''), COALESCE(i.phone, ''), s.expires_at
                 FROM auth_sessions s
                 JOIN identities i ON i.id = s.identity_id
                 WHERE s.token_hash = ?1 AND s.revoked_at IS NULL
                 LIMIT 1",
                params![token_hash],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((identity_id, username, status, display_name, email, phone, expires_at)) =
            identity
        else {
            return Ok(None);
        };

        if status != "active"
            || crate::utils::time::parse_date_time_to_epoch_ms(&expires_at)
                .is_none_or(|expires| shanghai_now_epoch_ms() >= expires)
        {
            conn.execute(
                "DELETE FROM auth_sessions WHERE token_hash = ?1",
                params![token_hash],
            )?;
            return Ok(None);
        }

        let authority = if let Some(tenant_id) = tenant_id {
            let membership: Option<(String, String)> = conn
                .query_row(
                    "SELECT id, role
                     FROM tenant_memberships
                     WHERE identity_id = ?1 AND tenant_id = ?2 AND status = 'active'
                     LIMIT 1",
                    params![identity_id, tenant_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((membership_id, role_value)) = membership else {
                return Ok(None);
            };
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new(membership_id)
                    .map_err(AppError::Internal)?,
                tenant_id: TenantId::new(tenant_id).map_err(AppError::Internal)?,
                role: parse_tenant_role(&role_value)?,
            }
        } else {
            let membership_id: Option<String> = conn
                .query_row(
                    "SELECT id FROM platform_memberships
                     WHERE identity_id = ?1 AND status = 'active'
                     LIMIT 1",
                    params![identity_id],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(membership_id) = membership_id else {
                return Ok(None);
            };
            let roles = platform_roles_sqlite(&conn, &membership_id)?;
            if roles.is_empty() {
                return Ok(None);
            }
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new(membership_id)
                    .map_err(AppError::Internal)?,
                roles,
            }
        };

        conn.execute(
            "UPDATE auth_sessions SET last_seen_at = ?1 WHERE token_hash = ?2",
            params![shanghai_now_iso(), token_hash],
        )?;

        Ok(Some(AuthUserInfo {
            id: identity_id,
            username,
            session_id: session_token.to_owned(),
            display_name,
            email,
            phone,
            authority,
        }))
    }
}

fn platform_roles_sqlite(
    conn: &rusqlite::Connection,
    membership_id: &str,
) -> Result<Vec<PlatformRole>, AppError> {
    let mut statement = conn.prepare(
        "SELECT role
         FROM platform_role_grants
         WHERE platform_membership_id = ?1
         ORDER BY role",
    )?;
    statement
        .query_map(params![membership_id], |row| row.get::<_, String>(0))?
        .map(|row| {
            let value = row?;
            parse_platform_role(&value)
        })
        .collect()
}

pub(crate) fn parse_tenant_role(value: &str) -> Result<TenantRole, AppError> {
    match value {
        "staff" => Ok(TenantRole::Staff),
        "admin" => Ok(TenantRole::Admin),
        "owner" => Ok(TenantRole::Owner),
        _ => Err(AppError::Internal(
            "invalid tenant role in identity authority".into(),
        )),
    }
}

pub(crate) fn parse_platform_role(value: &str) -> Result<PlatformRole, AppError> {
    match value {
        "platform_owner" => Ok(PlatformRole::Owner),
        "platform_admin" => Ok(PlatformRole::Admin),
        "platform_operator" => Ok(PlatformRole::Operator),
        "support_engineer" => Ok(PlatformRole::SupportEngineer),
        "business_operator" => Ok(PlatformRole::BusinessOperator),
        "security_auditor" => Ok(PlatformRole::SecurityAuditor),
        _ => Err(AppError::Internal(
            "invalid platform role in identity authority".into(),
        )),
    }
}

pub(crate) fn platform_role_name(role: PlatformRole) -> &'static str {
    match role {
        PlatformRole::Owner => "platform_owner",
        PlatformRole::Admin => "platform_admin",
        PlatformRole::Operator => "platform_operator",
        PlatformRole::SupportEngineer => "support_engineer",
        PlatformRole::BusinessOperator => "business_operator",
        PlatformRole::SecurityAuditor => "security_auditor",
    }
}

fn session_token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}
