//! Project/Profile machine authentication, not a new Canonical authority.
//! FUTURE_CANONICAL_MIGRATION: owner-approved local non-interactive profile.
//! No creator lookup is used during execution. Every request rechecks the
//! identity, real membership, client and credential before entering Registry.

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde::{Deserialize, Serialize};
use system_core::{AuthorityContext, TenantId, TenantMembershipId, TenantRole};

use crate::auth_contract::AuthUserInfo;
use crate::error::AppError;
use crate::registry::ModuleRegistry;
use crate::repositories::{
    MachineAdminContext, MachineAuthorityRepository, MachineIssuedCredential,
    MachineProvisionRecord, MachineScopeRecord,
};

type Db = Pool<SqliteConnectionManager>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provision {
    pub name: String,
    pub scopes: Vec<Scope>,
    pub rate_limit_rpm: u16,
    pub role: TenantRole,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub module: String,
    pub command: String,
}

// Deliberately no Debug: the secret exists only in the one-time response.
#[derive(Serialize)]
pub struct Issued {
    pub client_id: String,
    pub credential_id: String,
    pub secret: String,
    pub expires_at: String,
}

pub(crate) struct Authorized {
    pub identity_id: String,
    pub authority: AuthorityContext,
    pub tenant_id: TenantId,
}

fn administrator(auth: &AuthUserInfo) -> Result<MachineAdminContext, AppError> {
    match &auth.authority {
        AuthorityContext::Tenant {
            tenant_id,
            membership_id,
            role: TenantRole::Admin | TenantRole::Owner,
        } => Ok(MachineAdminContext {
            identity_id: auth.id.clone(),
            membership_id: membership_id.as_str().to_owned(),
            tenant_id: tenant_id.as_str().to_owned(),
            role: auth.tenant_role().ok_or(AppError::Forbidden)?,
        }),
        _ => Err(AppError::Forbidden),
    }
}

fn scope_record(scope: &Scope) -> MachineScopeRecord {
    MachineScopeRecord {
        module: scope.module.clone(),
        command: scope.command.clone(),
    }
}

/// Interactive identity/compliance surfaces are intentionally not machine
/// capabilities. Business automation may use tenant-admin commands when
/// explicitly scoped, but it must never become human authentication,
/// consent, deletion-governance, or UI-personalization authority.
pub(crate) fn machine_scope_allowed(scope: &Scope) -> bool {
    crate::repositories::machine_scope_allowed(&scope_record(scope))
}

fn issued_projection(issued: MachineIssuedCredential) -> Issued {
    Issued {
        client_id: issued.client_id,
        credential_id: issued.credential_id,
        secret: issued.secret,
        expires_at: issued.expires_at,
    }
}

pub fn provision(
    pool: &Db,
    registry: &ModuleRegistry,
    auth: &AuthUserInfo,
    input: Provision,
) -> Result<Issued, AppError> {
    provision_with_repository(
        &MachineAuthorityRepository::new(pool.clone()),
        registry,
        auth,
        input,
    )
}

pub(crate) fn provision_with_repository(
    repository: &MachineAuthorityRepository,
    registry: &ModuleRegistry,
    auth: &AuthUserInfo,
    input: Provision,
) -> Result<Issued, AppError> {
    if input.name.trim().is_empty()
        || input.name.len() > 120
        || !(1..=600).contains(&input.rate_limit_rpm)
        || input.scopes.is_empty()
        || input.scopes.len() > 64
        || input.role == TenantRole::Owner
    {
        return Err(AppError::BadRequest("invalid machine client policy".into()));
    }

    for scope in &input.scopes {
        if !machine_scope_allowed(scope) {
            return Err(AppError::BadRequest(
                "scope requires an interactive principal".into(),
            ));
        }
        let module = registry
            .get(&scope.module)
            .ok_or_else(|| AppError::BadRequest("unknown scope".into()))?;
        let metadata = module
            .commands()
            .into_iter()
            .find(|command| command.name == scope.command)
            .ok_or_else(|| AppError::BadRequest("unknown scope".into()))?;
        if !matches!(
            metadata.access,
            system_core::AccessRequirement::Authenticated
                | system_core::AccessRequirement::TenantAdmin
        ) {
            return Err(AppError::BadRequest("scope is not a tenant command".into()));
        }
    }

    let admin = administrator(auth)?;
    let record = MachineProvisionRecord {
        name: input.name,
        scopes: input.scopes.iter().map(scope_record).collect(),
        rate_limit_rpm: input.rate_limit_rpm,
        role: input.role,
    };
    repository.provision(&admin, record).map(issued_projection)
}

/// Called only by Registry's machine entry point. Fail-closed storage/audit,
/// durable rate budget and a fresh authority read, including on retries.
pub(crate) fn authorize(
    pool: &Db,
    token: &str,
    requested_tenant: &str,
    version: &str,
    scope: &Scope,
    correlation: &str,
) -> Result<Authorized, AppError> {
    authorize_with_repository(
        &MachineAuthorityRepository::new(pool.clone()),
        token,
        requested_tenant,
        version,
        scope,
        correlation,
    )
}

pub(crate) fn authorize_with_repository(
    repository: &MachineAuthorityRepository,
    token: &str,
    requested_tenant: &str,
    version: &str,
    scope: &Scope,
    correlation: &str,
) -> Result<Authorized, AppError> {
    let authorized = repository.authorize(
        token,
        requested_tenant,
        version,
        &scope_record(scope),
        correlation,
    )?;
    let tenant_id = TenantId::new(authorized.tenant_id).map_err(AppError::Internal)?;
    let authority = AuthorityContext::Tenant {
        tenant_id: tenant_id.clone(),
        membership_id: TenantMembershipId::new(authorized.membership_id)
            .map_err(AppError::Internal)?,
        role: authorized.role,
    };
    Ok(Authorized {
        identity_id: authorized.identity_id,
        authority,
        tenant_id,
    })
}

pub fn lifecycle(
    pool: &Db,
    auth: &AuthUserInfo,
    client: &str,
    action: &str,
) -> Result<Option<Issued>, AppError> {
    lifecycle_with_repository(
        &MachineAuthorityRepository::new(pool.clone()),
        auth,
        client,
        action,
    )
}

pub(crate) fn lifecycle_with_repository(
    repository: &MachineAuthorityRepository,
    auth: &AuthUserInfo,
    client: &str,
    action: &str,
) -> Result<Option<Issued>, AppError> {
    let admin = administrator(auth)?;
    repository
        .lifecycle(&admin, client, action)
        .map(|issued| issued.map(issued_projection))
}

pub fn list(pool: &Db, auth: &AuthUserInfo) -> Result<serde_json::Value, AppError> {
    list_with_repository(&MachineAuthorityRepository::new(pool.clone()), auth)
}

pub(crate) fn list_with_repository(
    repository: &MachineAuthorityRepository,
    auth: &AuthUserInfo,
) -> Result<serde_json::Value, AppError> {
    let admin = administrator(auth)?;
    let clients = repository.list(&admin)?;
    Ok(serde_json::json!({ "clients": clients }))
}

pub fn credential_lifecycle(
    pool: &Db,
    auth: &AuthUserInfo,
    client: &str,
    credential: &str,
    action: &str,
) -> Result<(), AppError> {
    credential_lifecycle_with_repository(
        &MachineAuthorityRepository::new(pool.clone()),
        auth,
        client,
        credential,
        action,
    )
}

pub(crate) fn credential_lifecycle_with_repository(
    repository: &MachineAuthorityRepository,
    auth: &AuthUserInfo,
    client: &str,
    credential: &str,
    action: &str,
) -> Result<(), AppError> {
    let admin = administrator(auth)?;
    repository.credential_lifecycle(&admin, client, credential, action)
}

#[cfg(test)]
mod tests;
