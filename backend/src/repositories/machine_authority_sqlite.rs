use chrono::{Duration, Utc};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use system_core::TenantRole;
use uuid::Uuid;

use crate::error::AppError;

use super::machine_authority::{
    MachineAdminContext, MachineAuthorization, MachineClientProjection, MachineIssuedCredential,
    MachineProvisionRecord, MachineScopeRecord, audit_role_name, machine_role_name,
    machine_scope_allowed,
};

#[derive(Clone)]
pub(crate) struct SqliteMachineAuthorityRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteMachineAuthorityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
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

        let mut connection = self.pool.get()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        verify_live_admin(&tx, admin)?;

        let identity_id = Uuid::new_v4().to_string();
        let membership_id = Uuid::new_v4().to_string();
        let client_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        tx.execute(
            "INSERT INTO identities
             (id,username,password_hash,display_name,status,created_at,updated_at)
             VALUES (?1,?2,'!non-interactive',?3,'active',?4,?4)",
            params![
                identity_id,
                format!("machine-{identity_id}"),
                input.name,
                now
            ],
        )?;
        tx.execute(
            "INSERT INTO machine_identities (identity_id,created_at)
             VALUES (?1,?2)",
            params![identity_id, now],
        )?;
        tx.execute(
            "INSERT INTO tenant_memberships
             (id,identity_id,tenant_id,role,status,created_at,updated_at)
             VALUES (?1,?2,?3,?4,'active',?5,?5)",
            params![
                membership_id,
                identity_id,
                admin.tenant_id,
                machine_role_name(input.role),
                now
            ],
        )?;
        tx.execute(
            "INSERT INTO api_clients
             (id,identity_id,membership_id,tenant_id,name,status,api_version,
              rate_limit_rpm,created_by,created_at)
             VALUES (?1,?2,?3,?4,?5,'active','v1',?6,?7,?8)",
            params![
                client_id,
                identity_id,
                membership_id,
                admin.tenant_id,
                input.name,
                input.rate_limit_rpm,
                admin.identity_id,
                now
            ],
        )?;

        for scope in &input.scopes {
            tx.execute(
                "INSERT OR IGNORE INTO api_scope_grants
                 (client_id,module,command)
                 VALUES (?1,?2,?3)",
                params![client_id, scope.module, scope.command],
            )?;
        }

        let issued = issue_credential(&tx, &client_id)?;
        append_machine_audit(
            &tx,
            Some(admin),
            &client_id,
            &Uuid::new_v4().to_string(),
            "machine.provision",
            "issued",
        )?;
        tx.commit()?;
        Ok(issued)
    }

    pub(crate) fn authorize(
        &self,
        token: &str,
        requested_tenant: &str,
        version: &str,
        scope: &MachineScopeRecord,
        correlation: &str,
    ) -> Result<MachineAuthorization, AppError> {
        let mut connection = self.pool.get()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prefix = token
            .split_once('.')
            .map(|(prefix, _)| prefix)
            .unwrap_or("");

        let candidate = if token.len() <= 128 && prefix.len() == 37 {
            tx.query_row(
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
                 WHERE k.key_prefix=?1 AND t.status='active'",
                [prefix],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, String>(11)?,
                        row.get::<_, u32>(12)?,
                    ))
                },
            )
            .optional()?
        } else {
            None
        };

        let Some((
            client_id,
            identity_id,
            membership_id,
            tenant_id,
            role_value,
            expected_hash,
            identity_status,
            membership_status,
            client_status,
            credential_status,
            expires_at,
            api_version,
            rate_limit_rpm,
        )) = candidate
        else {
            append_machine_audit(
                &tx,
                None,
                "unknown",
                correlation,
                "machine.access",
                "unauthorized",
            )?;
            tx.commit()?;
            return Err(AppError::Unauthorized);
        };

        let valid_secret = bool::from(digest(token).as_bytes().ct_eq(expected_hash.as_bytes()));
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
                &tx,
                actor.as_ref(),
                if valid_secret { &client_id } else { "unknown" },
                correlation,
                "machine.access",
                "unauthorized",
            )?;
            tx.commit()?;
            return Err(AppError::Unauthorized);
        }

        let scope_allowed: bool = tx.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM api_scope_grants
                WHERE client_id=?1 AND module=?2 AND command=?3
             )",
            params![client_id, scope.module, scope.command],
            |row| row.get(0),
        )?;

        if tenant_id != requested_tenant
            || version != api_version
            || !scope_allowed
            || !machine_scope_allowed(scope)
        {
            append_machine_audit(
                &tx,
                actor.as_ref(),
                &client_id,
                correlation,
                "machine.access",
                "policy_denied",
            )?;
            tx.commit()?;
            return Err(AppError::Forbidden);
        }

        let window = Utc::now().timestamp() / 60;
        let used: u32 = tx
            .query_row(
                "SELECT used FROM api_usage_windows
                 WHERE client_id=?1 AND window_start>=?2",
                params![client_id, window],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);

        if used >= rate_limit_rpm {
            append_machine_audit(
                &tx,
                actor.as_ref(),
                &client_id,
                correlation,
                "machine.access",
                "rate_limited",
            )?;
            tx.commit()?;
            return Err(AppError::RateLimited {
                retry_after_secs: 60,
            });
        }

        tx.execute(
            "INSERT INTO api_usage_windows
             (client_id,window_start,used,last_used_at)
             VALUES (?1,?2,1,?3)
             ON CONFLICT(client_id) DO UPDATE SET
               used=?4,
               window_start=MAX(window_start,excluded.window_start),
               last_used_at=excluded.last_used_at",
            params![client_id, window, Utc::now().to_rfc3339(), used + 1],
        )?;
        append_machine_audit(
            &tx,
            actor.as_ref(),
            &client_id,
            correlation,
            "machine.access",
            "admitted",
        )?;

        let role = parse_machine_role(&role_value)?;
        tx.commit()?;
        Ok(MachineAuthorization {
            identity_id,
            membership_id,
            tenant_id,
            role,
        })
    }

    pub(crate) fn lifecycle(
        &self,
        admin: &MachineAdminContext,
        client_id: &str,
        action: &str,
    ) -> Result<Option<MachineIssuedCredential>, AppError> {
        let mut connection = self.pool.get()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        verify_live_admin(&tx, admin)?;

        let status = tx
            .query_row(
                "SELECT status FROM api_clients
                 WHERE id=?1 AND tenant_id=?2",
                params![client_id, admin.tenant_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("client not found".into()))?;

        let issued = match action {
            "revoke" | "disable" => {
                tx.execute(
                    "UPDATE api_clients
                     SET status=?1
                     WHERE id=?2 AND status<>'revoked'",
                    params![
                        if action == "revoke" {
                            "revoked"
                        } else {
                            "disabled"
                        },
                        client_id
                    ],
                )?;
                None
            }
            "enable" if status != "revoked" => {
                tx.execute(
                    "UPDATE api_clients SET status='active' WHERE id=?1",
                    [client_id],
                )?;
                None
            }
            "rotate" if status == "active" => {
                tx.execute(
                    "UPDATE api_credentials
                     SET status='revoked'
                     WHERE client_id=?1",
                    [client_id],
                )?;
                Some(issue_credential(&tx, client_id)?)
            }
            _ => {
                return Err(AppError::BadRequest("invalid lifecycle transition".into()));
            }
        };

        append_machine_audit(
            &tx,
            Some(admin),
            client_id,
            &Uuid::new_v4().to_string(),
            "machine.lifecycle",
            action,
        )?;
        tx.commit()?;
        Ok(issued)
    }

    pub(crate) fn list(
        &self,
        admin: &MachineAdminContext,
    ) -> Result<Vec<MachineClientProjection>, AppError> {
        let connection = self.pool.get()?;
        verify_live_admin(&connection, admin)?;

        let mut statement = connection.prepare(
            "SELECT c.id,c.identity_id,c.name,c.status,c.api_version,c.rate_limit_rpm,
                    u.last_used_at,COALESCE(u.used,0)
             FROM api_clients c
             LEFT JOIN api_usage_windows u ON u.client_id=c.id
             WHERE c.tenant_id=?1
             ORDER BY c.created_at DESC
             LIMIT 200",
        )?;
        let clients = statement
            .query_map([&admin.tenant_id], |row| {
                Ok(MachineClientProjection {
                    client_id: row.get(0)?,
                    identity_id: row.get(1)?,
                    name: row.get(2)?,
                    status: row.get(3)?,
                    api_version: row.get(4)?,
                    rate_limit_rpm: row.get(5)?,
                    last_used_at: row.get(6)?,
                    window_usage: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(clients)
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

        let mut connection = self.pool.get()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        verify_live_admin(&tx, admin)?;
        let affected = tx.execute(
            "UPDATE api_credentials
             SET status=?1
             WHERE id=?2 AND client_id=?3
               AND status<>'revoked'
               AND EXISTS(
                 SELECT 1 FROM api_clients
                 WHERE id=?3 AND tenant_id=?4 AND status<>'revoked'
               )",
            params![status, credential_id, client_id, admin.tenant_id],
        )?;
        if affected != 1 {
            return Err(AppError::NotFound("active credential not found".into()));
        }
        append_machine_audit(
            &tx,
            Some(admin),
            client_id,
            &Uuid::new_v4().to_string(),
            "machine.credential",
            action,
        )?;
        tx.commit()?;
        Ok(())
    }
}

fn verify_live_admin(connection: &Connection, admin: &MachineAdminContext) -> Result<(), AppError> {
    if !matches!(admin.role, TenantRole::Admin | TenantRole::Owner) {
        return Err(AppError::Forbidden);
    }

    let live: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1
            FROM tenant_memberships m
            JOIN identities i ON i.id=m.identity_id
            JOIN tenants t ON t.id=m.tenant_id
            WHERE m.id=?1
              AND m.identity_id=?2
              AND m.tenant_id=?3
              AND m.status='active'
              AND m.role IN ('admin','owner')
              AND i.status='active'
              AND t.status='active'
         )",
        params![admin.membership_id, admin.identity_id, admin.tenant_id],
        |row| row.get(0),
    )?;
    if live {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn issue_credential(
    connection: &Connection,
    client_id: &str,
) -> Result<MachineIssuedCredential, AppError> {
    let credential_id = Uuid::new_v4().to_string();
    let prefix = format!("mapi_{}", Uuid::new_v4().simple());
    let random: [u8; 32] = rand::random();
    let secret = format!("{prefix}.{}", hex::encode(random));
    let expires_at = (Utc::now() + Duration::days(90)).to_rfc3339();
    connection.execute(
        "INSERT INTO api_credentials
         (id,client_id,key_prefix,key_hash,status,expires_at,created_at)
         VALUES (?1,?2,?3,?4,'active',?5,?6)",
        params![
            credential_id,
            client_id,
            prefix,
            digest(&secret),
            expires_at,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(MachineIssuedCredential {
        client_id: client_id.into(),
        credential_id,
        secret,
        expires_at,
    })
}

fn append_machine_audit(
    connection: &Connection,
    actor: Option<&MachineAdminContext>,
    client_id: &str,
    correlation_id: &str,
    action: &str,
    outcome: &str,
) -> Result<(), AppError> {
    let identity = actor.map(|actor| actor.identity_id.as_str());
    let membership = actor.map(|actor| actor.membership_id.as_str());
    let tenant = actor.map(|actor| actor.tenant_id.as_str());
    let role = actor.map(|actor| audit_role_name(actor.role)).unwrap_or("");
    connection.execute(
        "INSERT INTO audit_events
         (id,actor_identity_id,authority_kind,tenant_id,tenant_membership_id,
          roles_snapshot,action,resource_type,resource_id,correlation_id,
          detail_json,occurred_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,'api_client',?8,?9,?10,?11)",
        params![
            Uuid::new_v4().to_string(),
            identity,
            if identity.is_some() {
                "tenant"
            } else {
                "system"
            },
            tenant,
            membership,
            serde_json::json!([role]).to_string(),
            action,
            client_id,
            correlation_id,
            serde_json::json!({
                "client_id": client_id,
                "outcome": outcome
            })
            .to_string(),
            Utc::now().to_rfc3339()
        ],
    )?;
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
