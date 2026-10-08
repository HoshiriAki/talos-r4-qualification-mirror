use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Deserialize;
use system_core::{ALL_PLATFORM_CAPABILITIES, AuthorityContext, PlatformCapability, PlatformRole};

use crate::{
    error::AppError,
    middleware::tenant_extractors::PlatformUser,
    repositories::{
        CreatePlatformMembership, NewPlatformIdentity, PlatformMembershipAuditActor,
        PlatformMembershipMutationError, RevokePlatformMembership, UpdatePlatformMembership,
    },
    services::auth_service,
    state::AppState,
    utils::time::shanghai_now_iso,
};

const PLATFORM_ROLES: &[&str] = &[
    "platform_owner",
    "platform_admin",
    "platform_operator",
    "support_engineer",
    "business_operator",
    "security_auditor",
];

pub fn platform_membership_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/platform/memberships",
            get(list_memberships).post(create_membership),
        )
        .route(
            "/api/platform/memberships/{id}",
            axum::routing::put(update_membership).delete(revoke_membership),
        )
}

fn require_capability(user: &PlatformUser, capability: PlatformCapability) -> Result<(), AppError> {
    if user.0.has_platform_capability(capability) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn is_platform_owner(user: &PlatformUser) -> bool {
    user.0.platform_roles().contains(&PlatformRole::Owner)
}

fn require_platform_owner(user: &PlatformUser) -> Result<(), AppError> {
    if is_platform_owner(user) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn platform_membership_audit_actor(
    user: &PlatformUser,
) -> Result<PlatformMembershipAuditActor, AppError> {
    let AuthorityContext::Platform {
        membership_id,
        roles,
    } = &user.0.authority
    else {
        return Err(AppError::Forbidden);
    };
    let capabilities = ALL_PLATFORM_CAPABILITIES
        .iter()
        .copied()
        .filter(|capability| user.0.has_platform_capability(*capability))
        .collect::<Vec<_>>();
    Ok(PlatformMembershipAuditActor {
        identity_id: user.0.id.clone(),
        membership_id: membership_id.as_str().to_string(),
        roles_snapshot: serde_json::to_string(roles)?,
        capabilities_snapshot: serde_json::to_string(&capabilities)?,
    })
}

fn map_membership_mutation_error(error: PlatformMembershipMutationError) -> AppError {
    match error {
        PlatformMembershipMutationError::IdentityNotFound => {
            AppError::NotFound("Identity 不存在或不可用".into())
        }
        PlatformMembershipMutationError::DuplicateIdentity => {
            AppError::Conflict("用户名或邮箱已存在".into())
        }
        PlatformMembershipMutationError::MembershipExists => {
            AppError::Conflict("该 Identity 已有平台成员关系".into())
        }
        PlatformMembershipMutationError::MembershipNotFound => {
            AppError::NotFound("平台成员不存在".into())
        }
        PlatformMembershipMutationError::OwnerAuthorityRequired => AppError::Forbidden,
        PlatformMembershipMutationError::LastActiveOwner => {
            AppError::Conflict("平台必须保留至少一个有效 platform_owner".into())
        }
        PlatformMembershipMutationError::SelfRevoke => {
            AppError::BadRequest("不能撤销当前正在使用的平台成员关系".into())
        }
        PlatformMembershipMutationError::Storage(storage) => {
            tracing::error!(
                repository_error_code = storage.code(),
                "platform membership repository mutation failed"
            );
            AppError::Internal("platform membership repository unavailable".into())
        }
    }
}

fn normalize_roles(roles: Vec<String>) -> Result<Vec<String>, AppError> {
    let mut normalized = Vec::new();
    for role in roles {
        let role = role.trim().to_lowercase();
        if !PLATFORM_ROLES.contains(&role.as_str()) {
            return Err(AppError::BadRequest(format!("未知平台角色: {role}")));
        }
        if !normalized.contains(&role) {
            normalized.push(role);
        }
    }
    if normalized.is_empty() {
        return Err(AppError::BadRequest("平台成员至少需要一个角色".into()));
    }
    Ok(normalized)
}

async fn list_memberships(
    State(state): State<Arc<AppState>>,
    user: PlatformUser,
) -> Result<Json<serde_json::Value>, AppError> {
    require_capability(&user, PlatformCapability::PlatformIdentityManage)?;
    let memberships = state
        .platform_membership_repository()
        .list()
        .map_err(|error| {
            tracing::error!(
                repository_error_code = error.code(),
                "platform membership repository read failed"
            );
            AppError::Internal("platform membership repository unavailable".into())
        })?;
    let memberships = memberships
        .into_iter()
        .map(|membership| {
            serde_json::json!({
                "id": membership.id,
                "identityId": membership.identity_id,
                "username": membership.username,
                "displayName": membership.display_name,
                "email": membership.email,
                "status": membership.status,
                "roles": membership.roles,
                "createdAt": membership.created_at,
                "updatedAt": membership.updated_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(serde_json::json!({ "memberships": memberships })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateMembershipBody {
    identity_id: Option<String>,
    username: Option<String>,
    password: Option<String>,
    display_name: Option<String>,
    email: Option<String>,
    phone: Option<String>,
    roles: Vec<String>,
}

async fn create_membership(
    State(state): State<Arc<AppState>>,
    user: PlatformUser,
    Json(body): Json<CreateMembershipBody>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), AppError> {
    require_capability(&user, PlatformCapability::PlatformIdentityManage)?;
    require_capability(&user, PlatformCapability::PlatformRoleManage)?;

    let CreateMembershipBody {
        identity_id,
        username,
        password,
        display_name,
        email,
        phone,
        roles,
    } = body;
    let roles = normalize_roles(roles)?;
    let actor_is_platform_owner = is_platform_owner(&user);
    if roles.iter().any(|role| role == "platform_owner") {
        require_platform_owner(&user)?;
    }

    let new_identity = if identity_id.is_none() {
        let username = username.unwrap_or_default().trim().to_string();
        let password = password.unwrap_or_default();
        if username.is_empty() || password.len() < 8 {
            return Err(AppError::BadRequest(
                "新 Identity 需要用户名和至少 8 位密码".into(),
            ));
        }
        let budget = auth_service::consume_custom_budget(
            &format!("platform-identity-create::{}", user.0.id),
            &state.config,
        );
        if !budget.allowed {
            return Err(AppError::RateLimited {
                retry_after_secs: u64::try_from(((budget.retry_after_ms + 999) / 1000).max(1))
                    .unwrap_or(1),
            });
        }
        let display_name = display_name
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| username.clone());
        Some(NewPlatformIdentity {
            username,
            email,
            password_hash: auth_service::hash_password(&password),
            display_name,
            phone,
        })
    } else {
        None
    };

    let created = state
        .platform_membership_repository()
        .create(CreatePlatformMembership {
            existing_identity_id: identity_id,
            new_identity,
            roles: roles.clone(),
            actor: platform_membership_audit_actor(&user)?,
            actor_is_platform_owner,
            now: shanghai_now_iso(),
        })
        .map_err(map_membership_mutation_error)?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({
            "ok": true,
            "membershipId": created.membership_id,
            "identityId": created.identity_id,
            "roles": roles,
        })),
    ))
}

#[derive(Deserialize)]
struct UpdateMembershipBody {
    status: Option<String>,
    roles: Option<Vec<String>>,
}

async fn update_membership(
    State(state): State<Arc<AppState>>,
    user: PlatformUser,
    Path(membership_id): Path<String>,
    Json(body): Json<UpdateMembershipBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_capability(&user, PlatformCapability::PlatformIdentityManage)?;
    if body.roles.is_some() {
        require_capability(&user, PlatformCapability::PlatformRoleManage)?;
    }

    let roles = body.roles.map(normalize_roles).transpose()?;
    let status = body.status.map(|value| value.trim().to_string());
    if status
        .as_deref()
        .is_some_and(|value| !matches!(value, "active" | "suspended" | "revoked"))
    {
        return Err(AppError::BadRequest("平台成员状态无效".into()));
    }
    if roles.is_none() && status.is_none() {
        return Err(AppError::BadRequest("没有可更新字段".into()));
    }

    state
        .platform_membership_repository()
        .update(UpdatePlatformMembership {
            membership_id: membership_id.clone(),
            status,
            roles,
            actor: platform_membership_audit_actor(&user)?,
            actor_is_platform_owner: is_platform_owner(&user),
            now: shanghai_now_iso(),
        })
        .map_err(map_membership_mutation_error)?;

    Ok(Json(
        serde_json::json!({ "ok": true, "membershipId": membership_id }),
    ))
}

async fn revoke_membership(
    State(state): State<Arc<AppState>>,
    user: PlatformUser,
    Path(membership_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_capability(&user, PlatformCapability::PlatformIdentityManage)?;

    state
        .platform_membership_repository()
        .revoke(RevokePlatformMembership {
            membership_id: membership_id.clone(),
            actor: platform_membership_audit_actor(&user)?,
            actor_is_platform_owner: is_platform_owner(&user),
            now: shanghai_now_iso(),
        })
        .map_err(map_membership_mutation_error)?;

    Ok(Json(
        serde_json::json!({ "ok": true, "membershipId": membership_id }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_roles_are_validated_and_deduplicated() {
        let roles = normalize_roles(vec![
            "platform_owner".into(),
            "platform_owner".into(),
            "support_engineer".into(),
        ])
        .unwrap();
        assert_eq!(roles, vec!["platform_owner", "support_engineer"]);
        assert!(normalize_roles(vec!["root".into()]).is_err());
        assert!(normalize_roles(vec![]).is_err());
    }
}
