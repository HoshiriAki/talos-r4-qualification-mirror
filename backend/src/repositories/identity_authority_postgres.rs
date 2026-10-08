use std::future::Future;

use sha2::{Digest, Sha256};
use sqlx::Row;
use sqlx::postgres::PgPool;
use system_core::{
    AuthorityContext, DataScope, PlatformMembershipId, PlatformRole, TenantId, TenantMembershipId,
};
use tokio::runtime::{Handle, RuntimeFlavor};

use crate::auth_contract::{AuthUserInfo, IdentityAccount};
use crate::error::AppError;
use crate::utils::time::{shanghai_now_epoch_ms, shanghai_now_iso};

use super::identity_authority::{parse_platform_role, parse_tenant_role, platform_role_name};

#[derive(Clone)]
pub(crate) struct PostgresIdentityAuthorityRepository {
    pool: PgPool,
}

impl PostgresIdentityAuthorityRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn find_user_by_username(
        &self,
        scope: &DataScope,
        username: &str,
    ) -> Result<Option<IdentityAccount>, AppError> {
        let pool = self.pool.clone();
        let tenant_id = scope.tenant_id().as_str().to_owned();
        let username = username.to_owned();
        run_pg_identity(async move {
            let row = sqlx::query(
                "SELECT i.id, i.username, i.password_hash, tm.role, i.status,
                        i.display_name, COALESCE(i.email, ''), COALESCE(i.phone, ''), tm.tenant_id
                 FROM identities i
                 JOIN tenant_memberships tm ON tm.identity_id = i.id
                 WHERE i.username = $1 AND tm.tenant_id = $2
                   AND tm.status = 'active'
                 LIMIT 1",
            )
            .bind(username)
            .bind(tenant_id)
            .fetch_optional(&pool)
            .await
            .map_err(pg_identity_error)?;
            row.map(identity_account_from_tenant_row).transpose()
        })
    }

    pub(crate) fn find_platform_identity_by_username(
        &self,
        username: &str,
    ) -> Result<Option<IdentityAccount>, AppError> {
        let pool = self.pool.clone();
        let username = username.to_owned();
        run_pg_identity(async move {
            let row = sqlx::query(
                "SELECT i.id, i.username, i.password_hash, i.status, i.display_name,
                        COALESCE(i.email, ''), COALESCE(i.phone, ''), pm.id
                 FROM identities i
                 JOIN platform_memberships pm ON pm.identity_id = i.id
                 WHERE i.username = $1 AND pm.status = 'active'
                 LIMIT 1",
            )
            .bind(username)
            .fetch_optional(&pool)
            .await
            .map_err(pg_identity_error)?;
            let Some(row) = row else {
                return Ok(None);
            };
            let membership_id: String = row.try_get(7).map_err(pg_identity_error)?;
            let role_values = sqlx::query_scalar::<_, String>(
                "SELECT role
                 FROM platform_role_grants
                 WHERE platform_membership_id = $1
                 ORDER BY role",
            )
            .bind(&membership_id)
            .fetch_all(&pool)
            .await
            .map_err(pg_identity_error)?;
            let roles = role_values
                .iter()
                .map(|value| parse_platform_role(value))
                .collect::<Result<Vec<PlatformRole>, AppError>>()?;
            let Some(primary) = roles.first().copied() else {
                return Ok(None);
            };
            Ok(Some(IdentityAccount {
                id: row.try_get(0).map_err(pg_identity_error)?,
                username: row.try_get(1).map_err(pg_identity_error)?,
                password_hash: row.try_get(2).map_err(pg_identity_error)?,
                authority_role: platform_role_name(primary).to_owned(),
                is_enabled: row.try_get::<String, _>(3).map_err(pg_identity_error)? == "active",
                display_name: row.try_get(4).map_err(pg_identity_error)?,
                email: row.try_get(5).map_err(pg_identity_error)?,
                phone: row.try_get(6).map_err(pg_identity_error)?,
                tenant_scope_id: None,
            }))
        })
    }

    pub(crate) fn find_user_by_id(
        &self,
        scope: &DataScope,
        user_id: &str,
    ) -> Result<Option<IdentityAccount>, AppError> {
        let pool = self.pool.clone();
        let tenant_id = scope.tenant_id().as_str().to_owned();
        let user_id = user_id.to_owned();
        run_pg_identity(async move {
            let row = sqlx::query(
                "SELECT i.id, i.username, i.password_hash, tm.role, i.status,
                        i.display_name, COALESCE(i.email, ''), COALESCE(i.phone, ''), tm.tenant_id
                 FROM identities i
                 JOIN tenant_memberships tm ON tm.identity_id = i.id
                 WHERE i.id = $1 AND tm.tenant_id = $2
                   AND tm.status = 'active'
                 LIMIT 1",
            )
            .bind(user_id)
            .bind(tenant_id)
            .fetch_optional(&pool)
            .await
            .map_err(pg_identity_error)?;
            row.map(identity_account_from_tenant_row).transpose()
        })
    }

    pub(crate) fn find_identity_by_id(
        &self,
        identity_id: &str,
    ) -> Result<Option<IdentityAccount>, AppError> {
        let pool = self.pool.clone();
        let identity_id = identity_id.to_owned();
        run_pg_identity(async move {
            let row = sqlx::query(
                "SELECT id, username, password_hash, status, display_name,
                        COALESCE(email, ''), COALESCE(phone, '')
                 FROM identities
                 WHERE id = $1
                 LIMIT 1",
            )
            .bind(identity_id)
            .fetch_optional(&pool)
            .await
            .map_err(pg_identity_error)?;
            row.map(|row| {
                Ok(IdentityAccount {
                    id: row.try_get(0).map_err(pg_identity_error)?,
                    username: row.try_get(1).map_err(pg_identity_error)?,
                    password_hash: row.try_get(2).map_err(pg_identity_error)?,
                    authority_role: String::new(),
                    is_enabled: row.try_get::<String, _>(3).map_err(pg_identity_error)? == "active",
                    display_name: row.try_get(4).map_err(pg_identity_error)?,
                    email: row.try_get(5).map_err(pg_identity_error)?,
                    phone: row.try_get(6).map_err(pg_identity_error)?,
                    tenant_scope_id: None,
                })
            })
            .transpose()
        })
    }

    pub(crate) fn record_successful_login(
        &self,
        identity_id: &str,
        at: &str,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        let identity_id = identity_id.to_owned();
        let at = at.to_owned();
        run_pg_identity(async move {
            let changed = sqlx::query(
                "UPDATE identities
                 SET last_login_at = $1, updated_at = $1
                 WHERE id = $2 AND status = 'active'",
            )
            .bind(at)
            .bind(identity_id)
            .execute(&pool)
            .await
            .map_err(pg_identity_error)?
            .rows_affected();
            if changed == 0 {
                return Err(AppError::NotFound("Identity 不存在或不可用".into()));
            }
            Ok(())
        })
    }

    pub(crate) fn update_identity_profile(
        &self,
        identity_id: &str,
        display_name: Option<&str>,
        email: Option<&str>,
        phone: Option<&str>,
        updated_at: &str,
    ) -> Result<IdentityAccount, AppError> {
        let pool = self.pool.clone();
        let identity_id = identity_id.to_owned();
        let display_name = display_name.map(str::to_owned);
        let email = email.map(str::to_owned);
        let phone = phone.map(str::to_owned);
        let updated_at = updated_at.to_owned();
        run_pg_identity(async move {
            let row = sqlx::query(
                "UPDATE identities
                 SET display_name = COALESCE($1, display_name),
                     email = COALESCE($2, email),
                     phone = COALESCE($3, phone),
                     updated_at = $4
                 WHERE id = $5 AND status = 'active'
                 RETURNING id, username, password_hash, status, display_name,
                           COALESCE(email, ''), COALESCE(phone, '')",
            )
            .bind(display_name)
            .bind(email)
            .bind(phone)
            .bind(updated_at)
            .bind(identity_id)
            .fetch_optional(&pool)
            .await
            .map_err(pg_identity_error)?;
            let Some(row) = row else {
                return Err(AppError::NotFound("Identity 不存在或不可用".into()));
            };
            Ok(IdentityAccount {
                id: row.try_get(0).map_err(pg_identity_error)?,
                username: row.try_get(1).map_err(pg_identity_error)?,
                password_hash: row.try_get(2).map_err(pg_identity_error)?,
                authority_role: String::new(),
                is_enabled: row.try_get::<String, _>(3).map_err(pg_identity_error)? == "active",
                display_name: row.try_get(4).map_err(pg_identity_error)?,
                email: row.try_get(5).map_err(pg_identity_error)?,
                phone: row.try_get(6).map_err(pg_identity_error)?,
                tenant_scope_id: None,
            })
        })
    }

    pub(crate) fn get_auth_user(
        &self,
        session_token: &str,
        tenant_id: Option<&str>,
    ) -> Result<Option<AuthUserInfo>, AppError> {
        if session_token.is_empty() {
            return Ok(None);
        }

        let pool = self.pool.clone();
        let session_token = session_token.to_owned();
        let tenant_id = tenant_id.map(str::to_owned);
        run_pg_identity(async move {
            let token_hash = session_token_hash(&session_token);
            let mut tx = pool.begin().await.map_err(pg_identity_error)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *tx)
                .await
                .map_err(pg_identity_error)?;

            let identity = sqlx::query(
                "SELECT i.id, i.username, i.status, i.display_name,
                        COALESCE(i.email, ''), COALESCE(i.phone, ''), s.expires_at
                 FROM auth_sessions s
                 JOIN identities i ON i.id = s.identity_id
                 WHERE s.token_hash = $1 AND s.revoked_at IS NULL
                 LIMIT 1
                 FOR UPDATE OF s",
            )
            .bind(&token_hash)
            .fetch_optional(&mut *tx)
            .await
            .map_err(pg_identity_error)?;

            let Some(identity) = identity else {
                tx.commit().await.map_err(pg_identity_error)?;
                return Ok(None);
            };

            let identity_id: String = identity.try_get(0).map_err(pg_identity_error)?;
            let username: String = identity.try_get(1).map_err(pg_identity_error)?;
            let status: String = identity.try_get(2).map_err(pg_identity_error)?;
            let display_name: String = identity.try_get(3).map_err(pg_identity_error)?;
            let email: String = identity.try_get(4).map_err(pg_identity_error)?;
            let phone: String = identity.try_get(5).map_err(pg_identity_error)?;
            let expires_at: String = identity.try_get(6).map_err(pg_identity_error)?;

            if status != "active"
                || crate::utils::time::parse_date_time_to_epoch_ms(&expires_at)
                    .is_none_or(|expires| shanghai_now_epoch_ms() >= expires)
            {
                sqlx::query("DELETE FROM auth_sessions WHERE token_hash = $1")
                    .bind(&token_hash)
                    .execute(&mut *tx)
                    .await
                    .map_err(pg_identity_error)?;
                tx.commit().await.map_err(pg_identity_error)?;
                return Ok(None);
            }

            let authority = if let Some(tenant_id) = tenant_id {
                let membership = sqlx::query(
                    "SELECT id, role
                     FROM tenant_memberships
                     WHERE identity_id = $1 AND tenant_id = $2 AND status = 'active'
                     LIMIT 1",
                )
                .bind(&identity_id)
                .bind(&tenant_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(pg_identity_error)?;
                let Some(membership) = membership else {
                    tx.commit().await.map_err(pg_identity_error)?;
                    return Ok(None);
                };
                let membership_id: String = membership.try_get(0).map_err(pg_identity_error)?;
                let role_value: String = membership.try_get(1).map_err(pg_identity_error)?;
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new(membership_id)
                        .map_err(AppError::Internal)?,
                    tenant_id: TenantId::new(tenant_id).map_err(AppError::Internal)?,
                    role: parse_tenant_role(&role_value)?,
                }
            } else {
                let membership_id = sqlx::query_scalar::<_, String>(
                    "SELECT id
                     FROM platform_memberships
                     WHERE identity_id = $1 AND status = 'active'
                     LIMIT 1",
                )
                .bind(&identity_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(pg_identity_error)?;
                let Some(membership_id) = membership_id else {
                    tx.commit().await.map_err(pg_identity_error)?;
                    return Ok(None);
                };
                let role_values = sqlx::query_scalar::<_, String>(
                    "SELECT role
                     FROM platform_role_grants
                     WHERE platform_membership_id = $1
                     ORDER BY role",
                )
                .bind(&membership_id)
                .fetch_all(&mut *tx)
                .await
                .map_err(pg_identity_error)?;
                let roles = role_values
                    .iter()
                    .map(|value| parse_platform_role(value))
                    .collect::<Result<Vec<PlatformRole>, AppError>>()?;
                if roles.is_empty() {
                    tx.commit().await.map_err(pg_identity_error)?;
                    return Ok(None);
                }
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new(membership_id)
                        .map_err(AppError::Internal)?,
                    roles,
                }
            };

            sqlx::query("UPDATE auth_sessions SET last_seen_at = $1 WHERE token_hash = $2")
                .bind(shanghai_now_iso())
                .bind(&token_hash)
                .execute(&mut *tx)
                .await
                .map_err(pg_identity_error)?;
            tx.commit().await.map_err(pg_identity_error)?;

            Ok(Some(AuthUserInfo {
                id: identity_id,
                username,
                session_id: session_token,
                display_name,
                email,
                phone,
                authority,
            }))
        })
    }
}

fn identity_account_from_tenant_row(
    row: sqlx::postgres::PgRow,
) -> Result<IdentityAccount, AppError> {
    let role: String = row.try_get(3).map_err(pg_identity_error)?;
    parse_tenant_role(&role)?;
    Ok(IdentityAccount {
        id: row.try_get(0).map_err(pg_identity_error)?,
        username: row.try_get(1).map_err(pg_identity_error)?,
        password_hash: row.try_get(2).map_err(pg_identity_error)?,
        authority_role: role,
        is_enabled: row.try_get::<String, _>(4).map_err(pg_identity_error)? == "active",
        display_name: row.try_get(5).map_err(pg_identity_error)?,
        email: row.try_get(6).map_err(pg_identity_error)?,
        phone: row.try_get(7).map_err(pg_identity_error)?,
        tenant_scope_id: row.try_get(8).map_err(pg_identity_error)?,
    })
}

fn session_token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn pg_identity_error<E>(_error: E) -> AppError {
    tracing::error!(
        error_class = "identity_postgres",
        "identity authority persistence operation failed"
    );
    AppError::ServiceError {
        code: "SYS_IDENTITY_PERSISTENCE".into(),
        message: "identity persistence unavailable".into(),
    }
}

fn run_pg_identity<T, F>(future: F) -> Result<T, AppError>
where
    F: Future<Output = Result<T, AppError>>,
{
    let handle = Handle::try_current().map_err(|_| AppError::ServiceError {
        code: "SYS_IDENTITY_RUNTIME".into(),
        message: "identity persistence runtime unavailable".into(),
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(AppError::ServiceError {
            code: "SYS_IDENTITY_RUNTIME".into(),
            message: "identity persistence requires the multi-thread runtime".into(),
        });
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}
