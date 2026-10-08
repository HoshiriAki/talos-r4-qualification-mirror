use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppError;
use crate::middleware::auth::AdminUser;
use crate::registry::make_ctx;
use crate::services::auth_service;
use crate::state::AppState;

fn map_staff_error(raw: String) -> AppError {
    let payload: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(payload) => payload,
        Err(_) => return AppError::from_error_payload(raw),
    };
    let code = payload
        .get("code")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let message = payload
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or(&raw)
        .to_string();
    match code {
        "VAL_REQUIRED"
        | "VAL_MIN_LENGTH"
        | "VAL_INVALID"
        | "VAL_SELF_REF"
        | "VAL_SELF_DISABLE"
        | "VAL_LAST_ADMIN"
        | "VAL_OWNERSHIP_TARGET" => AppError::BadRequest(message),
        "VAL_DUPLICATE" | "VAL_SHARED_IDENTITY" | "BIZ_TENANT_OWNER_STALE" => {
            AppError::Conflict(message)
        }
        "VAL_NOT_FOUND" => AppError::NotFound(message),
        "MACHINE_OWNER_FORBIDDEN" => AppError::CodedForbidden {
            code: code.to_string(),
            message,
        },
        "VAL_OWNER_GOVERNANCE_REQUIRED" | "VAL_ROLE_ESCALATION" => AppError::Forbidden,
        _ => AppError::from_error_payload(raw),
    }
}

fn consume_credential_mutation_budget(
    state: &AppState,
    admin: &AdminUser,
    operation: &str,
) -> Result<(), AppError> {
    let tenant_id = admin.0.tenant_id().unwrap_or("unresolved");
    let key = format!("tenant-credential:{operation}:{tenant_id}:{}", admin.0.id);
    let rate = auth_service::consume_custom_budget(&key, &state.config);
    if !rate.allowed {
        return Err(AppError::RateLimited {
            retry_after_secs: ((rate.retry_after_ms + 999) / 1000).max(1) as u64,
        });
    }
    Ok(())
}

pub fn tenant_membership_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tenant-memberships", get(list_users).post(create_user))
        .route("/tenant-memberships/{id}", delete(delete_user))
        .route(
            "/tenant-memberships/{id}/reset-password",
            post(reset_password),
        )
        .route(
            "/tenant-memberships/{id}/toggle-enabled",
            post(toggle_enabled),
        )
        .route("/tenant-memberships/{id}/username", post(rename_user))
        .route(
            "/tenant-memberships/{id}/transfer-ownership",
            post(transfer_ownership),
        )
}

async fn transfer_ownership(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Path(target_membership_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let current_membership_id = match &admin.0.authority {
        system_core::AuthorityContext::Tenant {
            membership_id,
            role: system_core::TenantRole::Owner,
            ..
        } => membership_id.as_str(),
        _ => return Err(AppError::Forbidden),
    };
    if current_membership_id == target_membership_id {
        return Err(AppError::BadRequest("目标成员已经是租户 Owner".into()));
    }

    let ctx = make_ctx(&admin.0, state.http_client.clone());
    let transfer = state
        .registry
        .execute(
            "staff",
            "transfer_ownership",
            serde_json::json!({ "id": target_membership_id }),
            &ctx,
        )
        .map_err(map_staff_error)?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "tenantId": transfer["tenantId"],
        "previousOwnerMembershipId": transfer["previousOwnerMembershipId"],
        "ownerMembershipId": transfer["ownerMembershipId"],
    })))
}

async fn list_users(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&admin.0, state.http_client.clone());
    let memberships = state
        .registry
        .execute("staff", "list_tenant_members", serde_json::json!({}), &ctx)
        .map_err(map_staff_error)?;
    Ok(Json(serde_json::json!({ "memberships": memberships })))
}

#[derive(Deserialize)]
struct CreateUserBody {
    username: Option<String>,
    password: Option<String>,
    role: Option<String>,
}

async fn create_user(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Json(body): Json<CreateUserBody>,
) -> Result<Response, AppError> {
    let username = body.username.unwrap_or_default();
    let password = body.password.unwrap_or_default();
    let role = body.role.unwrap_or_default();

    // Preserve the legacy endpoint's response codes for its established validation contract.
    if username.is_empty() || username.len() > 128 {
        return Ok((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "ok": false,
                "code": "AUTH_INVALID_INPUT",
                "error": "用户名不能为空"
            })),
        )
            .into_response());
    }
    if password.len() < 8 || password.len() > 1024 {
        return Ok((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "ok": false,
                "code": "AUTH_WEAK_PASSWORD",
                "error": "密码长度至少 8 位"
            })),
        )
            .into_response());
    }
    if !["admin", "staff"].contains(&role.as_str()) {
        return Ok((
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "ok": false,
                "code": "AUTH_INVALID_INPUT",
                "error": "角色必须为 admin 或 staff"
            })),
        )
            .into_response());
    }

    consume_credential_mutation_budget(&state, &admin, "create")?;
    let ctx = make_ctx(&admin.0, state.http_client.clone());
    let password_hash = auth_service::hash_password(&password);
    let member = state
        .registry
        .execute(
            "staff",
            "create_tenant_member",
            serde_json::json!({
                "username": username,
                "passwordHash": password_hash,
                "role": role,
            }),
            &ctx,
        )
        .map_err(map_staff_error)?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({ "ok": true, "member": member })),
    )
        .into_response())
}

#[derive(Deserialize)]
struct ResetPasswordBody {
    #[serde(alias = "newPassword")]
    new_password: Option<String>,
}

async fn reset_password(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<ResetPasswordBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let new_password = body.new_password.unwrap_or_default();
    if new_password.len() < 8 || new_password.len() > 1024 {
        return Err(AppError::BadRequest(
            "新密码长度必须为 8 到 1024 个字符".into(),
        ));
    }
    consume_credential_mutation_budget(&state, &admin, "reset-password")?;
    let new_password_hash = auth_service::hash_password(&new_password);
    let ctx = make_ctx(&admin.0, state.http_client.clone());
    let result = state
        .registry
        .execute(
            "staff",
            "reset_password",
            serde_json::json!({
                "id": id,
                "newPasswordHash": new_password_hash,
            }),
            &ctx,
        )
        .map_err(map_staff_error)?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "revokedSessionCount": result["revokedSessionCount"],
    })))
}

#[derive(Deserialize)]
struct ToggleEnabledBody {
    #[serde(alias = "isEnabled")]
    is_enabled: Option<bool>,
}

async fn toggle_enabled(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<ToggleEnabledBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&admin.0, state.http_client.clone());
    let result = state
        .registry
        .execute(
            "staff",
            "toggle_tenant_member",
            serde_json::json!({ "id": id, "enabled": body.is_enabled }),
            &ctx,
        )
        .map_err(map_staff_error)?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "member": result["user"],
        "revokedSessionCount": result["revokedSessionCount"],
    })))
}

async fn delete_user(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&admin.0, state.http_client.clone());
    let result = state
        .registry
        .execute(
            "staff",
            "delete_tenant_member",
            serde_json::json!({ "id": id }),
            &ctx,
        )
        .map_err(map_staff_error)?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "revokedSessionCount": result["revokedSessionCount"],
    })))
}

#[derive(Deserialize)]
struct RenameBody {
    username: Option<String>,
}

async fn rename_user(
    State(state): State<Arc<AppState>>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<RenameBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ctx = make_ctx(&admin.0, state.http_client.clone());
    let member = state
        .registry
        .execute(
            "staff",
            "update_username",
            serde_json::json!({
                "id": id,
                "newUsername": body.username.unwrap_or_default(),
            }),
            &ctx,
        )
        .map_err(map_staff_error)?;

    Ok(Json(serde_json::json!({ "ok": true, "member": member })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_admin::FeatureStaff;
    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, Namespace,
        NoopHttpClient, RequestId, Revision, SystemModule, TenantId, TenantMembershipId,
        TenantRole, TenantScope,
    };

    fn context(tenant: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                "route-admin",
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new(format!("membership-{tenant}")).unwrap(),
                    tenant_id: tenant_id.clone(),
                    role: TenantRole::Admin,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::new(
                tenant_id,
                Namespace::Production,
                Revision::new("r1").unwrap(),
            )
            .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(format!("request-{tenant}")).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn maxwell_staff_errors_keep_legacy_http_categories() {
        let bad_request = map_staff_error(
            serde_json::json!({ "code": "VAL_MIN_LENGTH", "message": "too short" }).to_string(),
        );
        let conflict = map_staff_error(
            serde_json::json!({ "code": "VAL_DUPLICATE", "message": "duplicate" }).to_string(),
        );
        assert!(matches!(bad_request, AppError::BadRequest(_)));
        assert!(matches!(conflict, AppError::Conflict(_)));
    }

    #[test]
    fn staff_registry_path_keeps_tenant_data_isolated() {
        let db_path =
            std::env::temp_dir().join(format!("talos-staff-route-{}.db", uuid::Uuid::new_v4()));
        let pool = Pool::builder()
            .max_size(4)
            .build(SqliteConnectionManager::file(&db_path))
            .unwrap();
        pool.get()
            .unwrap()
            .execute_batch(
                "CREATE TABLE identities (
                   id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE, email TEXT,
                   password_hash TEXT NOT NULL, display_name TEXT NOT NULL, phone TEXT,
                   totp_secret_ciphertext TEXT, totp_enabled INTEGER NOT NULL,
                   status TEXT NOT NULL, last_login_at TEXT, created_at TEXT NOT NULL,
                   updated_at TEXT NOT NULL
                 );
                 CREATE TABLE tenant_memberships (
                   id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
                   role TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL,
                   updated_at TEXT NOT NULL
                 );
                 CREATE TABLE platform_memberships (
                   id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, status TEXT NOT NULL
                 );
                 CREATE TABLE auth_sessions (
                   id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, revoked_at TEXT
                 );
                 INSERT INTO identities VALUES
                   ('user-a', 'alice', NULL, 'hash', 'alice', NULL, NULL, 0, 'active', NULL, 'now', 'now'),
                   ('user-b', 'bob', NULL, 'hash', 'bob', NULL, NULL, 0, 'active', NULL, 'now', 'now');
                 INSERT INTO tenant_memberships VALUES
                   ('membership-a', 'user-a', 'tenant-a', 'staff', 'active', 'now', 'now'),
                   ('membership-b', 'user-b', 'tenant-b', 'staff', 'active', 'now', 'now');",
            )
            .unwrap();

        let module = FeatureStaff {
            pool: std::sync::Mutex::new(Some(pool.clone())),
        };
        let mut modules: HashMap<String, Arc<dyn SystemModule>> = HashMap::new();
        modules.insert("staff".into(), Arc::new(module));
        let registry = crate::registry::ModuleRegistry::new(modules).unwrap();

        let tenant_a = context("tenant-a");
        let users = registry
            .execute(
                "staff",
                "list_tenant_members",
                serde_json::json!({}),
                &tenant_a,
            )
            .unwrap();
        assert_eq!(users.as_array().unwrap().len(), 1);
        assert_eq!(users[0]["username"], "alice");

        let cross_tenant_update = registry.execute(
            "staff",
            "update_username",
            serde_json::json!({ "id": "membership-b", "newUsername": "intrusion" }),
            &tenant_a,
        );
        assert!(cross_tenant_update.is_err());

        drop(registry);
        drop(pool);
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn ownership_transfer_rejects_machine_target_and_allows_human_target() {
        use crate::application::{
            ApplicationServices, LegacyMaintenanceAdapter, MaintenanceWorkerRunner, SystemClock,
            TenantMembershipCompatibilityModule, WorkerContextFactory,
        };
        use crate::auth_contract::AuthUserInfo;

        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch(crate::db::baseline::SQLITE_IDENTITY_AUTHORITY_BASELINE)
            .unwrap();
        conn.execute_batch(include_str!("../db/migrations/066_r3_machine_api.sql"))
            .unwrap();
        conn.execute_batch(
            "INSERT INTO tenants VALUES ('tenant-a','A','a','active','free',NULL,'now','now');
             INSERT INTO identities (id,username,password_hash,display_name,status,created_at,updated_at) VALUES
             ('owner','owner','hash','Owner','active','now','now'),
             ('human','human','hash','Human','active','now','now'),
             ('machine','machine','!non-interactive','Machine','active','now','now');
             INSERT INTO tenant_memberships (id,identity_id,tenant_id,role,status,created_at,updated_at) VALUES
             ('owner-membership','owner','tenant-a','owner','active','now','now'),
             ('human-membership','human','tenant-a','staff','active','now','now'),
             ('machine-membership','machine','tenant-a','admin','active','now','now');
             INSERT INTO machine_identities (identity_id,created_at) VALUES ('machine','now');",
        )
        .unwrap();
        drop(conn);

        let staff_module = TenantMembershipCompatibilityModule::new(
            crate::repositories::TenantMembershipAuthorityRepository::new(pool.clone()),
        );
        let mut modules: HashMap<String, Arc<dyn SystemModule>> = HashMap::new();
        modules.insert("staff".into(), Arc::new(staff_module));
        let registry = Arc::new(crate::registry::ModuleRegistry::new(modules).unwrap());
        let http: Arc<dyn system_core::transport::http_client::HttpClient> =
            Arc::new(NoopHttpClient);
        let worker = Arc::new(MaintenanceWorkerRunner::new(
            WorkerContextFactory::new(Arc::new(SystemClock), http.clone()),
            Arc::new(LegacyMaintenanceAdapter::new(pool.clone())),
        ));
        let services = Arc::new(ApplicationServices::production(
            registry.clone(),
            Arc::new(SystemClock),
            worker,
            Arc::new(crate::repositories::SqliteRepositoryProvider::new(
                pool.clone(),
            )),
        ));
        let state = Arc::new(AppState::new_local(
            pool.clone(),
            crate::config::AppConfig::from_env(),
            registry,
            http,
            Arc::new(
                crate::integration::webhook::FixtureWebhookIngress::new_with_metrics(
                    crate::integration::store::IntegrationStore::new(pool.clone()),
                    Arc::new(crate::observability::RuntimeMetrics::default()),
                ),
            ),
            services,
        ));
        let owner = AuthUserInfo {
            id: "owner".into(),
            username: "owner".into(),
            session_id: "session".into(),
            display_name: "Owner".into(),
            email: String::new(),
            phone: String::new(),
            authority: AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new("owner-membership").unwrap(),
                tenant_id: TenantId::new("tenant-a").unwrap(),
                role: TenantRole::Owner,
            },
        };

        let rejected = transfer_ownership(
            State(state.clone()),
            AdminUser(owner.clone()),
            Path("machine-membership".into()),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            rejected,
            AppError::CodedForbidden { ref code, .. } if code == "MACHINE_OWNER_FORBIDDEN"
        ));

        let transferred = transfer_ownership(
            State(state),
            AdminUser(owner),
            Path("human-membership".into()),
        )
        .await
        .unwrap();
        assert_eq!(transferred.0["ok"], true);
        let conn = pool.get().unwrap();
        let current_owner: String = conn
            .query_row(
                "SELECT identity_id FROM tenant_memberships WHERE role='owner' AND tenant_id='tenant-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(current_owner, "human");
    }
}
