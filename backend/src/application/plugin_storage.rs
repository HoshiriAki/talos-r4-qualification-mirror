//! R4-P6 plugin-owned durable storage.
//!
//! Plugins receive a narrow key/value surface only. They never receive a SQL
//! handle, repository handle, table name, or caller-selected quota. Storage is
//! owned by `(tenant_id, PluginId)` so exact package upgrades retain private
//! state while current execution is still re-authorized against an exact active
//! installation on every operation.

use std::collections::BTreeMap;
use std::sync::Arc;

use system_core::TenantId;
use system_core::security::plugin::PluginPermission;
use system_core::transport::interconnect::PluginId;

use super::plugin_host::{PluginAdmission, PluginHostError};

const ABSOLUTE_MAX_ENTRY_BYTES: u64 = 1024 * 1024;
const ABSOLUTE_MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const ABSOLUTE_MAX_ENTRIES: u64 = 10_000;
const MAX_STORAGE_KEY_BYTES: usize = 192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginStorageQuota {
    pub max_entry_bytes: u64,
    pub max_total_bytes: u64,
    pub max_entries: u64,
}

impl PluginStorageQuota {
    pub fn validate(self) -> Result<Self, PluginStorageError> {
        if self.max_entry_bytes == 0
            || self.max_entry_bytes > ABSOLUTE_MAX_ENTRY_BYTES
            || self.max_total_bytes < self.max_entry_bytes
            || self.max_total_bytes > ABSOLUTE_MAX_TOTAL_BYTES
            || self.max_entries == 0
            || self.max_entries > ABSOLUTE_MAX_ENTRIES
        {
            return Err(PluginStorageError::InvalidQuota);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginStorageEntry {
    pub value: Vec<u8>,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginStorageError {
    Host(PluginHostError),
    InvalidQuota,
    QuotaPolicyMissing,
    QuotaAuthorityChanged,
    InvalidKey,
    EntryTooLarge,
    TotalQuotaExceeded,
    EntryCountQuotaExceeded,
    InstallationInactiveOrMissing,
    Persistence,
    InvalidPersistedState,
}

impl std::fmt::Display for PluginStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PluginStorageError {}

impl From<PluginHostError> for PluginStorageError {
    fn from(value: PluginHostError) -> Self {
        Self::Host(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StorageOwnerKey {
    tenant_id: String,
    plugin_id: String,
}

/// Trusted host policy. The plugin call surface has no quota parameter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginStoragePolicy {
    quotas: BTreeMap<StorageOwnerKey, PluginStorageQuota>,
}

impl PluginStoragePolicy {
    pub(crate) fn new(
        entries: impl IntoIterator<Item = (TenantId, PluginId, PluginStorageQuota)>,
    ) -> Result<Self, PluginStorageError> {
        let mut quotas = BTreeMap::new();
        for (tenant_id, plugin_id, quota) in entries {
            let quota = quota.validate()?;
            let key = StorageOwnerKey {
                tenant_id: tenant_id.as_str().to_owned(),
                plugin_id: plugin_id.as_str().to_owned(),
            };
            if quotas.insert(key, quota).is_some() {
                return Err(PluginStorageError::InvalidQuota);
            }
        }
        Ok(Self { quotas })
    }

    pub(crate) fn deny_all() -> Self {
        Self::default()
    }

    fn quota_for(
        &self,
        admission: &PluginAdmission,
    ) -> Result<PluginStorageQuota, PluginStorageError> {
        self.quotas
            .get(&StorageOwnerKey {
                tenant_id: admission.tenant_id.as_str().to_owned(),
                plugin_id: admission.executable.plugin_id.as_str().to_owned(),
            })
            .copied()
            .ok_or(PluginStorageError::QuotaPolicyMissing)
    }
}

fn authorize_namespace(
    admission: &PluginAdmission,
    namespace: &str,
) -> Result<(), PluginStorageError> {
    admission
        .authorize(&PluginPermission::PluginStorage(namespace.to_owned()))
        .map_err(PluginStorageError::Host)
}

fn validate_key(key: &str) -> Result<(), PluginStorageError> {
    if key.is_empty()
        || key.len() > MAX_STORAGE_KEY_BYTES
        || !key.is_ascii()
        || !key.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/')
        })
    {
        return Err(PluginStorageError::InvalidKey);
    }
    Ok(())
}

fn validate_write(
    admission: &PluginAdmission,
    namespace: &str,
    key: &str,
    value: &[u8],
    policy: &PluginStoragePolicy,
) -> Result<PluginStorageQuota, PluginStorageError> {
    authorize_namespace(admission, namespace)?;
    validate_key(key)?;
    let quota = policy.quota_for(admission)?;
    if value.len() as u64 > quota.max_entry_bytes {
        return Err(PluginStorageError::EntryTooLarge);
    }
    Ok(quota)
}

#[cfg(feature = "sqlite")]
pub struct SqlitePluginStorageService {
    pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
    policy: Arc<PluginStoragePolicy>,
}

#[cfg(feature = "sqlite")]
impl SqlitePluginStorageService {
    pub(crate) fn new(
        pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
        policy: Arc<PluginStoragePolicy>,
    ) -> Self {
        Self { pool, policy }
    }

    pub fn put(
        &self,
        admission: &PluginAdmission,
        namespace: &str,
        key: &str,
        value: &[u8],
    ) -> Result<PluginStorageEntry, PluginStorageError> {
        use rusqlite::{OptionalExtension, TransactionBehavior};

        let quota = validate_write(admission, namespace, key, value, &self.policy)?;
        let mut conn = self
            .pool
            .get()
            .map_err(|_| PluginStorageError::Persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| PluginStorageError::Persistence)?;
        sqlite_require_exact_active_installation(&tx, admission)?;
        sqlite_lock_or_create_owner(&tx, admission, quota)?;

        let existing: Option<(i64, i64)> = tx
            .query_row(
                "SELECT size_bytes,revision FROM plugin_storage_entries \
                 WHERE tenant_id=?1 AND plugin_id=?2 AND namespace=?3 AND entry_key=?4",
                rusqlite::params![
                    admission.tenant_id.as_str(),
                    admission.executable.plugin_id.as_str(),
                    namespace,
                    key,
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|_| PluginStorageError::Persistence)?;

        let (total_bytes, entry_count): (i64, i64) = tx
            .query_row(
                "SELECT COALESCE(SUM(size_bytes),0),COUNT(*) FROM plugin_storage_entries \
                 WHERE tenant_id=?1 AND plugin_id=?2",
                rusqlite::params![
                    admission.tenant_id.as_str(),
                    admission.executable.plugin_id.as_str(),
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| PluginStorageError::Persistence)?;
        if total_bytes < 0
            || entry_count < 0
            || existing.is_some_and(|(size, revision)| size < 0 || revision <= 0)
        {
            return Err(PluginStorageError::InvalidPersistedState);
        }

        let old_size = existing.map(|(size, _)| size as u64).unwrap_or(0);
        let new_size = value.len() as u64;
        let new_total = (total_bytes as u64)
            .checked_sub(old_size)
            .and_then(|bytes| bytes.checked_add(new_size))
            .ok_or(PluginStorageError::InvalidPersistedState)?;
        let new_count = entry_count as u64 + if existing.is_none() { 1 } else { 0 };
        if new_total > quota.max_total_bytes {
            return Err(PluginStorageError::TotalQuotaExceeded);
        }
        if new_count > quota.max_entries {
            return Err(PluginStorageError::EntryCountQuotaExceeded);
        }

        let now = chrono::Utc::now().to_rfc3339();
        tx.execute(
            "INSERT INTO plugin_storage_entries \
             (tenant_id,plugin_id,namespace,entry_key,value_bytes,size_bytes,revision,created_at,updated_at) \
             VALUES (?1,?2,?3,?4,?5,?6,1,?7,?7) \
             ON CONFLICT(tenant_id,plugin_id,namespace,entry_key) DO UPDATE SET \
               value_bytes=excluded.value_bytes,size_bytes=excluded.size_bytes,\
               revision=plugin_storage_entries.revision+1,updated_at=excluded.updated_at",
            rusqlite::params![
                admission.tenant_id.as_str(),
                admission.executable.plugin_id.as_str(),
                namespace,
                key,
                value,
                new_size as i64,
                now,
            ],
        )
        .map_err(|_| PluginStorageError::Persistence)?;
        let revision: i64 = tx
            .query_row(
                "SELECT revision FROM plugin_storage_entries \
                 WHERE tenant_id=?1 AND plugin_id=?2 AND namespace=?3 AND entry_key=?4",
                rusqlite::params![
                    admission.tenant_id.as_str(),
                    admission.executable.plugin_id.as_str(),
                    namespace,
                    key,
                ],
                |row| row.get(0),
            )
            .map_err(|_| PluginStorageError::Persistence)?;
        tx.commit().map_err(|_| PluginStorageError::Persistence)?;
        Ok(PluginStorageEntry {
            value: value.to_vec(),
            revision: u64::try_from(revision)
                .map_err(|_| PluginStorageError::InvalidPersistedState)?,
        })
    }

    pub fn get(
        &self,
        admission: &PluginAdmission,
        namespace: &str,
        key: &str,
    ) -> Result<Option<PluginStorageEntry>, PluginStorageError> {
        use rusqlite::{OptionalExtension, TransactionBehavior};

        authorize_namespace(admission, namespace)?;
        validate_key(key)?;
        let quota = self.policy.quota_for(admission)?;
        let mut conn = self
            .pool
            .get()
            .map_err(|_| PluginStorageError::Persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|_| PluginStorageError::Persistence)?;
        sqlite_require_exact_active_installation(&tx, admission)?;
        sqlite_require_owner_quota(&tx, admission, quota)?;
        let entry: Option<(Vec<u8>, i64)> = tx
            .query_row(
                "SELECT value_bytes,revision FROM plugin_storage_entries \
                 WHERE tenant_id=?1 AND plugin_id=?2 AND namespace=?3 AND entry_key=?4",
                rusqlite::params![
                    admission.tenant_id.as_str(),
                    admission.executable.plugin_id.as_str(),
                    namespace,
                    key,
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|_| PluginStorageError::Persistence)?;
        tx.commit().map_err(|_| PluginStorageError::Persistence)?;
        entry
            .map(|(value, revision)| {
                Ok(PluginStorageEntry {
                    value,
                    revision: u64::try_from(revision)
                        .map_err(|_| PluginStorageError::InvalidPersistedState)?,
                })
            })
            .transpose()
    }
}

#[cfg(feature = "sqlite")]
fn sqlite_require_exact_active_installation(
    tx: &rusqlite::Transaction<'_>,
    admission: &PluginAdmission,
) -> Result<(), PluginStorageError> {
    use rusqlite::OptionalExtension;
    let present: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM plugin_installations i JOIN plugin_packages p \
             ON p.plugin_id=i.plugin_id AND p.plugin_version=i.plugin_version \
             AND p.package_digest_sha256=i.package_digest_sha256 \
             AND p.manifest_digest_sha256=i.manifest_digest_sha256 \
             WHERE i.installation_id=?1 AND i.tenant_id=?2 AND i.plugin_id=?3 \
             AND i.plugin_version=?4 AND i.package_digest_sha256=?5 \
             AND i.manifest_digest_sha256=?6 AND p.capability_contract_version=?7 \
             AND i.lifecycle_state='active' AND p.lifecycle_state='active'",
            rusqlite::params![
                admission.installation_id,
                admission.tenant_id.as_str(),
                admission.executable.plugin_id.as_str(),
                admission.executable.version.as_str(),
                admission.executable.package_digest_sha256,
                admission.executable.manifest_digest_sha256,
                admission.executable.capability_contract_version.as_str(),
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| PluginStorageError::Persistence)?;
    if present.is_none() {
        return Err(PluginStorageError::InstallationInactiveOrMissing);
    }
    Ok(())
}

#[cfg(feature = "sqlite")]
fn sqlite_lock_or_create_owner(
    tx: &rusqlite::Transaction<'_>,
    admission: &PluginAdmission,
    quota: PluginStorageQuota,
) -> Result<(), PluginStorageError> {
    let now = chrono::Utc::now().to_rfc3339();
    tx.execute(
        "INSERT OR IGNORE INTO plugin_storage_owners \
         (tenant_id,plugin_id,max_entry_bytes,max_total_bytes,max_entries,quota_revision,created_at,updated_at) \
         VALUES (?1,?2,?3,?4,?5,1,?6,?6)",
        rusqlite::params![
            admission.tenant_id.as_str(),
            admission.executable.plugin_id.as_str(),
            quota.max_entry_bytes as i64,
            quota.max_total_bytes as i64,
            quota.max_entries as i64,
            now,
        ],
    )
    .map_err(|_| PluginStorageError::Persistence)?;
    sqlite_require_owner_quota(tx, admission, quota)
}

#[cfg(feature = "sqlite")]
fn sqlite_require_owner_quota(
    tx: &rusqlite::Transaction<'_>,
    admission: &PluginAdmission,
    quota: PluginStorageQuota,
) -> Result<(), PluginStorageError> {
    use rusqlite::OptionalExtension;
    let persisted: Option<(i64, i64, i64, i64)> = tx
        .query_row(
            "SELECT max_entry_bytes,max_total_bytes,max_entries,quota_revision \
             FROM plugin_storage_owners WHERE tenant_id=?1 AND plugin_id=?2",
            rusqlite::params![
                admission.tenant_id.as_str(),
                admission.executable.plugin_id.as_str(),
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|_| PluginStorageError::Persistence)?;
    let Some((entry, total, count, revision)) = persisted else {
        return Err(PluginStorageError::QuotaPolicyMissing);
    };
    if revision <= 0
        || entry != quota.max_entry_bytes as i64
        || total != quota.max_total_bytes as i64
        || count != quota.max_entries as i64
    {
        return Err(PluginStorageError::QuotaAuthorityChanged);
    }
    Ok(())
}

#[cfg(feature = "postgres")]
pub struct PostgresPluginStorageService {
    pool: sqlx::PgPool,
    policy: Arc<PluginStoragePolicy>,
}

#[cfg(feature = "postgres")]
impl PostgresPluginStorageService {
    pub(crate) fn new(pool: sqlx::PgPool, policy: Arc<PluginStoragePolicy>) -> Self {
        Self { pool, policy }
    }

    pub async fn put(
        &self,
        admission: &PluginAdmission,
        namespace: &str,
        key: &str,
        value: &[u8],
    ) -> Result<PluginStorageEntry, PluginStorageError> {
        use sqlx::Row;

        let quota = validate_write(admission, namespace, key, value, &self.policy)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| PluginStorageError::Persistence)?;
        postgres_require_exact_active_installation(&mut tx, admission).await?;
        postgres_lock_or_create_owner(&mut tx, admission, quota).await?;

        let existing = sqlx::query(
            "SELECT size_bytes,revision FROM plugin_storage_entries \
             WHERE tenant_id=$1 AND plugin_id=$2 AND namespace=$3 AND entry_key=$4 FOR UPDATE",
        )
        .bind(admission.tenant_id.as_str())
        .bind(admission.executable.plugin_id.as_str())
        .bind(namespace)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| PluginStorageError::Persistence)?;
        let totals = sqlx::query(
            "SELECT COALESCE(SUM(size_bytes),0)::BIGINT AS total_bytes,COUNT(*)::BIGINT AS entry_count \
             FROM plugin_storage_entries WHERE tenant_id=$1 AND plugin_id=$2",
        )
        .bind(admission.tenant_id.as_str())
        .bind(admission.executable.plugin_id.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| PluginStorageError::Persistence)?;
        let total_bytes: i64 = totals
            .try_get("total_bytes")
            .map_err(|_| PluginStorageError::Persistence)?;
        let entry_count: i64 = totals
            .try_get("entry_count")
            .map_err(|_| PluginStorageError::Persistence)?;
        if total_bytes < 0 || entry_count < 0 {
            return Err(PluginStorageError::InvalidPersistedState);
        }
        let old_size = if let Some(row) = &existing {
            let size: i64 = row
                .try_get("size_bytes")
                .map_err(|_| PluginStorageError::Persistence)?;
            let revision: i64 = row
                .try_get("revision")
                .map_err(|_| PluginStorageError::Persistence)?;
            if size < 0 || revision <= 0 {
                return Err(PluginStorageError::InvalidPersistedState);
            }
            size as u64
        } else {
            0
        };
        let new_size = value.len() as u64;
        let new_total = (total_bytes as u64)
            .checked_sub(old_size)
            .and_then(|bytes| bytes.checked_add(new_size))
            .ok_or(PluginStorageError::InvalidPersistedState)?;
        let new_count = entry_count as u64 + if existing.is_none() { 1 } else { 0 };
        if new_total > quota.max_total_bytes {
            return Err(PluginStorageError::TotalQuotaExceeded);
        }
        if new_count > quota.max_entries {
            return Err(PluginStorageError::EntryCountQuotaExceeded);
        }

        let now = chrono::Utc::now().to_rfc3339();
        let row = sqlx::query(
            "INSERT INTO plugin_storage_entries \
             (tenant_id,plugin_id,namespace,entry_key,value_bytes,size_bytes,revision,created_at,updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,1,$7,$7) \
             ON CONFLICT(tenant_id,plugin_id,namespace,entry_key) DO UPDATE SET \
               value_bytes=EXCLUDED.value_bytes,size_bytes=EXCLUDED.size_bytes,\
               revision=plugin_storage_entries.revision+1,updated_at=EXCLUDED.updated_at \
             RETURNING revision",
        )
        .bind(admission.tenant_id.as_str())
        .bind(admission.executable.plugin_id.as_str())
        .bind(namespace)
        .bind(key)
        .bind(value)
        .bind(new_size as i64)
        .bind(&now)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| PluginStorageError::Persistence)?;
        let revision: i64 = row
            .try_get("revision")
            .map_err(|_| PluginStorageError::Persistence)?;
        tx.commit()
            .await
            .map_err(|_| PluginStorageError::Persistence)?;
        Ok(PluginStorageEntry {
            value: value.to_vec(),
            revision: u64::try_from(revision)
                .map_err(|_| PluginStorageError::InvalidPersistedState)?,
        })
    }

    pub async fn get(
        &self,
        admission: &PluginAdmission,
        namespace: &str,
        key: &str,
    ) -> Result<Option<PluginStorageEntry>, PluginStorageError> {
        use sqlx::Row;

        authorize_namespace(admission, namespace)?;
        validate_key(key)?;
        let quota = self.policy.quota_for(admission)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| PluginStorageError::Persistence)?;
        postgres_require_exact_active_installation(&mut tx, admission).await?;
        postgres_require_owner_quota(&mut tx, admission, quota).await?;
        let row = sqlx::query(
            "SELECT value_bytes,revision FROM plugin_storage_entries \
             WHERE tenant_id=$1 AND plugin_id=$2 AND namespace=$3 AND entry_key=$4",
        )
        .bind(admission.tenant_id.as_str())
        .bind(admission.executable.plugin_id.as_str())
        .bind(namespace)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| PluginStorageError::Persistence)?;
        tx.commit()
            .await
            .map_err(|_| PluginStorageError::Persistence)?;
        row.map(|row| {
            let value: Vec<u8> = row
                .try_get("value_bytes")
                .map_err(|_| PluginStorageError::Persistence)?;
            let revision: i64 = row
                .try_get("revision")
                .map_err(|_| PluginStorageError::Persistence)?;
            Ok(PluginStorageEntry {
                value,
                revision: u64::try_from(revision)
                    .map_err(|_| PluginStorageError::InvalidPersistedState)?,
            })
        })
        .transpose()
    }
}

#[cfg(feature = "postgres")]
async fn postgres_require_exact_active_installation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admission: &PluginAdmission,
) -> Result<(), PluginStorageError> {
    let present = sqlx::query_scalar::<_, String>(
        "SELECT i.installation_id FROM plugin_installations i JOIN plugin_packages p \
         ON p.plugin_id=i.plugin_id AND p.plugin_version=i.plugin_version \
         AND p.package_digest_sha256=i.package_digest_sha256 \
         AND p.manifest_digest_sha256=i.manifest_digest_sha256 \
         WHERE i.installation_id=$1 AND i.tenant_id=$2 AND i.plugin_id=$3 \
         AND i.plugin_version=$4 AND i.package_digest_sha256=$5 \
         AND i.manifest_digest_sha256=$6 AND p.capability_contract_version=$7 \
         AND i.lifecycle_state='active' AND p.lifecycle_state='active' FOR UPDATE OF i",
    )
    .bind(&admission.installation_id)
    .bind(admission.tenant_id.as_str())
    .bind(admission.executable.plugin_id.as_str())
    .bind(admission.executable.version.as_str())
    .bind(&admission.executable.package_digest_sha256)
    .bind(&admission.executable.manifest_digest_sha256)
    .bind(admission.executable.capability_contract_version.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| PluginStorageError::Persistence)?;
    if present.is_none() {
        return Err(PluginStorageError::InstallationInactiveOrMissing);
    }
    Ok(())
}

#[cfg(feature = "postgres")]
async fn postgres_lock_or_create_owner(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admission: &PluginAdmission,
    quota: PluginStorageQuota,
) -> Result<(), PluginStorageError> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO plugin_storage_owners \
         (tenant_id,plugin_id,max_entry_bytes,max_total_bytes,max_entries,quota_revision,created_at,updated_at) \
         VALUES ($1,$2,$3,$4,$5,1,$6,$6) ON CONFLICT(tenant_id,plugin_id) DO NOTHING",
    )
    .bind(admission.tenant_id.as_str())
    .bind(admission.executable.plugin_id.as_str())
    .bind(quota.max_entry_bytes as i64)
    .bind(quota.max_total_bytes as i64)
    .bind(quota.max_entries as i64)
    .bind(&now)
    .execute(&mut **tx)
    .await
    .map_err(|_| PluginStorageError::Persistence)?;
    postgres_require_owner_quota(tx, admission, quota).await
}

#[cfg(feature = "postgres")]
async fn postgres_require_owner_quota(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admission: &PluginAdmission,
    quota: PluginStorageQuota,
) -> Result<(), PluginStorageError> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT max_entry_bytes,max_total_bytes,max_entries,quota_revision \
         FROM plugin_storage_owners WHERE tenant_id=$1 AND plugin_id=$2 FOR UPDATE",
    )
    .bind(admission.tenant_id.as_str())
    .bind(admission.executable.plugin_id.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| PluginStorageError::Persistence)?
    .ok_or(PluginStorageError::QuotaPolicyMissing)?;
    let entry: i64 = row
        .try_get("max_entry_bytes")
        .map_err(|_| PluginStorageError::Persistence)?;
    let total: i64 = row
        .try_get("max_total_bytes")
        .map_err(|_| PluginStorageError::Persistence)?;
    let count: i64 = row
        .try_get("max_entries")
        .map_err(|_| PluginStorageError::Persistence)?;
    let revision: i64 = row
        .try_get("quota_revision")
        .map_err(|_| PluginStorageError::Persistence)?;
    if revision <= 0
        || entry != quota.max_entry_bytes as i64
        || total != quota.max_total_bytes as i64
        || count != quota.max_entries as i64
    {
        return Err(PluginStorageError::QuotaAuthorityChanged);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_is_bounded_by_host_absolute_limits() {
        assert!(
            PluginStorageQuota {
                max_entry_bytes: 1024,
                max_total_bytes: 4096,
                max_entries: 16,
            }
            .validate()
            .is_ok()
        );
        assert_eq!(
            PluginStorageQuota {
                max_entry_bytes: ABSOLUTE_MAX_ENTRY_BYTES + 1,
                max_total_bytes: ABSOLUTE_MAX_TOTAL_BYTES,
                max_entries: 1,
            }
            .validate(),
            Err(PluginStorageError::InvalidQuota)
        );
    }

    #[test]
    fn keys_are_bounded_and_never_table_or_sql_identifiers() {
        for valid in ["cache-key", "analysis/v1:item_2", "a.b"] {
            assert_eq!(validate_key(valid), Ok(()));
        }
        for invalid in ["", "has space", "x;DROP", "../\\escape"] {
            assert_eq!(validate_key(invalid), Err(PluginStorageError::InvalidKey));
        }
    }

    #[test]
    fn caller_cannot_supply_or_raise_its_own_quota() {
        let policy = PluginStoragePolicy::new([(
            TenantId::new("tenant-a").unwrap(),
            PluginId::new("official.fixture").unwrap(),
            PluginStorageQuota {
                max_entry_bytes: 1024,
                max_total_bytes: 4096,
                max_entries: 16,
            },
        )])
        .unwrap();
        assert_eq!(policy.quotas.len(), 1);
    }
}
