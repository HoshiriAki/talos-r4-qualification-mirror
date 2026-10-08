use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::AnyAuthorityUser;
use crate::middleware::tenant::Tenant;
use crate::middleware::trusted_proxy::ResolvedClientIp;
use crate::services::audit_service;
use crate::services::auth_rate_limit::AuthRateLimiter;
use crate::services::auth_service;
use crate::services::session_security;
use crate::services::totp_login;
use crate::state::AppState;
use crate::utils::time;
use system_core::{ALL_PLATFORM_CAPABILITIES, DataScope, Revision, TenantId};

fn auth_user_payload(user: &crate::auth_contract::AuthUserInfo) -> serde_json::Value {
    let capabilities: Vec<_> = ALL_PLATFORM_CAPABILITIES
        .iter()
        .copied()
        .filter(|capability| user.has_platform_capability(*capability))
        .collect();
    serde_json::json!({
        "id": user.id,
        "username": user.username,
        "displayName": user.display_name,
        "email": user.email,
        "phone": user.phone,
        "authority": user.authority,
        "capabilities": capabilities,
    })
}

fn resolved_auth_scope(tenant_id: Option<&str>) -> Result<DataScope, AppError> {
    let tenant_id = tenant_id.ok_or(AppError::Unauthorized)?;
    DataScope::production(
        TenantId::new(tenant_id).map_err(AppError::BadRequest)?,
        Revision::new("production-current").map_err(AppError::Internal)?,
    )
    .map_err(AppError::Internal)
}

pub fn auth_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/platform/login", post(platform_login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me).put(update_profile))
        .route("/auth/change-password", post(change_password))
}

async fn platform_login(
    State(state): State<Arc<AppState>>,
    Extension(auth_rate_limiter): Extension<Arc<AuthRateLimiter>>,
    client_ip: Option<Extension<ResolvedClientIp>>,
    Json(body): Json<LoginBody>,
) -> Result<Response, AppError> {
    let username = body.username.as_deref().unwrap_or("").trim().to_string();
    let password = body.password.as_deref().unwrap_or("").to_string();
    if username.is_empty() || password.is_empty() || username.len() > 128 || password.len() > 1024 {
        return Ok(auth_error_response(
            StatusCode::BAD_REQUEST,
            "AUTH_INVALID_INPUT",
            "username and password are required",
            None,
        ));
    }
    let ip = get_client_ip(client_ip.as_ref());
    let rate = auth_rate_limiter.check_login(&ip, &username, &state.config)?;
    if !rate.allowed {
        let retry_after = std::cmp::max(1, (rate.retry_after_ms + 999) / 1000);
        let mut resp_headers = HeaderMap::new();
        resp_headers.insert("Retry-After", retry_after.to_string().parse().unwrap());
        return Ok(auth_error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "AUTH_RATE_LIMITED",
            "too many login attempts",
            Some(resp_headers),
        ));
    }
    let identity = auth_service::find_platform_identity_by_username_with_repository(
        state.identity_authority_repository(),
        &username,
    )?;
    let Some(identity) = identity else {
        auth_rate_limiter.record_login_failure(&ip, &username, &state.config)?;
        return Ok(auth_error_response(
            StatusCode::UNAUTHORIZED,
            "AUTH_INVALID_CREDENTIALS",
            "invalid credentials",
            None,
        ));
    };
    if !identity.is_enabled || !auth_service::verify_password(&password, &identity.password_hash) {
        auth_rate_limiter.record_login_failure(&ip, &username, &state.config)?;
        return Ok(auth_error_response(
            StatusCode::UNAUTHORIZED,
            "AUTH_INVALID_CREDENTIALS",
            "invalid credentials",
            None,
        ));
    }
    let auth_strength = match totp_login::verify_login_totp_with_repository(
        state.auth_security_repository(),
        &identity.id,
        body.totp_code.as_deref(),
    )? {
        auth_service::TotpLoginResult::NotEnabled => "password",
        auth_service::TotpLoginResult::Verified => "mfa",
        auth_service::TotpLoginResult::Required => {
            auth_rate_limiter.record_login_failure(&ip, &username, &state.config)?;
            return Ok(auth_error_response(
                StatusCode::UNAUTHORIZED,
                "AUTH_MFA_REQUIRED",
                "TOTP verification code is required",
                None,
            ));
        }
        auth_service::TotpLoginResult::Invalid => {
            auth_rate_limiter.record_login_failure(&ip, &username, &state.config)?;
            return Ok(auth_error_response(
                StatusCode::UNAUTHORIZED,
                "AUTH_INVALID_MFA",
                "invalid TOTP verification code",
                None,
            ));
        }
    };
    auth_rate_limiter.record_login_success(&ip, &username)?;
    let session = session_security::create_session_with_repository(
        state.auth_security_repository(),
        &identity.id,
        auth_strength,
        state.config.session_ttl_days,
    )?;
    let max_age = state.config.session_ttl_days * 24 * 60 * 60;
    let cookie = crate::utils::http::build_set_cookie(
        &state.config.auth_cookie_name,
        &session.id,
        max_age,
        state.config.auth_cookie_secure,
        true,
    );
    let authenticated = auth_service::get_auth_user_with_repository(
        state.identity_authority_repository(),
        &session.id,
        None,
    )?
    .ok_or(AppError::Unauthorized)?;
    let mut response = Json(serde_json::json!({
        "ok": true,
        "user": auth_user_payload(&authenticated),
    }))
    .into_response();
    response.headers_mut().insert("Set-Cookie", cookie);
    Ok(response)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginBody {
    username: Option<String>,
    password: Option<String>,
    #[serde(rename = "totpCode")]
    totp_code: Option<String>,
}

fn get_client_ip(resolved: Option<&Extension<ResolvedClientIp>>) -> String {
    resolved
        .map(|resolved| resolved.0.0.to_string())
        .unwrap_or_else(|| "unresolved".to_string())
}

async fn login(
    State(state): State<Arc<AppState>>,
    Extension(auth_rate_limiter): Extension<Arc<AuthRateLimiter>>,
    headers: HeaderMap,
    client_ip: Option<Extension<ResolvedClientIp>>,
    tenant: Option<Extension<Tenant>>,
    Json(body): Json<LoginBody>,
) -> Result<Response, AppError> {
    let username = body.username.as_deref().unwrap_or("").trim().to_string();
    let password = body.password.as_deref().unwrap_or("").to_string();

    if username.is_empty() || password.is_empty() || username.len() > 128 || password.len() > 1024 {
        return Ok(auth_error_response(
            StatusCode::BAD_REQUEST,
            "AUTH_INVALID_INPUT",
            "用户名和密码不能为空",
            None,
        ));
    }

    // Client identity comes only from the P4 trusted-proxy boundary. P7 uses a
    // durable throttling authority keyed by a digest of the resolved IP +
    // normalized username; auth handlers never reinterpret forwarding headers.
    let ip = get_client_ip(client_ip.as_ref());
    let rate = auth_rate_limiter.check_login(&ip, &username, &state.config)?;
    if !rate.allowed {
        let retry_after = std::cmp::max(1, (rate.retry_after_ms + 999) / 1000);
        let mut resp_headers = HeaderMap::new();
        resp_headers.insert("Retry-After", retry_after.to_string().parse().unwrap());
        return Ok(auth_error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "AUTH_RATE_LIMITED",
            "登录尝试过于频繁，请稍后再试",
            Some(resp_headers),
        ));
    }

    let tenant_id = match tenant.as_ref() {
        Some(tenant) => tenant.id.clone(),
        None => {
            return Ok(auth_error_response(
                StatusCode::BAD_REQUEST,
                "TENANT_CONTEXT_REQUIRED",
                "登录请求缺少租户上下文",
                None,
            ));
        }
    };
    let scope = resolved_auth_scope(Some(&tenant_id))?;

    let user = auth_service::find_user_by_username_with_repository(
        state.identity_authority_repository(),
        &scope,
        &username,
    )?;

    // P7 pre-auth contract intentionally collapses account existence,
    // membership/status and password failures. Internal authority checks remain
    // fail-closed, but unauthenticated callers cannot enumerate their reason.
    let user = match user {
        Some(u)
            if u.is_enabled
                && u.tenant_scope_id.as_deref() == Some(&tenant_id)
                && auth_service::verify_password(&password, &u.password_hash) =>
        {
            u
        }
        _ => {
            auth_rate_limiter.record_login_failure(&ip, &username, &state.config)?;
            return Ok(auth_error_response(
                StatusCode::UNAUTHORIZED,
                "AUTH_INVALID_CREDENTIALS",
                "用户名或密码错误",
                None,
            ));
        }
    };

    let auth_strength = match totp_login::verify_login_totp_with_repository(
        state.auth_security_repository(),
        &user.id,
        body.totp_code.as_deref(),
    )? {
        auth_service::TotpLoginResult::NotEnabled => "password",
        auth_service::TotpLoginResult::Verified => "mfa",
        auth_service::TotpLoginResult::Required => {
            auth_rate_limiter.record_login_failure(&ip, &username, &state.config)?;
            return Ok(auth_error_response(
                StatusCode::UNAUTHORIZED,
                "AUTH_MFA_REQUIRED",
                "请输入 TOTP 验证码",
                None,
            ));
        }
        auth_service::TotpLoginResult::Invalid => {
            auth_rate_limiter.record_login_failure(&ip, &username, &state.config)?;
            return Ok(auth_error_response(
                StatusCode::UNAUTHORIZED,
                "AUTH_INVALID_MFA",
                "TOTP 验证码无效",
                None,
            ));
        }
    };
    auth_rate_limiter.record_login_success(&ip, &username)?;

    auth_service::record_successful_login_with_repository(
        state.identity_authority_repository(),
        &user.id,
    )?;

    let session = session_security::create_session_with_repository(
        state.auth_security_repository(),
        &user.id,
        auth_strength,
        state.config.session_ttl_days,
    )?;

    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let now = time::shanghai_now_iso();
    let max_age = state.config.session_ttl_days * 24 * 60 * 60;
    let cookie = crate::utils::http::build_set_cookie(
        &state.config.auth_cookie_name,
        &session.id,
        max_age,
        state.config.auth_cookie_secure,
        true,
    );

    let authenticated = auth_service::get_auth_user_with_repository(
        state.identity_authority_repository(),
        &session.id,
        Some(&tenant_id),
    )?
    .ok_or(AppError::Unauthorized)?;
    let _ = audit_service::write_audit_log_with_repository(
        state.audit_compatibility_repository(),
        "auth_login",
        "auth",
        &user.id,
        &user.username,
        &serde_json::json!({
            "authorityRole": user.authority_role,
            "ip": ip,
            "userAgent": user_agent,
            "loginTime": now,
            "tenantId": user.tenant_scope_id,
        }),
        &authenticated,
    );
    let body = serde_json::json!({ "ok": true, "user": auth_user_payload(&authenticated) });

    let mut response = Json(body).into_response();
    response.headers_mut().insert("Set-Cookie", cookie);
    Ok(response)
}

async fn logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session_token = headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            crate::utils::http::parse_auth_cookie(value, &state.config.auth_cookie_name)
        });
    if let Some(session_token) = session_token {
        let user = auth_service::get_auth_user_with_repository(
            state.identity_authority_repository(),
            &session_token,
            None,
        )
        .ok()
        .flatten();
        let revoked = auth_service::delete_session_with_repository(
            state.auth_security_repository(),
            &session_token,
        )
        .is_ok();

        if let Some(user) = user {
            let _ = audit_service::write_audit_log_with_repository(
                state.audit_compatibility_repository(),
                "auth_logout",
                "auth",
                &user.id,
                &user.username,
                &serde_json::json!({
                    "sessionFingerprint": auth_service::session_token_fingerprint(&session_token),
                    "revoked": revoked,
                }),
                &user,
            );
        }
    }

    let cookie = crate::utils::http::build_clear_cookie(
        &state.config.auth_cookie_name,
        state.config.auth_cookie_secure,
        true,
    );

    let body = serde_json::json!({ "ok": true });
    let mut response = Json(body).into_response();
    response.headers_mut().insert("Set-Cookie", cookie);
    Ok(response)
}

async fn me(auth: AnyAuthorityUser) -> Result<Response, AppError> {
    let body = serde_json::json!({ "user": auth_user_payload(&auth.0) });
    Ok(Json(body).into_response())
}

#[derive(Deserialize)]
struct UpdateProfileBody {
    #[serde(alias = "displayName")]
    display_name: Option<String>,
    email: Option<String>,
    phone: Option<String>,
}

async fn update_profile(
    State(state): State<Arc<AppState>>,
    _headers: HeaderMap,
    auth: AnyAuthorityUser,
    Json(body): Json<UpdateProfileBody>,
) -> Result<Response, AppError> {
    if let Some(ref em) = body.email
        && !em.is_empty()
    {
        let email_re = regex::Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$")
            .map_err(|_| AppError::Internal("regex init failed".into()))?;
        if !email_re.is_match(em) {
            return Ok(auth_error_response(
                StatusCode::BAD_REQUEST,
                "AUTH_INVALID_INPUT",
                "邮箱格式不正确",
                None,
            ));
        }
    }

    if let Some(ref ph) = body.phone
        && !ph.is_empty()
    {
        let phone_re = regex::Regex::new(r"^[+]?[0-9\s-]{7,20}$")
            .map_err(|_| AppError::Internal("regex init failed".into()))?;
        if !phone_re.is_match(ph) {
            return Ok(auth_error_response(
                StatusCode::BAD_REQUEST,
                "AUTH_INVALID_INPUT",
                "手机号格式不正确",
                None,
            ));
        }
    }

    let updated = auth_service::update_identity_profile_with_repository(
        state.identity_authority_repository(),
        &auth.0.id,
        body.display_name.as_deref(),
        body.email.as_deref(),
        body.phone.as_deref(),
    )?;

    let mut updated_fields: Vec<&str> = Vec::new();
    if body.display_name.is_some() {
        updated_fields.push("displayName");
    }
    if body.email.is_some() {
        updated_fields.push("email");
    }
    if body.phone.is_some() {
        updated_fields.push("phone");
    }

    let _ = audit_service::write_audit_log_with_repository(
        state.audit_compatibility_repository(),
        "profile_update",
        "auth",
        &updated.id,
        &updated.username,
        &serde_json::json!({
            "updatedFields": updated_fields,
        }),
        &auth.0,
    );

    let mut refreshed = auth.0.clone();
    refreshed.display_name = updated.display_name;
    refreshed.email = updated.email;
    refreshed.phone = updated.phone;
    let body = serde_json::json!({ "ok": true, "user": auth_user_payload(&refreshed) });
    Ok(Json(body).into_response())
}

#[derive(Deserialize)]
struct ChangePasswordBody {
    #[serde(alias = "oldPassword")]
    old_password: Option<String>,
    #[serde(alias = "currentPassword")]
    current_password: Option<String>,
    #[serde(alias = "newPassword")]
    new_password: Option<String>,
}

async fn change_password(
    State(state): State<Arc<AppState>>,
    Extension(auth_rate_limiter): Extension<Arc<AuthRateLimiter>>,
    client_ip: Option<Extension<ResolvedClientIp>>,
    auth: AnyAuthorityUser,
    Json(body): Json<ChangePasswordBody>,
) -> Result<Response, AppError> {
    let ip = get_client_ip(client_ip.as_ref());
    let rate_key = format!("{}::change-pwd::{}", ip, auth.0.id);
    let rate = auth_rate_limiter.check_custom(&rate_key, &state.config)?;
    if !rate.allowed {
        let retry_after = std::cmp::max(1, (rate.retry_after_ms + 999) / 1000);
        let mut resp_headers = HeaderMap::new();
        resp_headers.insert("Retry-After", retry_after.to_string().parse().unwrap());
        return Ok(auth_error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "AUTH_RATE_LIMITED",
            &format!("请求过于频繁，请 {} 秒后再试", retry_after),
            Some(resp_headers),
        ));
    }

    let old_password = body
        .old_password
        .or(body.current_password)
        .unwrap_or_default();
    let new_password = body.new_password.unwrap_or_default();

    if old_password.is_empty() || new_password.is_empty() {
        return Ok(auth_error_response(
            StatusCode::BAD_REQUEST,
            "AUTH_INVALID_INPUT",
            "原密码和新密码不能为空",
            None,
        ));
    }
    if new_password.len() < 8 {
        return Ok(auth_error_response(
            StatusCode::BAD_REQUEST,
            "AUTH_WEAK_PASSWORD",
            "新密码长度至少 8 位",
            None,
        ));
    }
    if old_password == new_password {
        return Ok(auth_error_response(
            StatusCode::BAD_REQUEST,
            "AUTH_WEAK_PASSWORD",
            "新密码不能与原密码相同",
            None,
        ));
    }

    let user = auth_service::find_identity_by_id_with_repository(
        state.identity_authority_repository(),
        &auth.0.id,
    )?;
    let user = match user {
        Some(u) if u.is_enabled => u,
        _ => {
            return Ok(auth_error_response(
                StatusCode::UNAUTHORIZED,
                "AUTH_UNAUTHORIZED",
                "未登录",
                None,
            ));
        }
    };

    let old_ok = auth_service::verify_password(&old_password, &user.password_hash);
    if !old_ok {
        auth_rate_limiter.record_custom_failure(&rate_key, &state.config)?;
        return Ok(auth_error_response(
            StatusCode::UNAUTHORIZED,
            "AUTH_INVALID_CREDENTIALS",
            "原密码错误",
            None,
        ));
    }

    session_security::rotate_password_and_revoke_sessions_with_repository(
        state.auth_security_repository(),
        &user.id,
        &new_password,
    )?;

    auth_rate_limiter.record_custom_success(&rate_key)?;

    let cookie = crate::utils::http::build_clear_cookie(
        &state.config.auth_cookie_name,
        state.config.auth_cookie_secure,
        true,
    );

    let body = serde_json::json!({
        "ok": true,
        "message": "密码修改成功，请重新登录",
    });
    let mut response = Json(body).into_response();
    response.headers_mut().insert("Set-Cookie", cookie);
    Ok(response)
}

fn auth_error_response(
    status: StatusCode,
    code: &str,
    message: &str,
    extra_headers: Option<HeaderMap>,
) -> Response {
    let body = serde_json::json!({
        "ok": false,
        "code": code,
        "error": message,
    });
    let mut response = (status, Json(body)).into_response();
    if let Some(headers) = extra_headers {
        for (key, value) in headers {
            if let Some(k) = key {
                response.headers_mut().insert(k, value);
            }
        }
    }
    response
}

#[cfg(test)]
mod tenant_scope_tests {
    use super::*;

    #[test]
    fn missing_tenant_cannot_create_login_scope() {
        assert!(matches!(
            resolved_auth_scope(None),
            Err(AppError::Unauthorized)
        ));
        let scope = resolved_auth_scope(Some("tenant-a")).unwrap();
        assert_eq!(scope.tenant_id().as_str(), "tenant-a");
    }

    #[test]
    fn login_rate_key_uses_only_resolved_client_ip_extension() {
        let resolved = Extension(ResolvedClientIp("203.0.113.42".parse().unwrap()));
        assert_eq!(get_client_ip(Some(&resolved)), "203.0.113.42");
        assert_eq!(get_client_ip(None), "unresolved");
    }

    #[test]
    fn login_payload_rejects_caller_selected_session_authority() {
        assert!(
            serde_json::from_value::<LoginBody>(serde_json::json!({
                "username": "alice",
                "password": "password",
                "session": "caller-selected"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<LoginBody>(serde_json::json!({
                "username": "alice",
                "password": "password",
                "token": "caller-selected"
            }))
            .is_err()
        );
    }
}
