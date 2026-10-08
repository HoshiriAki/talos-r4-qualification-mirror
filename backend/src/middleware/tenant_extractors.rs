use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use std::sync::Arc;
use system_core::transport::http_client::HttpClient;
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, RequestId,
    Revision, TenantId, TenantScope,
};

use crate::auth_contract::AuthUserInfo;
use crate::middleware::tenant::Tenant;
use crate::state::AppState;

/// Axum extractor for authenticated requests with tenant context.
///
/// Usage in route handlers:
/// ```rust
/// async fn my_handler(TenantUser(tenant, user): TenantUser) -> Result<Json<Response>, AppError> {
///     // tenant: Tenant
///     // user: AuthUserInfo
/// }
/// ```
pub struct TenantUser(pub Tenant, pub AuthUserInfo);

impl FromRequestParts<Arc<AppState>> for TenantUser {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // Extract tenant from extensions (injected by tenant middleware)
        let tenant = parts
            .extensions
            .get::<Tenant>()
            .cloned()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        // Extract authenticated user
        let user = crate::middleware::auth::AnyAuthorityUser::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        // Verify user belongs to this tenant
        if user.0.tenant_id() != Some(&tenant.id) {
            return Err(StatusCode::FORBIDDEN);
        }

        // Verify tenant status is active
        if tenant.status != "active" {
            return Err(StatusCode::FORBIDDEN);
        }

        Ok(TenantUser(tenant, user.0))
    }
}

/// Axum extractor for admin requests with tenant context.
///
/// Requires both:
/// - User must be authenticated with admin role
/// - User must belong to the current tenant
pub struct TenantAdmin(pub Tenant, pub AuthUserInfo);

impl FromRequestParts<Arc<AppState>> for TenantAdmin {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // Extract tenant from extensions
        let tenant = parts
            .extensions
            .get::<Tenant>()
            .cloned()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        // Extract authenticated admin user
        let user = crate::middleware::auth::AdminUser::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::FORBIDDEN)?;

        // Verify user belongs to this tenant
        if user.0.tenant_id() != Some(&tenant.id) {
            return Err(StatusCode::FORBIDDEN);
        }

        // Verify tenant status is active
        if tenant.status != "active" {
            return Err(StatusCode::FORBIDDEN);
        }

        Ok(TenantAdmin(tenant, user.0))
    }
}

/// Axum extractor for requests authenticated with platform authority.
/// Handlers must additionally require their operation-specific capability.
pub struct PlatformUser(pub AuthUserInfo);

impl FromRequestParts<Arc<AppState>> for PlatformUser {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // Extract authenticated user
        let user = crate::middleware::auth::AnyAuthorityUser::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        if !matches!(
            user.0.authority,
            system_core::AuthorityContext::Platform { .. }
        ) {
            return Err(StatusCode::FORBIDDEN);
        }

        Ok(PlatformUser(user.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustedTenantResolutionError {
    TenantInactive,
    TenantAuthorityRequired,
    TenantMismatch,
    InvalidTenantId(String),
    InvalidActor(String),
    InvalidContext(String),
}

impl TrustedTenantResolutionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TenantInactive => "TRUSTED_TENANT_INACTIVE",
            Self::TenantAuthorityRequired => "TRUSTED_TENANT_AUTHORITY_REQUIRED",
            Self::TenantMismatch => "TRUSTED_TENANT_MISMATCH",
            Self::InvalidTenantId(_) => "TRUSTED_TENANT_ID_INVALID",
            Self::InvalidActor(_) => "TRUSTED_TENANT_ACTOR_INVALID",
            Self::InvalidContext(_) => "TRUSTED_TENANT_CONTEXT_INVALID",
        }
    }

    fn rejection_status(&self) -> StatusCode {
        match self {
            Self::TenantInactive | Self::TenantAuthorityRequired | Self::TenantMismatch => {
                StatusCode::FORBIDDEN
            }
            Self::InvalidTenantId(_) | Self::InvalidActor(_) | Self::InvalidContext(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

/// A tenant request whose active host tenant and authenticated tenant authority
/// were resolved together before an ExecutionContext was constructed.
pub struct TrustedTenantUser {
    tenant: Tenant,
    user: AuthUserInfo,
    context: ExecutionContext,
}

impl TrustedTenantUser {
    pub fn tenant(&self) -> &Tenant {
        &self.tenant
    }

    pub fn user(&self) -> &AuthUserInfo {
        &self.user
    }

    pub fn context(&self) -> &ExecutionContext {
        &self.context
    }
}

impl FromRequestParts<Arc<AppState>> for TrustedTenantUser {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let TenantUser(tenant, user) = TenantUser::from_request_parts(parts, state).await?;
        let context = resolve_trusted_tenant_context(&tenant, &user, state.http_client.clone())
            .map_err(|error| {
                tracing::warn!(
                    code = error.code(),
                    "trusted tenant request resolution failed"
                );
                error.rejection_status()
            })?;
        Ok(Self {
            tenant,
            user,
            context,
        })
    }
}

/// Admin variant of TrustedTenantUser. The tenant/admin checks happen before
/// the immutable ExecutionContext is exposed to a handler.
pub struct TrustedTenantAdmin {
    tenant: Tenant,
    user: AuthUserInfo,
    context: ExecutionContext,
}

impl TrustedTenantAdmin {
    pub fn tenant(&self) -> &Tenant {
        &self.tenant
    }

    pub fn user(&self) -> &AuthUserInfo {
        &self.user
    }

    pub fn context(&self) -> &ExecutionContext {
        &self.context
    }
}

impl FromRequestParts<Arc<AppState>> for TrustedTenantAdmin {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let TenantAdmin(tenant, user) = TenantAdmin::from_request_parts(parts, state).await?;
        let context = resolve_trusted_tenant_context(&tenant, &user, state.http_client.clone())
            .map_err(|error| {
                tracing::warn!(
                    code = error.code(),
                    "trusted tenant admin resolution failed"
                );
                error.rejection_status()
            })?;
        Ok(Self {
            tenant,
            user,
            context,
        })
    }
}

fn resolve_trusted_tenant_context(
    tenant: &Tenant,
    user: &AuthUserInfo,
    http_client: Arc<dyn HttpClient>,
) -> Result<ExecutionContext, TrustedTenantResolutionError> {
    if tenant.status != "active" {
        return Err(TrustedTenantResolutionError::TenantInactive);
    }

    let authority_tenant = match &user.authority {
        AuthorityContext::Tenant { tenant_id, .. } => tenant_id,
        AuthorityContext::Platform { .. } => {
            return Err(TrustedTenantResolutionError::TenantAuthorityRequired);
        }
    };
    if authority_tenant.as_str() != tenant.id {
        return Err(TrustedTenantResolutionError::TenantMismatch);
    }

    let tenant_id =
        TenantId::new(tenant.id.clone()).map_err(TrustedTenantResolutionError::InvalidTenantId)?;
    let data_scope = DataScope::production(
        tenant_id.clone(),
        Revision::new("trusted-route-production-current")
            .map_err(TrustedTenantResolutionError::InvalidContext)?,
    )
    .map_err(TrustedTenantResolutionError::InvalidContext)?;
    ExecutionContext::new(
        ActorIdentity::with_authority(user.id.clone(), user.authority.clone())
            .map_err(TrustedTenantResolutionError::InvalidActor)?,
        TenantScope::tenant(tenant_id),
        data_scope,
        ExecutionMode::Normal,
        RequestId::new(format!("http:{}", uuid::Uuid::new_v4()))
            .map_err(TrustedTenantResolutionError::InvalidContext)?,
        None,
        http_client,
    )
    .map_err(TrustedTenantResolutionError::InvalidContext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_core::{
        ExecutionPlane, NoopHttpClient, PlatformMembershipId, PlatformRole, TenantMembershipId,
        TenantRole,
    };

    fn tenant(id: &str, status: &str) -> Tenant {
        Tenant {
            id: id.into(),
            name: id.into(),
            slug: id.into(),
            status: status.into(),
            plan: "test".into(),
            settings: None,
            created_at: "2026-08-04T00:00:00Z".into(),
            updated_at: "2026-08-04T00:00:00Z".into(),
        }
    }

    fn tenant_user(tenant_id: &str) -> AuthUserInfo {
        AuthUserInfo {
            id: "identity-a".into(),
            username: "operator".into(),
            session_id: "session-a".into(),
            display_name: "Operator".into(),
            email: "operator@example.test".into(),
            phone: String::new(),
            authority: AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new("membership-a").unwrap(),
                tenant_id: TenantId::new(tenant_id).unwrap(),
                role: TenantRole::Admin,
            },
        }
    }

    fn platform_user() -> AuthUserInfo {
        AuthUserInfo {
            id: "platform-identity".into(),
            username: "platform".into(),
            session_id: "platform-session".into(),
            display_name: "Platform".into(),
            email: "platform@example.test".into(),
            phone: String::new(),
            authority: AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        }
    }

    #[test]
    fn matching_active_tenant_produces_fresh_immutable_scope() {
        let selected = tenant("tenant-a", "active");
        let user = tenant_user("tenant-a");
        let first =
            resolve_trusted_tenant_context(&selected, &user, Arc::new(NoopHttpClient)).unwrap();
        let second =
            resolve_trusted_tenant_context(&selected, &user, Arc::new(NoopHttpClient)).unwrap();

        assert_eq!(
            first.tenant_scope().effective_tenant_id().as_str(),
            "tenant-a"
        );
        assert_eq!(first.data_scope().tenant_id().as_str(), "tenant-a");
        assert_eq!(first.actor().id(), Some("identity-a"));
        assert_eq!(first.plane(), ExecutionPlane::TenantBusiness);
        assert!(matches!(first.execution_mode(), ExecutionMode::Normal));
        assert_ne!(first.correlation_id(), second.correlation_id());
    }

    #[test]
    fn tenant_mismatch_fails_closed() {
        let error = resolve_trusted_tenant_context(
            &tenant("tenant-a", "active"),
            &tenant_user("tenant-b"),
            Arc::new(NoopHttpClient),
        )
        .err()
        .expect("tenant mismatch must fail");
        assert_eq!(error.code(), "TRUSTED_TENANT_MISMATCH");
    }

    #[test]
    fn platform_authority_cannot_become_tenant_context() {
        let error = resolve_trusted_tenant_context(
            &tenant("tenant-a", "active"),
            &platform_user(),
            Arc::new(NoopHttpClient),
        )
        .err()
        .expect("platform authority must not become tenant authority");
        assert_eq!(error.code(), "TRUSTED_TENANT_AUTHORITY_REQUIRED");
    }

    #[test]
    fn inactive_tenant_fails_closed() {
        let error = resolve_trusted_tenant_context(
            &tenant("tenant-a", "suspended"),
            &tenant_user("tenant-a"),
            Arc::new(NoopHttpClient),
        )
        .err()
        .expect("inactive tenant must fail");
        assert_eq!(error.code(), "TRUSTED_TENANT_INACTIVE");
    }
}
