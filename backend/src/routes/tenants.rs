//! 租户管理 API
//!
//! 平台控制平面租户 CRUD。HTTP 层只负责 capability/input contract；
//! 持久化、首个 owner 创建与 authority audit 由 backend-neutral repository 原子执行。

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch},
};
use serde::{Deserialize, Serialize};
use system_core::{ALL_PLATFORM_CAPABILITIES, AuthorityContext, PlatformCapability};

use crate::{
    middleware::tenant_extractors::PlatformUser,
    repositories::{
        ClosePlatformTenant, CreatePlatformTenant, PlatformTenantAuditActor,
        PlatformTenantMutationError, PlatformTenantProjection, RepositoryError,
        UpdatePlatformTenant, UpdatePlatformTenantStatus,
    },
    utils::time::shanghai_now_iso,
};

fn require_capability(
    user: &PlatformUser,
    capability: PlatformCapability,
) -> Result<(), (StatusCode, String)> {
    if user.0.has_platform_capability(capability) {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "platform capability required".into()))
    }
}

fn platform_tenant_audit_actor(
    user: &PlatformUser,
) -> Result<PlatformTenantAuditActor, (StatusCode, String)> {
    let AuthorityContext::Platform {
        membership_id,
        roles,
    } = &user.0.authority
    else {
        return Err((StatusCode::FORBIDDEN, "platform authority required".into()));
    };
    let capabilities = ALL_PLATFORM_CAPABILITIES
        .iter()
        .copied()
        .filter(|capability| user.0.has_platform_capability(*capability))
        .collect::<Vec<_>>();
    Ok(PlatformTenantAuditActor {
        identity_id: user.0.id.clone(),
        membership_id: membership_id.as_str().to_string(),
        roles_snapshot: serde_json::to_string(roles).map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("audit actor serialization failed: {error}"),
            )
        })?,
        capabilities_snapshot: serde_json::to_string(&capabilities).map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("audit capability serialization failed: {error}"),
            )
        })?,
    })
}

fn map_tenant_projection(tenant: PlatformTenantProjection) -> Tenant {
    Tenant {
        id: tenant.id,
        name: tenant.name,
        slug: tenant.slug,
        status: tenant.status,
        plan: tenant.plan,
        settings: tenant.settings,
        created_at: tenant.created_at,
        updated_at: tenant.updated_at,
    }
}

fn map_read_error(error: RepositoryError) -> (StatusCode, String) {
    tracing::error!(
        repository_error_code = error.code(),
        "platform tenant repository read failed"
    );
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("查询失败: {error}"),
    )
}

fn map_create_error(error: PlatformTenantMutationError) -> (StatusCode, String) {
    match error {
        PlatformTenantMutationError::SlugExists => {
            (StatusCode::CONFLICT, "租户 slug 已存在".into())
        }
        PlatformTenantMutationError::OwnerIdentityNotFound => (
            StatusCode::BAD_REQUEST,
            "Owner Identity 不存在或不可用".into(),
        ),
        PlatformTenantMutationError::TenantNotFound
        | PlatformTenantMutationError::TenantNotFoundOrClosed => {
            (StatusCode::NOT_FOUND, "租户不存在".into())
        }
        PlatformTenantMutationError::Storage(error) => {
            tracing::error!(
                repository_error_code = error.code(),
                "platform tenant create failed"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("创建租户失败: {error}"),
            )
        }
    }
}

fn map_update_error(error: PlatformTenantMutationError) -> (StatusCode, String) {
    match error {
        PlatformTenantMutationError::SlugExists => {
            (StatusCode::CONFLICT, "租户 slug 已存在".into())
        }
        PlatformTenantMutationError::TenantNotFound
        | PlatformTenantMutationError::TenantNotFoundOrClosed => {
            (StatusCode::NOT_FOUND, "租户不存在".into())
        }
        PlatformTenantMutationError::OwnerIdentityNotFound => (
            StatusCode::BAD_REQUEST,
            "Owner Identity 不存在或不可用".into(),
        ),
        PlatformTenantMutationError::Storage(error) => {
            tracing::error!(
                repository_error_code = error.code(),
                "platform tenant update failed"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("更新失败: {error}"),
            )
        }
    }
}

fn map_close_error(error: PlatformTenantMutationError) -> (StatusCode, String) {
    match error {
        PlatformTenantMutationError::TenantNotFoundOrClosed => (
            StatusCode::NOT_FOUND,
            "tenant not found or already closed".into(),
        ),
        PlatformTenantMutationError::TenantNotFound => (StatusCode::NOT_FOUND, "租户不存在".into()),
        PlatformTenantMutationError::SlugExists
        | PlatformTenantMutationError::OwnerIdentityNotFound => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "删除失败: invalid repository mutation state".into(),
        ),
        PlatformTenantMutationError::Storage(error) => {
            tracing::error!(
                repository_error_code = error.code(),
                "platform tenant closure failed"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("删除失败: {error}"),
            )
        }
    }
}

pub fn tenant_routes() -> Router<std::sync::Arc<crate::AppState>> {
    Router::new()
        .route("/api/tenants", get(list_tenants).post(create_tenant))
        .route(
            "/api/tenants/{id}",
            get(get_tenant).put(update_tenant).delete(delete_tenant),
        )
        .route("/api/tenants/{id}/status", patch(update_tenant_status))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub status: String,
    pub plan: String,
    pub settings: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTenantInput {
    pub name: String,
    pub slug: String,
    pub settings: Option<serde_json::Value>,
    pub owner_identity_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateTenantInput {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub settings: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantListResponse {
    pub tenants: Vec<Tenant>,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct DeleteTenantResponse {
    pub success: bool,
}

pub async fn list_tenants(
    State(state): State<std::sync::Arc<crate::AppState>>,
    platform_user: PlatformUser,
) -> Result<Json<TenantListResponse>, (StatusCode, String)> {
    require_capability(&platform_user, PlatformCapability::TenantList)?;
    let tenants = state
        .platform_tenant_repository()
        .list()
        .map_err(map_read_error)?
        .into_iter()
        .map(map_tenant_projection)
        .collect::<Vec<_>>();
    let total = tenants.len();
    Ok(Json(TenantListResponse { tenants, total }))
}

pub async fn get_tenant(
    State(state): State<std::sync::Arc<crate::AppState>>,
    platform_user: PlatformUser,
    Path(tenant_id): Path<String>,
) -> Result<Json<Tenant>, (StatusCode, String)> {
    require_capability(&platform_user, PlatformCapability::TenantRead)?;
    let tenant = state
        .platform_tenant_repository()
        .get(&tenant_id)
        .map_err(map_read_error)?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "租户不存在".to_string()))?;
    Ok(Json(map_tenant_projection(tenant)))
}

pub async fn create_tenant(
    State(state): State<std::sync::Arc<crate::AppState>>,
    platform_user: PlatformUser,
    Json(input): Json<CreateTenantInput>,
) -> Result<Json<Tenant>, (StatusCode, String)> {
    require_capability(&platform_user, PlatformCapability::TenantCreate)?;
    if input.name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "租户名称不能为空".to_string()));
    }
    if input.slug.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "租户 slug 不能为空".to_string()));
    }
    if !input
        .slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "slug 只能包含小写字母、数字和连字符".to_string(),
        ));
    }

    let settings = input
        .settings
        .map(|settings| serde_json::to_string(&settings).unwrap_or_default());
    let owner_identity_id = input
        .owner_identity_id
        .as_deref()
        .unwrap_or(&platform_user.0.id)
        .trim()
        .to_string();
    let tenant = state
        .platform_tenant_repository()
        .create(CreatePlatformTenant {
            name: input.name,
            slug: input.slug,
            settings,
            owner_identity_id,
            actor: platform_tenant_audit_actor(&platform_user)?,
            now: shanghai_now_iso(),
        })
        .map_err(map_create_error)?;
    Ok(Json(map_tenant_projection(tenant)))
}

pub async fn update_tenant(
    State(state): State<std::sync::Arc<crate::AppState>>,
    platform_user: PlatformUser,
    Path(tenant_id): Path<String>,
    Json(input): Json<UpdateTenantInput>,
) -> Result<Json<Tenant>, (StatusCode, String)> {
    require_capability(&platform_user, PlatformCapability::TenantUpdate)?;
    if input.name.is_none() && input.slug.is_none() && input.settings.is_none() {
        return Err((StatusCode::BAD_REQUEST, "没有可更新的字段".to_string()));
    }
    let settings = input
        .settings
        .map(|settings| serde_json::to_string(&settings).unwrap_or_default());
    let tenant = state
        .platform_tenant_repository()
        .update(UpdatePlatformTenant {
            tenant_id,
            name: input.name,
            slug: input.slug,
            settings,
            actor: platform_tenant_audit_actor(&platform_user)?,
            now: shanghai_now_iso(),
        })
        .map_err(map_update_error)?;
    Ok(Json(map_tenant_projection(tenant)))
}

pub async fn delete_tenant(
    State(state): State<std::sync::Arc<crate::AppState>>,
    platform_user: PlatformUser,
    Path(tenant_id): Path<String>,
) -> Result<Json<DeleteTenantResponse>, (StatusCode, String)> {
    require_capability(&platform_user, PlatformCapability::TenantDelete)?;
    state
        .platform_tenant_repository()
        .close(ClosePlatformTenant {
            tenant_id,
            actor: platform_tenant_audit_actor(&platform_user)?,
            now: shanghai_now_iso(),
        })
        .map_err(map_close_error)?;
    Ok(Json(DeleteTenantResponse { success: true }))
}

pub async fn update_tenant_status(
    State(state): State<std::sync::Arc<crate::AppState>>,
    platform_user: PlatformUser,
    Path(tenant_id): Path<String>,
    Json(status): Json<serde_json::Value>,
) -> Result<Json<Tenant>, (StatusCode, String)> {
    require_capability(&platform_user, PlatformCapability::TenantSuspend)?;
    let status_str = status
        .get("status")
        .and_then(|status| status.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "缺少 status 字段".to_string()))?;
    if !["active", "suspended", "inactive"].contains(&status_str) {
        return Err((
            StatusCode::BAD_REQUEST,
            "status 必须是 active, suspended 或 inactive".to_string(),
        ));
    }

    let tenant = state
        .platform_tenant_repository()
        .update_status(UpdatePlatformTenantStatus {
            tenant_id,
            status: status_str.to_string(),
            actor: platform_tenant_audit_actor(&platform_user)?,
            now: shanghai_now_iso(),
        })
        .map_err(map_update_error)?;
    Ok(Json(map_tenant_projection(tenant)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_slug_contract_still_rejects_uppercase_and_spaces() {
        for slug in ["Tenant-A", "tenant a", "tenant_a"] {
            assert!(
                !slug
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            );
        }
        assert!(
            "tenant-a"
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        );
    }
}
