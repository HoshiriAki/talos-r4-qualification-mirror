use chrono::{SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::repositories::sqlite::SqliteRepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone)]
pub struct ImportedOrderDraft {
    pub reconciliation_order_no: String,
    pub start_date: String,
    pub end_date: String,
    pub delivery_date: String,
    pub pickup_methods: Vec<String>,
    pub address: String,
    pub notes: String,
}

/// A closed set of mutable fields allowed while an Order remains draft.
///
/// `None` means that the field is not part of the command.  `Some("")` is
/// intentionally distinct so callers can clear text fields without falling
/// back to the legacy generic update contract.
#[derive(Debug, Clone, Default)]
pub struct DraftOrderPatch {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub delivery_date: Option<String>,
    pub pickup_methods: Option<Vec<String>>,
    pub address: Option<String>,
    pub province: Option<String>,
    pub notes: Option<String>,
}

impl DraftOrderPatch {
    pub fn has_changes(&self) -> bool {
        self.start_date.is_some()
            || self.end_date.is_some()
            || self.delivery_date.is_some()
            || self.pickup_methods.is_some()
            || self.address.is_some()
            || self.province.is_some()
            || self.notes.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedOrderProjection {
    pub id: String,
    pub order_no: String,
    pub status: String,
    pub created_at: String,
}

pub struct ScopedOrderCommandRepository<'a> {
    session: &'a SqliteRepositorySession,
}

impl<'a> ScopedOrderCommandRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub fn create_imported_draft(
        &self,
        draft: ImportedOrderDraft,
    ) -> Result<ImportedOrderProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let pickup_methods = serde_json::to_string(&draft.pickup_methods)
            .map_err(|error| RepositoryError::ContractViolation(error.to_string()))?;
        self.session.write_immediate(|transaction| {
            let collision: Option<String> = transaction
                .query_row(
                    "SELECT tenant_id FROM orders WHERE orderNo = ?1 LIMIT 1",
                    params![draft.reconciliation_order_no],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
            if let Some(owner_tenant) = collision {
                let scope = if owner_tenant == tenant_id {
                    "active tenant"
                } else {
                    "another tenant under the legacy global key constraint"
                };
                return Err(RepositoryError::ContractViolation(format!(
                    "import reconciliation orderNo collision in {scope}"
                )));
            }
            transaction
                .execute(
                    "INSERT INTO orders
                     (id, orderNo, startDate, endDate, deliveryDate, pickupMethods,
                      address, notes, deviceSerialNo, totalPrice, status, createdAt,
                      tenant_id, updatedAt)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '', 0, 'draft', ?9, ?10, ?9)",
                    params![
                        order_id,
                        draft.reconciliation_order_no,
                        draft.start_date,
                        draft.end_date,
                        draft.delivery_date,
                        pickup_methods,
                        draft.address,
                        draft.notes,
                        created_at,
                        tenant_id,
                    ],
                )
                .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
            Ok(())
        })?;
        Ok(ImportedOrderProjection {
            id: order_id,
            order_no: draft.reconciliation_order_no,
            status: "draft".into(),
            created_at,
        })
    }

    /// Apply a named draft patch under the tenant-bound repository session.
    ///
    /// The lifecycle row is re-read inside the write transaction so the
    /// optimistic version check cannot be bypassed by a concurrent lifecycle
    /// command between the application read and this write.
    pub fn update_draft(
        &self,
        order_id: &str,
        expected_version: i64,
        patch: DraftOrderPatch,
    ) -> Result<(), RepositoryError> {
        let order_id = order_id.trim();
        if order_id.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "draft update requires a non-empty order id".into(),
            ));
        }
        if expected_version <= 0 {
            return Err(RepositoryError::ContractViolation(
                "draft update requires a positive expected version".into(),
            ));
        }
        if !patch.has_changes() {
            return Err(RepositoryError::ContractViolation(
                "draft update requires at least one named field".into(),
            ));
        }
        if let Some(methods) = patch.pickup_methods.as_ref() {
            if methods.is_empty() || methods.iter().any(|method| method.trim().is_empty()) {
                return Err(RepositoryError::ContractViolation(
                    "pickupMethods must contain at least one non-blank method".into(),
                ));
            }
        }

        let pickup_methods = patch
            .pickup_methods
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| RepositoryError::ContractViolation(error.to_string()))?;
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();

        self.session.write_immediate(|transaction| {
            let current: Option<(String, i64, String, String, String)> = transaction
                .query_row(
                    "SELECT l.commercial_status, l.version, o.startDate, o.endDate, o.deliveryDate
                     FROM order_lifecycle l
                     JOIN orders o ON o.id = l.order_id AND o.tenant_id = l.tenant_id
                     WHERE l.tenant_id = ?1 AND l.order_id = ?2",
                    params![tenant_id, order_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
            let Some((
                commercial_status,
                actual_version,
                current_start,
                current_end,
                current_delivery,
            )) = current
            else {
                return Err(RepositoryError::ContractViolation(
                    "Order lifecycle not found in the active tenant scope".into(),
                ));
            };
            if actual_version != expected_version {
                return Err(RepositoryError::ContractViolation(format!(
                    "stale Order version: expected {expected_version}, actual {actual_version}"
                )));
            }
            if commercial_status != "draft" {
                return Err(RepositoryError::ContractViolation(
                    "draft fields are only mutable while commercialStatus is draft".into(),
                ));
            }

            let effective_start = patch.start_date.as_deref().unwrap_or(&current_start);
            let effective_end = patch.end_date.as_deref().unwrap_or(&current_end);
            let effective_delivery = patch.delivery_date.as_deref().unwrap_or(&current_delivery);
            for (field, value, allow_empty) in [
                ("startDate", effective_start, false),
                ("endDate", effective_end, false),
                ("deliveryDate", effective_delivery, true),
            ] {
                if (!allow_empty && value.is_empty())
                    || (!value.is_empty() && !is_business_date(value))
                {
                    return Err(RepositoryError::ContractViolation(format!(
                        "{field} must use YYYY-MM-DD"
                    )));
                }
            }
            if effective_start > effective_end {
                return Err(RepositoryError::ContractViolation(
                    "endDate must not be before startDate".into(),
                ));
            }

            let changed = transaction
                .execute(
                    "UPDATE orders SET
                       startDate = COALESCE(?1, startDate),
                       endDate = COALESCE(?2, endDate),
                       deliveryDate = COALESCE(?3, deliveryDate),
                       pickupMethods = COALESCE(?4, pickupMethods),
                       address = COALESCE(?5, address),
                       province = COALESCE(?6, province),
                       notes = COALESCE(?7, notes)
                     WHERE tenant_id = ?8 AND id = ?9",
                    params![
                        patch.start_date,
                        patch.end_date,
                        patch.delivery_date,
                        pickup_methods,
                        patch.address,
                        patch.province,
                        patch.notes,
                        tenant_id,
                        order_id,
                    ],
                )
                .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
            if changed != 1 {
                return Err(RepositoryError::ContractViolation(
                    "Order draft update affected no row in the active tenant scope".into(),
                ));
            }
            Ok(())
        })
    }
}

fn is_business_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use crate::repositories::{
        DraftOrderPatch, ImportedOrderDraft, RepositoryError, RepositoryProvider,
        SqliteRepositoryProvider,
    };
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use rusqlite::params;
    use std::sync::Arc;
    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode,
        NoopHttpClient, PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision,
        TenantId, TenantMembershipId, TenantRole, TenantScope,
    };

    fn pool() -> Pool<SqliteConnectionManager> {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        pool.get().unwrap().execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE orders (id TEXT PRIMARY KEY, orderNo TEXT NOT NULL UNIQUE,
               startDate TEXT NOT NULL, endDate TEXT NOT NULL, deliveryDate TEXT NOT NULL,
               pickupMethods TEXT NOT NULL, address TEXT DEFAULT '', notes TEXT DEFAULT '',
               province TEXT DEFAULT '',
               deviceSerialNo TEXT DEFAULT '', totalPrice REAL DEFAULT 0, status TEXT DEFAULT 'active',
               createdAt TEXT NOT NULL, tenant_id TEXT NOT NULL, updatedAt TEXT, UNIQUE(id, tenant_id));
             CREATE TABLE order_lifecycle (order_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
               commercial_status TEXT NOT NULL, version INTEGER NOT NULL, PRIMARY KEY(order_id, tenant_id));
             CREATE TRIGGER trg_order_lifecycle AFTER INSERT ON orders BEGIN
               INSERT INTO order_lifecycle(order_id, tenant_id, commercial_status, version)
               VALUES(NEW.id, NEW.tenant_id, 'draft', 1);
             END;"
        ).unwrap();
        pool
    }

    fn normal(tenant: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                format!("actor-{tenant}"),
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new(format!("member-{tenant}")).unwrap(),
                    tenant_id: tenant_id.clone(),
                    role: TenantRole::Owner,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("import-test").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new(format!("request-{tenant}")).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn preview(tenant: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                "platform-owner",
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new("platform-member").unwrap(),
                    roles: vec![PlatformRole::Owner],
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("preview-import").unwrap()).unwrap(),
            ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("preview-import").unwrap()),
            RequestId::new("preview-import-request").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn draft(order_no: &str) -> ImportedOrderDraft {
        ImportedOrderDraft {
            reconciliation_order_no: order_no.into(),
            start_date: "2026-09-01".into(),
            end_date: "2026-09-03".into(),
            delivery_date: "2026-09-01".into(),
            pickup_methods: vec!["delivery".into()],
            address: "address".into(),
            notes: "notes".into(),
        }
    }

    #[test]
    fn imported_order_is_tenant_scoped_draft_without_device_binding() {
        let pool = pool();
        let provider = SqliteRepositoryProvider::new(pool.clone());
        let scoped = provider.bind(&normal("tenant-a")).unwrap();
        let created = scoped
            .order_commands()
            .create_imported_draft(draft("10001"))
            .unwrap();
        assert_eq!(created.status, "draft");
        let row: (String, String, String) = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT o.tenant_id, o.deviceSerialNo, l.commercial_status FROM orders o
             JOIN order_lifecycle l ON l.order_id=o.id AND l.tenant_id=o.tenant_id WHERE o.id=?1",
                [created.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("tenant-a".into(), String::new(), "draft".into()));
    }

    #[test]
    fn reconciliation_collision_and_preview_write_fail_closed() {
        let pool = pool();
        let provider = SqliteRepositoryProvider::new(pool);
        let scoped = provider.bind(&normal("tenant-a")).unwrap();
        scoped
            .order_commands()
            .create_imported_draft(draft("10002"))
            .unwrap();
        assert!(matches!(
            scoped
                .order_commands()
                .create_imported_draft(draft("10002")),
            Err(RepositoryError::ContractViolation(_))
        ));
        let preview = provider.bind(&preview("tenant-a")).unwrap();
        assert!(matches!(
            preview
                .order_commands()
                .create_imported_draft(draft("10003"))
                .unwrap_err(),
            RepositoryError::PreviewWriteDenied
        ));
    }

    #[test]
    fn named_draft_update_is_scoped_and_version_checked() {
        let pool = pool();
        let provider = SqliteRepositoryProvider::new(pool.clone());
        let scoped = provider.bind(&normal("tenant-a")).unwrap();
        let created = scoped
            .order_commands()
            .create_imported_draft(draft("10004"))
            .unwrap();
        scoped
            .order_commands()
            .update_draft(
                &created.id,
                1,
                DraftOrderPatch {
                    address: Some("updated".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let address: String = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT address FROM orders WHERE tenant_id=?1 AND id=?2",
                params!["tenant-a", created.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(address, "updated");
        assert!(matches!(
            scoped.order_commands().update_draft(
                &created.id,
                2,
                DraftOrderPatch {
                    notes: Some("stale".into()),
                    ..Default::default()
                },
            ),
            Err(RepositoryError::ContractViolation(message)) if message.contains("stale Order version")
        ));
        let preview = provider.bind(&preview("tenant-a")).unwrap();
        assert!(matches!(
            preview.order_commands().update_draft(
                &created.id,
                1,
                DraftOrderPatch {
                    notes: Some("blocked".into()),
                    ..Default::default()
                },
            ),
            Err(RepositoryError::PreviewWriteDenied)
        ));
    }
}
