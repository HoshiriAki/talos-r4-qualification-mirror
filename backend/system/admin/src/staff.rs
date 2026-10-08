//! 员工管理模块 (storage-bound — 连接池 + auth 模块注入)
//!
//! 提供员工的完整 CRUD、密码重置、启用/禁用和工作站会话撤销。
//! 命令:
//! - list_tenant_members: 列出租户成员，按创建时间倒序
//! - create_tenant_member: 创建员工，UUID v4 生成 ID，通过 auth 模块哈希密码
//! - delete_tenant_member: 按 ID 删除员工，最后一位管理员保护
//! - toggle_tenant_member: 启用/禁用员工，禁止自禁用
//! - reset_password: 重置员工密码，强制撤销所有会话
//! - update_username: 修改用户名，唯一性校验
//!
//! 通过 init() 注入 databaseUrl 连接池；密码格式与运行时 AuthService 保持一致。

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde_json::Value;
use std::sync::Mutex;
use system_core::*;

// ══════════════════════════════════════════════════════════════════
// 常量
// ══════════════════════════════════════════════════════════════════

const STAFF_MIN_USERNAME_LENGTH: usize = 2;
const STAFF_MAX_USERNAME_LENGTH: usize = 128;
const PASSWORD_HASH_MAX_LENGTH: usize = 256;

// ══════════════════════════════════════════════════════════════════
// 时间辅助
// ══════════════════════════════════════════════════════════════════

fn shanghai_now_iso() -> String {
    chrono::Utc::now()
        .with_timezone(&chrono_tz::Asia::Shanghai)
        .format("%Y-%m-%dT%H:%M:%S%.3f+08:00")
        .to_string()
}

// ══════════════════════════════════════════════════════════════════
// 错误载荷辅助
// ══════════════════════════════════════════════════════════════════

fn err_payload(category: &str, code: &str, message: &str) -> String {
    serde_json::to_string(&ErrorPayload {
        category: category.into(),
        code: code.into(),
        message: message.into(),
        field: None,
        context: None,
    })
    .unwrap_or_default()
}

fn val_err(field: &str, code: &str, message: &str) -> String {
    serde_json::to_string(&ErrorPayload {
        category: "val".into(),
        code: code.into(),
        message: message.into(),
        field: Some(field.into()),
        context: None,
    })
    .unwrap_or_default()
}

// ══════════════════════════════════════════════════════════════════
// 输入类型 (全部 camelCase + Validate + Sanitize)
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListTenantMembersInput {}

impl Validate for ListTenantMembersInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for ListTenantMembersInput {
    fn sanitize(&mut self) {}
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateTenantMemberInput {
    pub username: String,
    pub password_hash: String,
    pub role: String,
}

impl Validate for CreateTenantMemberInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.username.trim().is_empty() {
            errors.push(FieldError {
                field: "username".into(),
                code: "VAL_REQUIRED".into(),
                message: "用户名不能为空".into(),
            });
        }
        if self.username.len() > STAFF_MAX_USERNAME_LENGTH {
            errors.push(FieldError {
                field: "username".into(),
                code: "VAL_MAX_LENGTH".into(),
                message: format!("用户名最长 {} 个字符", STAFF_MAX_USERNAME_LENGTH),
            });
        }
        if self.password_hash.is_empty() {
            errors.push(FieldError {
                field: "passwordHash".into(),
                code: "VAL_REQUIRED".into(),
                message: "密码凭证不能为空".into(),
            });
        } else if self.password_hash.len() > PASSWORD_HASH_MAX_LENGTH
            || !self.password_hash.starts_with("scrypt$")
        {
            errors.push(FieldError {
                field: "passwordHash".into(),
                code: "VAL_INVALID".into(),
                message: "密码凭证格式无效".into(),
            });
        }
        let role = self.role.trim().to_lowercase();
        if role.is_empty() {
            errors.push(FieldError {
                field: "role".into(),
                code: "VAL_REQUIRED".into(),
                message: "角色不能为空".into(),
            });
        } else if role != "admin" && role != "staff" {
            errors.push(FieldError {
                field: "role".into(),
                code: "VAL_INVALID".into(),
                message: "role 仅支持 admin 或 staff；owner 必须通过所有权流程授予".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for CreateTenantMemberInput {
    fn sanitize(&mut self) {
        self.username = self.username.trim().to_string();
        self.password_hash = self.password_hash.trim().to_string();
        self.role = self.role.trim().to_lowercase();
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevokeTenantMemberInput {
    pub id: String,
}

impl Validate for RevokeTenantMemberInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.trim().is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                code: "VAL_REQUIRED".into(),
                message: "员工 ID 不能为空".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for RevokeTenantMemberInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToggleTenantMemberInput {
    pub id: String,
    pub enabled: Option<bool>,
}

impl Validate for ToggleTenantMemberInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.trim().is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                code: "VAL_REQUIRED".into(),
                message: "员工 ID 不能为空".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ToggleTenantMemberInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordInput {
    pub id: String,
    pub new_password_hash: String,
}

impl Validate for ResetPasswordInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.trim().is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                code: "VAL_REQUIRED".into(),
                message: "员工 ID 不能为空".into(),
            });
        }
        if self.new_password_hash.is_empty() {
            errors.push(FieldError {
                field: "newPasswordHash".into(),
                code: "VAL_REQUIRED".into(),
                message: "新密码凭证不能为空".into(),
            });
        } else if self.new_password_hash.len() > PASSWORD_HASH_MAX_LENGTH
            || !self.new_password_hash.starts_with("scrypt$")
        {
            errors.push(FieldError {
                field: "newPasswordHash".into(),
                code: "VAL_INVALID".into(),
                message: "新密码凭证格式无效".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ResetPasswordInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
        self.new_password_hash = self.new_password_hash.trim().to_string();
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUsernameInput {
    pub id: String,
    pub new_username: String,
}

impl Validate for UpdateUsernameInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.trim().is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                code: "VAL_REQUIRED".into(),
                message: "员工 ID 不能为空".into(),
            });
        }
        let trimmed = self.new_username.trim();
        if trimmed.is_empty() {
            errors.push(FieldError {
                field: "newUsername".into(),
                code: "VAL_REQUIRED".into(),
                message: "用户名不能为空".into(),
            });
        } else if trimmed.len() < STAFF_MIN_USERNAME_LENGTH {
            errors.push(FieldError {
                field: "newUsername".into(),
                code: "VAL_MIN_LENGTH".into(),
                message: format!("用户名至少 {} 个字符", STAFF_MIN_USERNAME_LENGTH),
            });
        } else if trimmed.len() > STAFF_MAX_USERNAME_LENGTH {
            errors.push(FieldError {
                field: "newUsername".into(),
                code: "VAL_MAX_LENGTH".into(),
                message: format!("用户名最长 {} 个字符", STAFF_MAX_USERNAME_LENGTH),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for UpdateUsernameInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
        self.new_username = self.new_username.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════
// 输出类型 (全部 Serialize + JsonSchema, camelCase)
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublicTenantMember {
    pub membership_id: String,
    pub identity_id: String,
    pub username: String,
    pub role: String,
    pub is_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_login_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub user: PublicTenantMember,
    pub revoked_session_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToggleResult {
    pub user: PublicTenantMember,
    pub revoked_session_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordResult {
    pub user: PublicTenantMember,
    pub revoked_session_count: usize,
}

// ══════════════════════════════════════════════════════════════════
// 数据库行映射
// ══════════════════════════════════════════════════════════════════

fn row_to_public(row: &rusqlite::Row) -> rusqlite::Result<PublicTenantMember> {
    Ok(PublicTenantMember {
        membership_id: row.get(0)?,
        identity_id: row.get(1)?,
        username: row.get(2)?,
        role: row.get(3)?,
        is_enabled: row.get::<_, i32>(4)? != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        last_login_at: row.get(7)?,
    })
}

// ══════════════════════════════════════════════════════════════════
// 模块主体
// ══════════════════════════════════════════════════════════════════

/// 员工管理模块
///
/// 通过 r2d2 连接池访问 SQLite 数据库。
/// 可注入 feature-auth 模块用于密码哈希和会话管理。
/// init() 接受 `{"databaseUrl": ":memory:"}` 或实际文件路径，
pub struct FeatureStaff {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureStaff {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self
            .pool
            .lock()
            .map_err(|e| err_payload("sys", "SYS_DB_LOCK", &format!("连接池锁错误: {}", e)))?;
        guard
            .as_ref()
            .ok_or_else(|| err_payload("sys", "SYS_DB_NOT_INIT", "数据库未初始化"))?
            .get()
            .map_err(|e| err_payload("sys", "SYS_DB_POOL", &format!("连接池获取失败: {}", e)))
    }

    // ── 命令实现 ──

    fn find_tenant_member_by_username(
        &self,
        conn: &rusqlite::Connection,
        username: &str,
        _scope: &DataScope,
    ) -> Result<bool, String> {
        conn.query_row(
            "SELECT COUNT(1) FROM identities WHERE username = ?1",
            params![username],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .map_err(|e| err_payload("sys", "SYS_DB_QUERY", &format!("查询用户名失败: {}", e)))
    }

    // 返回 (membership_id, identity_id, username, password_hash, role, membership_active)
    #[allow(clippy::type_complexity)]
    fn find_tenant_member_raw(
        &self,
        conn: &rusqlite::Connection,
        id: &str,
        scope: &DataScope,
    ) -> Result<Option<(String, String, String, String, String, bool)>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT tm.id, i.id, i.username, i.password_hash, tm.role, \
                        CASE WHEN i.status = 'active' AND tm.status = 'active' THEN 1 ELSE 0 END \
                 FROM tenant_memberships tm JOIN identities i ON i.id = tm.identity_id \
                 WHERE tm.id = ?1 AND tm.tenant_id = ?2 LIMIT 1",
            )
            .map_err(|e| err_payload("sys", "SYS_DB_PREPARE", &format!("查询准备失败: {}", e)))?;
        let mut rows = stmt
            .query_map(params![id, scope.tenant_id().as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i32>(5)? != 0,
                ))
            })
            .map_err(|e| err_payload("sys", "SYS_DB_QUERY", &format!("查询员工失败: {}", e)))?;
        rows.next()
            .transpose()
            .map_err(|e| err_payload("sys", "SYS_DB_ROW", &format!("行解析失败: {}", e)))
    }

    fn do_list_tenant_members(
        &self,
        conn: &rusqlite::Connection,
        scope: &DataScope,
    ) -> Result<Vec<PublicTenantMember>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT tm.id, i.id, i.username, tm.role, \
                        CASE WHEN i.status = 'active' AND tm.status = 'active' THEN 1 ELSE 0 END, \
                        tm.created_at, tm.updated_at, i.last_login_at \
                 FROM tenant_memberships tm JOIN identities i ON i.id = tm.identity_id \
                 WHERE tm.tenant_id = ?1 AND tm.status != 'revoked' ORDER BY tm.created_at DESC",
            )
            .map_err(|e| {
                err_payload("sys", "SYS_DB_PREPARE", &format!("列表查询准备失败: {}", e))
            })?;
        let rows = stmt
            .query_map(params![scope.tenant_id().as_str()], row_to_public)
            .map_err(|e| err_payload("sys", "SYS_DB_QUERY", &format!("列表查询失败: {}", e)))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(
                row.map_err(|e| err_payload("sys", "SYS_DB_ROW", &format!("行解析失败: {}", e)))?,
            );
        }
        Ok(results)
    }

    fn do_create_tenant_member(
        &self,
        conn: &rusqlite::Connection,
        input: &CreateTenantMemberInput,
        ctx: &ExecutionContext,
    ) -> Result<PublicTenantMember, String> {
        self.require_role_management(ctx, &input.role)?;
        let scope = ctx.data_scope();
        // 检查用户名唯一性
        if self.find_tenant_member_by_username(conn, &input.username, scope)? {
            return Err(val_err("username", "VAL_DUPLICATE", "用户名已存在"));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let hash = input.password_hash.clone();
        let now = shanghai_now_iso();

        let membership_id = uuid::Uuid::new_v4().to_string();
        let tx = conn.unchecked_transaction().map_err(|e| {
            err_payload(
                "sys",
                "SYS_DB_TRANSACTION",
                &format!("创建员工事务失败: {}", e),
            )
        })?;
        tx.execute(
            "INSERT INTO identities \
             (id, username, email, password_hash, display_name, phone, totp_secret_ciphertext, \
              totp_enabled, status, last_login_at, created_at, updated_at) \
             VALUES (?1, ?2, NULL, ?3, ?2, NULL, NULL, 0, 'active', NULL, ?4, ?4)",
            params![id, input.username, hash, now],
        )
        .map_err(|e| err_payload("sys", "SYS_DB_EXECUTE", &format!("创建身份失败: {}", e)))?;
        tx.execute(
            "INSERT INTO tenant_memberships \
             (id, identity_id, tenant_id, role, status, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?5)",
            params![
                membership_id,
                id,
                scope.tenant_id().as_str(),
                input.role,
                now
            ],
        )
        .map_err(|e| {
            err_payload(
                "sys",
                "SYS_DB_EXECUTE",
                &format!("创建租户成员关系失败: {}", e),
            )
        })?;
        tx.commit().map_err(|e| {
            err_payload(
                "sys",
                "SYS_DB_TRANSACTION",
                &format!("提交员工事务失败: {}", e),
            )
        })?;

        Ok(PublicTenantMember {
            membership_id,
            identity_id: id,
            username: input.username.clone(),
            role: input.role.clone(),
            is_enabled: true,
            created_at: now.clone(),
            updated_at: now,
            last_login_at: None,
        })
    }

    /// Returns true only when removing this active owner/admin membership would
    /// leave no interactive human governance member. Machine identities are
    /// intentionally excluded: their tenant membership is execution authority,
    /// not an interactive governance succession path.
    fn would_remove_last_human_governance_member(
        &self,
        conn: &rusqlite::Connection,
        identity_id: &str,
        role: &str,
        tenant_id: &str,
    ) -> Result<bool, String> {
        if !matches!(role, "owner" | "admin") {
            return Ok(false);
        }
        let target_is_machine: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM machine_identities WHERE identity_id = ?1)",
                params![identity_id],
                |row| row.get(0),
            )
            .map_err(|e| err_payload("sys", "SYS_DB_QUERY", &format!("查询机器成员失败: {}", e)))?;
        if target_is_machine {
            return Ok(false);
        }
        let human_governance_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tenant_memberships tm
                 WHERE tm.role IN ('owner', 'admin') AND tm.status = 'active'
                   AND tm.tenant_id = ?1
                   AND NOT EXISTS (
                     SELECT 1 FROM machine_identities mi WHERE mi.identity_id = tm.identity_id
                   )",
                params![tenant_id],
                |row| row.get(0),
            )
            .map_err(|e| {
                err_payload("sys", "SYS_DB_QUERY", &format!("计算人类管理员失败: {}", e))
            })?;
        Ok(human_governance_count <= 1)
    }

    fn do_delete_tenant_member(
        &self,
        conn: &rusqlite::Connection,
        input: &RevokeTenantMemberInput,
        ctx: &ExecutionContext,
    ) -> Result<DeleteResult, String> {
        let user = self
            .find_tenant_member_raw(conn, &input.id, ctx.data_scope())?
            .ok_or_else(|| val_err("id", "VAL_NOT_FOUND", "员工不存在"))?;
        self.require_role_management(ctx, &user.4)?;

        let actor_id = ctx.user_id().unwrap_or("");

        // 不能删除自己
        if !actor_id.is_empty() && actor_id == user.1 {
            return Err(val_err("id", "VAL_SELF_REF", "不能删除当前登录管理员账号"));
        }

        // A non-interactive machine may execute a tenant command as admin, but
        // cannot be the sole remaining human governance member.
        if self.would_remove_last_human_governance_member(
            conn,
            &user.1,
            &user.4,
            ctx.data_scope().tenant_id().as_str(),
        )? {
            return Err(val_err("role", "VAL_LAST_ADMIN", "不能删除最后一位管理员"));
        }

        conn.execute(
            "UPDATE tenant_memberships SET status = 'revoked', updated_at = ?1 \
             WHERE id = ?2 AND tenant_id = ?3",
            params![
                shanghai_now_iso(),
                user.0,
                ctx.data_scope().tenant_id().as_str()
            ],
        )
        .map_err(|e| err_payload("sys", "SYS_DB_EXECUTE", &format!("删除员工失败: {}", e)))?;

        Ok(DeleteResult {
            user: PublicTenantMember {
                membership_id: user.0,
                identity_id: user.1,
                username: user.2,
                role: user.4,
                is_enabled: user.5,
                created_at: String::new(),
                updated_at: shanghai_now_iso(),
                last_login_at: None,
            },
            // Session belongs to the Identity, not this membership. Authority resolution
            // immediately rejects the revoked membership without breaking other tenants.
            revoked_session_count: 0,
        })
    }

    fn do_toggle_tenant_member(
        &self,
        conn: &rusqlite::Connection,
        input: &ToggleTenantMemberInput,
        ctx: &ExecutionContext,
    ) -> Result<ToggleResult, String> {
        let user = self
            .find_tenant_member_raw(conn, &input.id, ctx.data_scope())?
            .ok_or_else(|| val_err("id", "VAL_NOT_FOUND", "员工不存在"))?;
        self.require_role_management(ctx, &user.4)?;

        let new_enabled = match input.enabled {
            Some(v) => v,
            None => !user.5,
        };

        let actor_id = ctx.user_id().unwrap_or("");

        // 不能禁用自己
        if !new_enabled && !actor_id.is_empty() && actor_id == user.1 {
            return Err(val_err(
                "id",
                "VAL_SELF_DISABLE",
                "不能禁用当前登录管理员账号",
            ));
        }

        if !new_enabled
            && self.would_remove_last_human_governance_member(
                conn,
                &user.1,
                &user.4,
                ctx.data_scope().tenant_id().as_str(),
            )?
        {
            return Err(val_err("role", "VAL_LAST_ADMIN", "不能禁用最后一位管理员"));
        }

        let now = shanghai_now_iso();
        conn.execute(
            "UPDATE tenant_memberships SET status = ?1, updated_at = ?2 \
             WHERE id = ?3 AND tenant_id = ?4 AND status != 'revoked'",
            params![
                if new_enabled { "active" } else { "suspended" },
                now,
                user.0,
                ctx.data_scope().tenant_id().as_str()
            ],
        )
        .map_err(|e| err_payload("sys", "SYS_DB_EXECUTE", &format!("更新状态失败: {}", e)))?;

        Ok(ToggleResult {
            user: PublicTenantMember {
                membership_id: user.0,
                identity_id: user.1,
                username: user.2,
                role: user.4,
                is_enabled: new_enabled,
                created_at: String::new(),
                updated_at: now,
                last_login_at: None,
            },
            // Suspending one membership must not revoke this Identity's sessions for
            // unrelated tenant or platform authorities.
            revoked_session_count: 0,
        })
    }

    fn do_reset_password(
        &self,
        conn: &rusqlite::Connection,
        input: &ResetPasswordInput,
        ctx: &ExecutionContext,
    ) -> Result<ResetPasswordResult, String> {
        let scope = ctx.data_scope();
        let user = self
            .find_tenant_member_raw(conn, &input.id, scope)?
            .ok_or_else(|| val_err("id", "VAL_NOT_FOUND", "员工不存在"))?;
        self.require_role_management(ctx, &user.4)?;

        let hash = input.new_password_hash.clone();

        let tx = conn.unchecked_transaction().map_err(|e| {
            err_payload(
                "sys",
                "SYS_DB_TRANSACTION",
                &format!("重置密码事务失败: {}", e),
            )
        })?;
        self.require_tenant_exclusive_identity(&tx, &user.1)?;
        tx.execute(
            "UPDATE identities SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
            params![hash, shanghai_now_iso(), user.1],
        )
        .map_err(|e| err_payload("sys", "SYS_DB_EXECUTE", &format!("更新密码失败: {}", e)))?;

        let revoked = tx
            .execute(
                "UPDATE auth_sessions SET revoked_at = ?1 \
                 WHERE identity_id = ?2 AND revoked_at IS NULL",
                params![shanghai_now_iso(), user.1],
            )
            .map_err(|e| err_payload("sys", "SYS_DB_EXECUTE", &format!("撤销会话失败: {}", e)))?;
        tx.commit().map_err(|e| {
            err_payload(
                "sys",
                "SYS_DB_TRANSACTION",
                &format!("提交密码重置失败: {}", e),
            )
        })?;

        Ok(ResetPasswordResult {
            user: PublicTenantMember {
                membership_id: user.0,
                identity_id: user.1,
                username: user.2,
                role: user.4,
                is_enabled: user.5,
                created_at: String::new(),
                updated_at: shanghai_now_iso(),
                last_login_at: None,
            },
            revoked_session_count: revoked,
        })
    }

    fn do_update_username(
        &self,
        conn: &rusqlite::Connection,
        input: &UpdateUsernameInput,
        ctx: &ExecutionContext,
    ) -> Result<PublicTenantMember, String> {
        let scope = ctx.data_scope();
        let user = self
            .find_tenant_member_raw(conn, &input.id, scope)?
            .ok_or_else(|| val_err("id", "VAL_NOT_FOUND", "员工不存在"))?;
        self.require_role_management(ctx, &user.4)?;

        self.require_tenant_exclusive_identity(conn, &user.1)?;
        let normalized = input.new_username.trim().to_string();

        // 用户名变更时需要检查唯一性
        if normalized != user.2 && self.find_tenant_member_by_username(conn, &normalized, scope)? {
            return Err(val_err("newUsername", "VAL_DUPLICATE", "用户名已存在"));
        }

        let now = shanghai_now_iso();
        conn.execute(
            "UPDATE identities SET username = ?1, updated_at = ?2 WHERE id = ?3",
            params![normalized, now, user.1],
        )
        .map_err(|e| err_payload("sys", "SYS_DB_EXECUTE", &format!("更新用户名失败: {}", e)))?;

        Ok(PublicTenantMember {
            membership_id: user.0,
            identity_id: user.1,
            username: normalized,
            role: user.4,
            is_enabled: user.5,
            created_at: String::new(),
            updated_at: now,
            last_login_at: None,
        })
    }

    fn require_tenant_exclusive_identity(
        &self,
        conn: &rusqlite::Connection,
        identity_id: &str,
    ) -> Result<(), String> {
        let authority_count: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM tenant_memberships WHERE identity_id = ?1 AND status != 'revoked') + \
                        (SELECT COUNT(*) FROM platform_memberships WHERE identity_id = ?1 AND status != 'revoked')",
                params![identity_id],
                |row| row.get(0),
            )
            .map_err(|e| err_payload("sys", "SYS_DB_QUERY", &format!("检查身份归属失败: {}", e)))?;
        if authority_count != 1 {
            return Err(val_err(
                "id",
                "VAL_SHARED_IDENTITY",
                "共享身份的账号名或凭证只能由身份管理流程修改",
            ));
        }
        Ok(())
    }

    fn require_role_management(
        &self,
        ctx: &ExecutionContext,
        target_role: &str,
    ) -> Result<(), String> {
        match (ctx.tenant_role(), target_role) {
            (_, "owner") => Err(val_err(
                "role",
                "VAL_OWNER_GOVERNANCE_REQUIRED",
                "owner 只能通过租户所有权治理流程管理",
            )),
            (Some(TenantRole::Owner), "admin" | "staff") | (Some(TenantRole::Admin), "staff") => {
                Ok(())
            }
            _ => Err(val_err(
                "role",
                "VAL_ROLE_ESCALATION",
                "当前成员角色不能管理目标角色",
            )),
        }
    }
}

impl Default for FeatureStaff {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureStaff {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "staff".into(),
            version: "0.1.0".into(),
            description: "员工管理模块 — 员工 CRUD + 密码重置 + 会话撤销".into(),
            author: "hoshi".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        if let Some(db_url) = config.get("databaseUrl").and_then(|v| v.as_str()) {
            let manager = SqliteConnectionManager::file(db_url);
            let pool = Pool::builder().max_size(5).build(manager).map_err(|e| {
                err_payload("sys", "SYS_DB_POOL", &format!("创建连接池失败: {}", e))
            })?;
            let mut guard = self
                .pool
                .lock()
                .map_err(|e| err_payload("sys", "SYS_LOCK", &format!("锁获取失败: {}", e)))?;
            *guard = Some(pool);
        }
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        match command {
            "list_tenant_members" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ListTenantMembersInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let _validated = sanitized.validate()?;
                let conn = self.get_conn()?;
                let results = self.do_list_tenant_members(&conn, ctx.data_scope())?;
                serde_json::to_value(results)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            "create_tenant_member" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<CreateTenantMemberInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let conn = self.get_conn()?;
                let result = self.do_create_tenant_member(&conn, &input, ctx)?;
                serde_json::to_value(result)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            "delete_tenant_member" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<RevokeTenantMemberInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let conn = self.get_conn()?;
                let result = self.do_delete_tenant_member(&conn, &input, ctx)?;
                serde_json::to_value(result)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            "toggle_tenant_member" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ToggleTenantMemberInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let conn = self.get_conn()?;
                let result = self.do_toggle_tenant_member(&conn, &input, ctx)?;
                serde_json::to_value(result)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            "reset_password" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ResetPasswordInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let conn = self.get_conn()?;
                let result = self.do_reset_password(&conn, &input, ctx)?;
                serde_json::to_value(result)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            "update_username" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<UpdateUsernameInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let conn = self.get_conn()?;
                let result = self.do_update_username(&conn, &input, ctx)?;
                serde_json::to_value(result)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            _ => Err(err_payload(
                "sys",
                "SYS_UNKNOWN_COMMAND",
                &format!("未知命令: {}", command),
            )),
        }
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        vec![
            system_core::CommandMetadata::new(
                "list_tenant_members",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "create_tenant_member",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "delete_tenant_member",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "toggle_tenant_member",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "reset_password",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "update_username",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "staff".into(),
            description: "员工管理模块 — 员工 CRUD + 密码重置 + 会话撤销".into(),
            commands: vec![
                CommandSchema {
                    name: "list_tenant_members".into(),
                    description: "列出所有员工，按创建时间倒序".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "create_tenant_member".into(),
                    description: "创建员工，UUID v4 生成 ID，密码通过 auth 模块或本地 scrypt 哈希"
                        .into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "delete_tenant_member".into(),
                    description: "按 ID 删除员工，最后一位管理员保护".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "toggle_tenant_member".into(),
                    description: "启用/禁用员工，禁止自禁用".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "reset_password".into(),
                    description: "重置员工密码，强制撤销所有会话".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "update_username".into(),
                    description: "修改用户名，唯一性校验".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tenant_scope_tests {
    use super::*;
    use std::sync::Arc;

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(
            TenantId::new(tenant).unwrap(),
            Revision::new("test-revision").unwrap(),
        )
        .unwrap()
    }

    fn context(tenant: &str, actor_id: &str, role: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        let tenant_role = match role {
            "owner" => TenantRole::Owner,
            "admin" => TenantRole::Admin,
            _ => TenantRole::Staff,
        };
        ExecutionContext::new(
            ActorIdentity::with_authority(
                actor_id,
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new(format!("membership-{actor_id}"))
                        .unwrap(),
                    tenant_id: tenant_id.clone(),
                    role: tenant_role,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("test-revision").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new(format!("request-{tenant}-{actor_id}")).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn connection() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
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
                updated_at TEXT NOT NULL, UNIQUE(identity_id, tenant_id)
            );
            CREATE TABLE platform_memberships (
                id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, status TEXT NOT NULL
            );
            CREATE TABLE auth_sessions (
                id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, revoked_at TEXT
            );
            CREATE TABLE machine_identities (
                identity_id TEXT PRIMARY KEY
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn staff_queries_are_limited_to_effective_data_scope() {
        let conn = connection();
        for (identity, tenant, username) in [
            ("identity-a", "tenant-a", "alice"),
            ("identity-b", "tenant-b", "bob"),
        ] {
            conn.execute(
                "INSERT INTO identities VALUES (?1, ?2, NULL, 'hash', ?2, NULL, NULL, 0, 'active', NULL, 'now', 'now')",
                params![identity, username],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO tenant_memberships VALUES (?1, ?2, ?3, 'staff', 'active', 'now', 'now')",
                params![format!("membership-{identity}"), identity, tenant],
            )
            .unwrap();
        }

        let module = FeatureStaff::new();
        let tenant_a = scope("tenant-a");
        let users = module.do_list_tenant_members(&conn, &tenant_a).unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].username, "alice");

        let selected = module
            .find_tenant_member_raw(&conn, "membership-identity-a", &tenant_a)
            .unwrap()
            .unwrap();
        assert_eq!(selected.2, "alice");
    }

    #[test]
    fn username_uniqueness_is_identity_global() {
        let conn = connection();
        conn.execute(
            "INSERT INTO identities VALUES
             ('a-user', 'shared-name', NULL, 'hash', 'shared-name', NULL, NULL, 0, 'active', NULL, 'now', 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tenant_memberships VALUES
             ('membership-a', 'a-user', 'tenant-a', 'staff', 'active', 'now', 'now')",
            [],
        )
        .unwrap();

        let module = FeatureStaff::new();
        assert!(
            module
                .find_tenant_member_by_username(&conn, "shared-name", &scope("tenant-a"))
                .unwrap()
        );
        assert!(
            module
                .find_tenant_member_by_username(&conn, "shared-name", &scope("tenant-b"))
                .unwrap()
        );
    }

    #[test]
    fn role_hierarchy_and_shared_identity_credentials_are_protected() {
        let conn = connection();
        let module = FeatureStaff::new();
        let admin = context("tenant-a", "admin-a", "admin");
        let owner = context("tenant-a", "owner-a", "owner");

        let create_admin = CreateTenantMemberInput {
            username: "second-admin".into(),
            password_hash: "scrypt$test$hash".into(),
            role: "admin".into(),
        };
        assert!(
            module
                .do_create_tenant_member(&conn, &create_admin, &admin)
                .unwrap_err()
                .contains("VAL_ROLE_ESCALATION")
        );
        let created = module
            .do_create_tenant_member(&conn, &create_admin, &owner)
            .unwrap();

        conn.execute(
            "INSERT INTO tenant_memberships VALUES
             ('membership-b', ?1, 'tenant-b', 'staff', 'active', 'now', 'now')",
            params![created.identity_id],
        )
        .unwrap();
        let reset = ResetPasswordInput {
            id: created.membership_id,
            new_password_hash: "scrypt$replacement$hash".into(),
        };
        assert!(
            module
                .do_reset_password(&conn, &reset, &owner)
                .unwrap_err()
                .contains("VAL_SHARED_IDENTITY")
        );
    }

    #[test]
    fn password_and_session_revocation_commit_atomically() {
        let conn = connection();
        conn.execute_batch(
            "INSERT INTO identities VALUES
             ('staff-a', 'staff-a', NULL, 'scrypt$old$hash', 'staff-a', NULL, NULL, 0,
              'active', NULL, 'now', 'now');
             INSERT INTO tenant_memberships VALUES
             ('membership-a', 'staff-a', 'tenant-a', 'staff', 'active', 'now', 'now');
             INSERT INTO auth_sessions VALUES ('session-a', 'staff-a', NULL);
             CREATE TRIGGER reject_session_revoke
             BEFORE UPDATE OF revoked_at ON auth_sessions
             BEGIN SELECT RAISE(ABORT, 'forced revoke failure'); END;",
        )
        .unwrap();
        let module = FeatureStaff::new();
        let owner = context("tenant-a", "owner-a", "owner");
        let result = module.do_reset_password(
            &conn,
            &ResetPasswordInput {
                id: "membership-a".into(),
                new_password_hash: "scrypt$new$hash".into(),
            },
            &owner,
        );
        assert!(result.is_err());
        let stored: String = conn
            .query_row(
                "SELECT password_hash FROM identities WHERE id = 'staff-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, "scrypt$old$hash");
    }

    #[test]
    fn machine_admin_cannot_be_the_last_human_governance_survivor() {
        let conn = connection();
        conn.execute_batch(
            "INSERT INTO identities VALUES
             ('human-admin', 'human-admin', NULL, 'hash', 'human-admin', NULL, NULL, 0,
              'active', NULL, 'now', 'now'),
             ('machine-admin', 'machine-admin', NULL, '!non-interactive', 'machine-admin', NULL, NULL, 0,
              'active', NULL, 'now', 'now');
             INSERT INTO tenant_memberships VALUES
             ('human-membership', 'human-admin', 'tenant-a', 'admin', 'active', 'now', 'now'),
             ('machine-membership', 'machine-admin', 'tenant-a', 'admin', 'active', 'now', 'now');
             INSERT INTO machine_identities VALUES ('machine-admin');",
        )
        .unwrap();
        let module = FeatureStaff::new();
        let owner = context("tenant-a", "owner-a", "owner");

        let delete = module.do_delete_tenant_member(
            &conn,
            &RevokeTenantMemberInput {
                id: "human-membership".into(),
            },
            &owner,
        );
        assert!(delete.unwrap_err().contains("VAL_LAST_ADMIN"));

        let disable = module.do_toggle_tenant_member(
            &conn,
            &ToggleTenantMemberInput {
                id: "human-membership".into(),
                enabled: Some(false),
            },
            &owner,
        );
        assert!(disable.unwrap_err().contains("VAL_LAST_ADMIN"));
        let status: String = conn
            .query_row(
                "SELECT status FROM tenant_memberships WHERE id = 'human-membership'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "active");
    }
}
