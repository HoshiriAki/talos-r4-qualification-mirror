use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

use crate::auth_contract::AuthUserInfo;
use crate::middleware::tenant::Tenant;
use crate::services::auth_service;
use crate::state::AppState;
use system_core::{AuthorityContext, TenantRole};

pub struct AuthUser(pub AuthUserInfo);
pub struct AnyAuthorityUser(pub AuthUserInfo);

pub struct AdminUser(pub AuthUserInfo);

fn auth_error(status: StatusCode, code: &str, message: &str) -> Response {
    let body = serde_json::json!({
        "ok": false,
        "code": code,
        "error": message,
    });
    (status, Json(body)).into_response()
}

fn parse_auth_cookie(cookie_header: Option<&str>, cookie_name: &str) -> Option<String> {
    let header = cookie_header?;
    for part in header.split(';') {
        let trimmed = part.trim();
        if let Some((key, value)) = trimmed.split_once('=')
            && key.trim() == cookie_name
        {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn requested_tenant_id(parts: &Parts) -> Option<&str> {
    let platform = parts
        .headers
        .get("x-talos-authority")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("platform"));
    if platform {
        None
    } else {
        parts
            .extensions
            .get::<Tenant>()
            .map(|tenant| tenant.id.as_str())
    }
}

impl FromRequestParts<Arc<AppState>> for AnyAuthorityUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let cookie_name = &state.config.auth_cookie_name;
        let cookie_header = parts.headers.get("cookie").and_then(|v| v.to_str().ok());
        let session_id = parse_auth_cookie(cookie_header, cookie_name).unwrap_or_default();

        if session_id.is_empty() {
            return Err(auth_error(
                StatusCode::UNAUTHORIZED,
                "AUTH_UNAUTHORIZED",
                "未登录",
            ));
        }

        let tenant_id = requested_tenant_id(parts);
        let user = auth_service::get_auth_user_with_repository(
            state.identity_authority_repository(),
            &session_id,
            tenant_id,
        )
        .map_err(|_| auth_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", "服务器错误"))?;

        match user {
            Some(u) => Ok(AnyAuthorityUser(u)),
            None => Err(auth_error(
                StatusCode::UNAUTHORIZED,
                "AUTH_UNAUTHORIZED",
                "未登录",
            )),
        }
    }
}

impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let AnyAuthorityUser(user) = AnyAuthorityUser::from_request_parts(parts, state).await?;
        if !matches!(user.authority, AuthorityContext::Tenant { .. }) {
            return Err(auth_error(
                StatusCode::FORBIDDEN,
                "AUTH_TENANT_AUTHORITY_REQUIRED",
                "该接口需要租户权限上下文",
            ));
        }
        Ok(AuthUser(user))
    }
}

impl FromRequestParts<Arc<AppState>> for AdminUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let AuthUser(user) = AuthUser::from_request_parts(parts, state).await?;

        if !matches!(
            user.authority,
            AuthorityContext::Tenant {
                role: TenantRole::Admin | TenantRole::Owner,
                ..
            }
        ) {
            return Err(auth_error(
                StatusCode::FORBIDDEN,
                "AUTH_FORBIDDEN",
                "仅管理员可操作",
            ));
        }

        Ok(AdminUser(user))
    }
}
