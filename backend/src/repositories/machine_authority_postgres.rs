use std::future::Future;

use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPool;
use sqlx::{Executor, Row};
use subtle::ConstantTimeEq;
use system_core::TenantRole;
use tokio::runtime::{Handle, RuntimeFlavor};
use uuid::Uuid;

use crate::error::AppError;

use super::machine_authority::{
    MachineAdminContext, MachineAuthorization, MachineClientProjection, MachineIssuedCredential,
    MachineProvisionRecord, MachineScopeRecord, audit_role_name, machine_role_name,
    machine_scope_allowed,
};

#[derive(Clone)]
pub(crate) struct PostgresMachineAuthorityRepository {
    pool: PgPool,
}

impl PostgresMachineAuthorityRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn provision(
        &self,
        admin: &MachineAdminContext,
        input: MachineProvisionRecord,
    ) -> Result<MachineIssuedCredential, AppError> {
        if input.role == TenantRole::Owner {
            return Err(AppError::BadRequest(
                "machine identity cannot receive owner authority".into(),
            ));
        }
        if input
            .scopes
            .iter()
            .any(|scope| !machine_scope_allowed(scope))
        {
            return Err(AppError::BadRequest(
                "scope requires an interactive principal".into(),
            ));
        }

        let pool = self.pool.clone();
        let admin = admin.clone();
        run_pg_machine(async move {
            let mut tx = pool.begin().await.map_err(pg_machine_error)?;
            set_serializable(&mut tx).await?;
            verify_live_admin(&mut tx, &admin).await?;

            let identity_id = Uuid::new_v4().to_string();
            let membership_id = Uuid::new_v4().to_string();
            let client_id = Uuid::new_v4().to_string();
            let now = Utc::now().to_rfc3339();

            sqlx::query(
                "INSERT INTO identities
                 (id,username,password_hash,display_name,status,created_at,updated_at)
                 VALUES ($1,$2,'!non-interactive',$3,'active',$4,$4)",
            )
            .bind(&identity_id)
            .bind(format!("machine-{identity_id}"))
            .bind(&input.name)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(pg_machine_error)?;

            sqlx::query(
                "INSERT INTO machine_identities (identity_id,created_at)
                 VALUES ($1,$2)",
            )
            .bind(&identity_id)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(pg_machine_error)?;

            sqlx::query(
                "INSERT INTO tenant_memberships
                 (id,identity_id,tenant_id,role,status,created_at,updated_at)
                 VALUES ($1,$2,$3,$4,'active',$5,$5)",
            )
            .bind(&membership_id)
            .bind(&identity_id)
            .bind(&admin.tenant_id)
            .bind(machine_role_name(input.role))
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(pg_machine_error)?;

            sqlx::query(
                "INSERT INTO api_clients
                 (id,identity_id,membership_id,tenant_id,name,status,api_version,
                  rate_limit_rpm,created_by,created_at)
                 VALUES ($1,$2,$3,$4,$5,'active','v1',$6,$7,$8)",
            )
            .bind(&client_id)
            .bind(&identity_id)
            .bind(&membership_id)
            .bind(&admin.tenant_id)
            .bind(&input.name)
            .bind(i32::from(input.rate_limit_rpm))
            .bind(&admin.identity_id)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(pg_machine_error)?;

            for scope in &input.scopes {
                sqlx::query(
                    "INSERT INTO api_scope_grants (client_id,module,command)
                     VALUES ($1,$2,$3)
                     ON CONFLICT (client_id,module,command) DO NOTHING",
                )
                .bind(&client_id)
                .bind(&scope.module)
                .bind(&scope.command)
                .execute(&mut *tx)
                .await
                .map_err(pg_machine_error)?;
            }

            let issued = issue_credential(&mut tx, &client_id).await?;
            append_machine_audit(
                &mut tx,
                Some(&admin),
                &client_id,
                &Uuid::new_v4().to_string(),
                "machine.provision",
                "issued",
            )
            .await?;
            tx.commit().await.map_err(pg_machine_error)?;
            Ok(issued)
        })
    }

    pub(crate) fn authorize(
        &self,
        token: &str,
        requested_tenant: &str,
        version: &str,
        scope: &MachineScopeRecord,
        correlation: &str,
    ) -> Result<MachineAuthorization, AppError> {
        let pool = self.pool.clone();
        let token = token.to_owned();
        let requested_tenant = requested_tenant.to_owned();
        let version = version.to_owned();
        let scope = scope.clone();
        let correlation = correlation.to_owned();

        run_pg_machine(async move {
            let mut tx = pool.begin().await.map_err(pg_machine_error)?;
            set_serializable(&mut tx).await?;

            let prefix = token
                .split_once('.')
                .map(|(prefix, _)| prefix)
                .unwrap_or("");
            let candidate = if token.len() <= 128 && prefix.len() == 37 {
                sqlx::query(
                    "SELECT c.id,c.identity_id,c.membership_id,c.tenant_id,m.role,k.key_hash,
                            i.status,m.status,c.status,k.status,k.expires_at,c.api_version,
                            c.rate_limit_rpm
                     FROM api_credentials k
                     JOIN api_clients c ON c.id=k.client_id
                     JOIN machine_identities mi ON mi.identity_id=c.identity_id
                     JOIN identities i ON i.id=c.identity_id
                     JOIN tenant_memberships m
                       ON m.id=c.membership_id
                      AND m.identity_id=c.identity_id
                      AND m.tenant_id=c.tenant_id
                     JOIN tenants t ON t.id=c.tenant_id
                     WHERE k.key_prefix=$1 AND t.status='active'
                     FOR UPDATE OF k,c,m,i",
                )
                .bind(prefix)
                .fetch_optional(&mut *tx)
                .await
                .map_err(pg_machine_error)?
            } else {
                None
            };

            let Some(candidate) = candidate else {
                append_machine_audit(
                    &mut tx,
                    None,
                    "unknown",
                    &correlation,
                    "machine.access",
                    "unauthorized",
                )
                .await?;
                tx.commit().await.map_err(pg_machine_error)?;
                return Err(AppError::Unauthorized);
            };

            let client_id: String = candidate.try_get(0).map_err(pg_machine_error)?;
            let identity_id: String = candidate.try_get(1).map_err(pg_machine_error)?;
            let membership_id: String = candidate.try_get(2).map_err(pg_machine_error)?;
            let tenant_id: String = candidate.try_get(3).map_err(pg_machine_error)?;
            let role_value: String = candidate.try_get(4).map_err(pg_machine_error)?;
            let expected_hash: String = candidate.try_get(5).map_err(pg_machine_error)?;
            let identity_status: String = candidate.try_get(6).map_err(pg_machine_error)?;
            let membership_status: String = candidate.try_get(7).map_err(pg_machine_error)?;
            let client_status: String = candidate.try_get(8).map_err(pg_machine_error)?;
            let credential_status: String = candidate.try_get(9).map_err(pg_machine_error)?;
            let expires_at: String = candidate.try_get(10).map_err(pg_machine_error)?;
            let api_version: String = candidate.try_get(11).map_err(pg_machine_error)?;
            let rate_limit_rpm: i32 = candidate.try_get(12).map_err(pg_machine_error)?;

            let valid_secret =
                bool::from(digest(&token).as_bytes().ct_eq(expected_hash.as_bytes()));
            let valid = valid_secret
                && identity_status == "active"
                && membership_status == "active"
                && client_status == "active"
                && credential_status == "active"
                && chrono::DateTime::parse_from_rfc3339(&expires_at)
                    .is_ok_and(|expires| expires > Utc::now());

            let actor = if valid_secret {
                Some(MachineAdminContext {
                    identity_id: identity_id.clone(),
                    membership_id: membership_id.clone(),
                    tenant_id: tenant_id.clone(),
                    role: parse_machine_role(&role_value)?,
                })
            } else {
                None
            };

            if !valid {
                append_machine_audit(
                    &mut tx,
                    actor.as_ref(),
                    if valid_secret { &client_id } else { "unknown" },
                    &correlation,
                    "machine.access",
                    "unauthorized",
                )
                .await?;
                tx.commit().await.map_err(pg_machine_error)?;
                return Err(AppError::Unauthorized);
            }

            let scope_allowed: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM api_scope_grants
                    WHERE client_id=$1 AND module=$2 AND command=$3
                 )",
            )
            .bind(&client_id)
            .bind(&scope.module)
            .bind(&scope.command)
            .fetch_one(&mut *tx)
            .await
            .map_err(pg_machine_error)?;

            if tenant_id != requested_tenant
                || version != api_version
                || !scope_allowed
                || !machine_scope_allowed(&scope)
            {
                append_machine_audit(
                    &mut tx,
                    actor.as_ref(),
                    &client_id,
                    &correlation,
                    "machine.access",
                    "policy_denied",
                )
                .await?;
                tx.commit().await.map_err(pg_machine_error)?;
                return Err(AppError::Forbidden);
            }

            let window = Utc::now().timestamp() / 60;
            let usage = sqlx::query(
                "SELECT window_start, used
                 FROM api_usage_windows
                 WHERE client_id=$1
                 FOR UPDATE",
            )
            .bind(&client_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(pg_machine_error)?;

            let used = usage
                .as_ref()
                .and_then(|row| {
                    let start: i64 = row.try_get(0).ok()?;
                    let used: i32 = row.try_get(1).ok()?;
                    (start >= window).then_some(used)
                })
                .unwrap_or(0);

            if used >= rate_limit_rpm {
                append_machine_audit(
                    &mut tx,
                    actor.as_ref(),
                    &client_id,
                    &correlation,
                    "machine.access",
                    "rate_limited",
                )
                .await?;
                tx.commit().await.map_err(pg_machine_error)?;
                return Err(AppError::RateLimited {
                    retry_after_secs: 60,
                });
            }

            let next_used = used.saturating_add(1);
            sqlx::query(
                "INSERT INTO api_usage_windows
                 (client_id,window_start,used,last_used_at)
                 VALUES ($1,$2,1,$3)
                 ON CONFLICT (client_id) DO UPDATE SET
                   used=$4,
                   window_start=GREATEST(api_usage_windows.window_start,EXCLUDED.window_start),
                   last_used_at=EXCLUDED.last_used_at",
            )
            .bind(&client_id)
            .bind(window)
            .bind(Utc::now().to_rfc3339())
            .bind(next_used)
            .execute(&mut *tx)
            .await
            .map_err(pg_machine_error)?;

            append_machine_audit(
                &mut tx,
                actor.as_ref(),
                &client_id,
                &correlation,
                "machine.access",
                "admitted",
            )
            .await?;

            let role = parse_machine_role(&role_value)?;
            tx.commit().await.map_err(pg_machine_error)?;

            Ok(MachineAuthorization {
                identity_id,
                membership_id,
                tenant_id,
                role,
            })
        })
    }

    pub(crate) fn lifecycle(
        &self,
        admin: &MachineAdminContext,
        client_id: &str,
        action: &str,
    ) -> Result<Option<MachineIssuedCredential>, AppError> {
        let pool = self.pool.clone();
        let admin = admin.clone();
        let client_id = client_id.to_owned();
        let action = action.to_owned();

        run_pg_machine(async move {
            let mut tx = pool.begin().await.map_err(pg_machine_error)?;
            set_serializable(&mut tx).await?;
            verify_live_admin(&mut tx, &admin).await?;

            let status = sqlx::query_scalar::<_, String>(
                "SELECT status FROM api_clients
                 WHERE id=$1 AND tenant_id=$2
                 FOR UPDATE",
            )
            .bind(&client_id)
            .bind(&admin.tenant_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(pg_machine_error)?
            .ok_or_else(|| AppError::NotFound("client not found".into()))?;

            let issued = match action.as_str() {
                "revoke" | "disable" => {
                    sqlx::query(
                        "UPDATE api_clients
                         SET status=$1
                         WHERE id=$2 AND status<>'revoked'",
                    )
                    .bind(if action == "revoke" {
                        "revoked"
                    } else {
                        "disabled"
                    })
                    .bind(&client_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(pg_machine_error)?;
                    None
                }
                "enable" if status != "revoked" => {
                    sqlx::query("UPDATE api_clients SET status='active' WHERE id=$1")
                        .bind(&client_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(pg_machine_error)?;
                    None
                }
                "rotate" if status == "active" => {
                    sqlx::query(
                        "UPDATE api_credentials
                         SET status='revoked'
                         WHERE client_id=$1",
                    )
                    .bind(&client_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(pg_machine_error)?;
                    Some(issue_credential(&mut tx, &client_id).await?)
                }
                _ => {
                    let _ = tx.rollback().await;
                    return Err(AppError::BadRequest("invalid lifecycle transition".into()));
                }
            };

            append_machine_audit(
                &mut tx,
                Some(&admin),
                &client_id,
                &Uuid::new_v4().to_string(),
                "machine.lifecycle",
                &action,
            )
            .await?;
            tx.commit().await.map_err(pg_machine_error)?;
            Ok(issued)
        })
    }

    pub(crate) fn list(
        &self,
        admin: &MachineAdminContext,
    ) -> Result<Vec<MachineClientProjection>, AppError> {
        let pool = self.pool.clone();
        let admin = admin.clone();

        run_pg_machine(async move {
            let mut tx = pool.begin().await.map_err(pg_machine_error)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ ONLY")
                .execute(&mut *tx)
                .await
                .map_err(pg_machine_error)?;
            verify_live_admin(&mut tx, &admin).await?;

            let rows = sqlx::query(
                "SELECT c.id,c.identity_id,c.name,c.status,c.api_version,c.rate_limit_rpm,
                        u.last_used_at,COALESCE(u.used,0)
                 FROM api_clients c
                 LEFT JOIN api_usage_windows u ON u.client_id=c.id
                 WHERE c.tenant_id=$1
                 ORDER BY c.created_at DESC
                 LIMIT 200",
            )
            .bind(&admin.tenant_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(pg_machine_error)?;

            let clients = rows
                .into_iter()
                .map(|row| {
                    Ok(MachineClientProjection {
                        client_id: row.try_get(0).map_err(pg_machine_error)?,
                        identity_id: row.try_get(1).map_err(pg_machine_error)?,
                        name: row.try_get(2).map_err(pg_machine_error)?,
                        status: row.try_get(3).map_err(pg_machine_error)?,
                        api_version: row.try_get(4).map_err(pg_machine_error)?,
                        rate_limit_rpm: row
                            .try_get::<i32, _>(5)
                            .map_err(pg_machine_error)?
                            .try_into()
                            .map_err(|_| AppError::Internal("invalid machine RPM".into()))?,
                        last_used_at: row.try_get(6).map_err(pg_machine_error)?,
                        window_usage: row
                            .try_get::<i32, _>(7)
                            .map_err(pg_machine_error)?
                            .try_into()
                            .map_err(|_| AppError::Internal("invalid machine usage".into()))?,
                    })
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            tx.commit().await.map_err(pg_machine_error)?;
            Ok(clients)
        })
    }

    pub(crate) fn credential_lifecycle(
        &self,
        admin: &MachineAdminContext,
        client_id: &str,
        credential_id: &str,
        action: &str,
    ) -> Result<(), AppError> {
        let status = match action {
            "disable" => "disabled",
            "enable" => "active",
            "revoke" => "revoked",
            _ => {
                return Err(AppError::BadRequest("invalid credential action".into()));
            }
        };

        let pool = self.pool.clone();
        let admin = admin.clone();
        let client_id = client_id.to_owned();
        let credential_id = credential_id.to_owned();
        let action = action.to_owned();
        let status = status.to_owned();

        run_pg_machine(async move {
            let mut tx = pool.begin().await.map_err(pg_machine_error)?;
            set_serializable(&mut tx).await?;
            verify_live_admin(&mut tx, &admin).await?;

            let affected = sqlx::query(
                "UPDATE api_credentials
                 SET status=$1
                 WHERE id=$2 AND client_id=$3
                   AND status<>'revoked'
                   AND EXISTS(
                     SELECT 1 FROM api_clients
                     WHERE id=$3 AND tenant_id=$4 AND status<>'revoked'
                   )",
            )
            .bind(&status)
            .bind(&credential_id)
            .bind(&client_id)
            .bind(&admin.tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_machine_error)?
            .rows_affected();

            if affected != 1 {
                let _ = tx.rollback().await;
                return Err(AppError::NotFound("active credential not found".into()));
            }

            append_machine_audit(
                &mut tx,
                Some(&admin),
                &client_id,
                &Uuid::new_v4().to_string(),
                "machine.credential",
                &action,
            )
            .await?;
            tx.commit().await.map_err(pg_machine_error)?;
            Ok(())
        })
    }
}

async fn verify_live_admin(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &MachineAdminContext,
) -> Result<(), AppError> {
    if !matches!(admin.role, TenantRole::Admin | TenantRole::Owner) {
        return Err(AppError::Forbidden);
    }

    let live: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM tenant_memberships m
            JOIN identities i ON i.id=m.identity_id
            JOIN tenants t ON t.id=m.tenant_id
            WHERE m.id=$1
              AND m.identity_id=$2
              AND m.tenant_id=$3
              AND m.status='active'
              AND m.role IN ('admin','owner')
              AND i.status='active'
              AND t.status='active'
         )",
    )
    .bind(&admin.membership_id)
    .bind(&admin.identity_id)
    .bind(&admin.tenant_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(pg_machine_error)?;

    if live {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

async fn issue_credential(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    client_id: &str,
) -> Result<MachineIssuedCredential, AppError> {
    let credential_id = Uuid::new_v4().to_string();
    let prefix = format!("mapi_{}", Uuid::new_v4().simple());
    let random: [u8; 32] = rand::random();
    let secret = format!("{prefix}.{}", hex::encode(random));
    let expires_at = (Utc::now() + Duration::days(90)).to_rfc3339();

    sqlx::query(
        "INSERT INTO api_credentials
         (id,client_id,key_prefix,key_hash,status,expires_at,created_at)
         VALUES ($1,$2,$3,$4,'active',$5,$6)",
    )
    .bind(&credential_id)
    .bind(client_id)
    .bind(&prefix)
    .bind(digest(&secret))
    .bind(&expires_at)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(pg_machine_error)?;

    Ok(MachineIssuedCredential {
        client_id: client_id.to_owned(),
        credential_id,
        secret,
        expires_at,
    })
}

async fn append_machine_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: Option<&MachineAdminContext>,
    client_id: &str,
    correlation_id: &str,
    action: &str,
    outcome: &str,
) -> Result<(), AppError> {
    let actor_identity_id = actor.map(|actor| actor.identity_id.as_str());
    let tenant_id = actor.map(|actor| actor.tenant_id.as_str());
    let membership_id = actor.map(|actor| actor.membership_id.as_str());
    let role = actor.map(|actor| audit_role_name(actor.role)).unwrap_or("");
    let authority_kind = if actor.is_some() { "tenant" } else { "system" };

    sqlx::query(
        "INSERT INTO audit_events
         (id,actor_identity_id,authority_kind,tenant_id,tenant_membership_id,
          platform_membership_id,roles_snapshot,capabilities_snapshot,action,
          resource_type,resource_id,correlation_id,detail_json,occurred_at)
         VALUES ($1,$2,$3,$4,$5,NULL,$6::jsonb,'[]'::jsonb,$7,
                 'api_client',$8,$9,$10::jsonb,$11)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(actor_identity_id)
    .bind(authority_kind)
    .bind(tenant_id)
    .bind(membership_id)
    .bind(serde_json::json!([role]).to_string())
    .bind(action)
    .bind(client_id)
    .bind(correlation_id)
    .bind(
        serde_json::json!({
            "client_id": client_id,
            "outcome": outcome
        })
        .to_string(),
    )
    .bind(Utc::now().to_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(pg_machine_error)?;
    Ok(())
}

async fn set_serializable(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), AppError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut **tx)
        .await
        .map_err(pg_machine_error)?;
    Ok(())
}

fn parse_machine_role(value: &str) -> Result<TenantRole, AppError> {
    match value {
        "staff" => Ok(TenantRole::Staff),
        "admin" => Ok(TenantRole::Admin),
        _ => Err(AppError::Forbidden),
    }
}

fn digest(secret: &str) -> String {
    hex::encode(Sha256::digest(secret.as_bytes()))
}

fn pg_machine_error<E>(_error: E) -> AppError {
    tracing::error!(
        error_class = "machine_postgres",
        "machine authority persistence operation failed"
    );
    AppError::ServiceError {
        code: "SYS_MACHINE_PERSISTENCE".into(),
        message: "machine authority persistence unavailable".into(),
    }
}

fn run_pg_machine<T, F>(future: F) -> Result<T, AppError>
where
    T: Send,
    F: Future<Output = Result<T, AppError>> + Send,
{
    let handle = Handle::try_current().map_err(|_| AppError::ServiceError {
        code: "SYS_MACHINE_RUNTIME".into(),
        message: "machine authority runtime unavailable".into(),
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(AppError::ServiceError {
            code: "SYS_MACHINE_RUNTIME".into(),
            message: "machine authority requires the multi-thread runtime".into(),
        });
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}
