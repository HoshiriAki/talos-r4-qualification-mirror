#![cfg(feature = "postgres")]

use chrono::{SecondsFormat, Utc};
use sqlx::Row;
use uuid::Uuid;

use crate::repositories::order_commands::{
    DraftOrderPatch, ImportedOrderDraft, ImportedOrderProjection,
};
use crate::repositories::{RepositoryError, RepositorySession};

pub(in crate::repositories) struct PostgresOrderCommandRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresOrderCommandRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn create_imported_draft(
        &self,
        draft: ImportedOrderDraft,
    ) -> Result<ImportedOrderProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let pickup_methods = serde_json::to_string(&draft.pickup_methods)
            .map_err(|error| RepositoryError::ContractViolation(error.to_string()))?;
        let order_no = draft.reconciliation_order_no.clone();
        let order_id_for_tx = order_id.clone();
        let order_no_for_tx = order_no.clone();
        let created_at_for_tx = created_at.clone();

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let collision = sqlx::query_scalar::<_, String>(
                        "SELECT tenant_id FROM orders WHERE orderno = $1 LIMIT 1 FOR SHARE",
                    )
                    .bind(&order_no_for_tx)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?;
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

                    sqlx::query(
                        "INSERT INTO orders \
                         (id, orderno, startdate, enddate, deliverydate, pickupmethods, \
                          address, notes, deviceserialno, totalprice, status, createdat, tenant_id) \
                         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'',0,'draft',$9,$10)",
                    )
                    .bind(&order_id_for_tx)
                    .bind(&order_no_for_tx)
                    .bind(&draft.start_date)
                    .bind(&draft.end_date)
                    .bind(&draft.delivery_date)
                    .bind(&pickup_methods)
                    .bind(&draft.address)
                    .bind(&draft.notes)
                    .bind(&created_at_for_tx)
                    .bind(&tenant_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(|error| map_insert_error(error))?;

                    let lifecycle_version = sqlx::query_scalar::<_, i64>(
                        "SELECT version FROM order_lifecycle \
                         WHERE tenant_id = $1 AND order_id = $2",
                    )
                    .bind(&tenant_id)
                    .bind(&order_id_for_tx)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    if lifecycle_version != Some(1) {
                        return Err(RepositoryError::ContractViolation(
                            "imported draft did not initialize canonical lifecycle version 1".into(),
                        ));
                    }

                    Ok(())
                })
            })?;

        Ok(ImportedOrderProjection {
            id: order_id,
            order_no,
            status: "draft".into(),
            created_at,
        })
    }

    pub(in crate::repositories) fn update_draft(
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
        let order_id = order_id.to_owned();

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let current = sqlx::query(
                        "SELECT l.commercial_status, l.version, \
                                o.startdate, o.enddate, o.deliverydate \
                         FROM order_lifecycle l \
                         JOIN orders o ON o.id = l.order_id AND o.tenant_id = l.tenant_id \
                         WHERE l.tenant_id = $1 AND l.order_id = $2 \
                         FOR UPDATE OF l, o",
                    )
                    .bind(&tenant_id)
                    .bind(&order_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    let Some(current) = current else {
                        return Err(RepositoryError::ContractViolation(
                            "Order lifecycle not found in the active tenant scope".into(),
                        ));
                    };

                    let commercial_status: String =
                        current.try_get("commercial_status").map_err(pg_error)?;
                    let actual_version: i64 = current.try_get("version").map_err(pg_error)?;
                    let current_start: String = current.try_get("startdate").map_err(pg_error)?;
                    let current_end: String = current.try_get("enddate").map_err(pg_error)?;
                    let current_delivery: String =
                        current.try_get("deliverydate").map_err(pg_error)?;

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
                    let effective_delivery =
                        patch.delivery_date.as_deref().unwrap_or(&current_delivery);
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

                    let changed = sqlx::query(
                        "UPDATE orders SET \
                           startdate = COALESCE($1, startdate), \
                           enddate = COALESCE($2, enddate), \
                           deliverydate = COALESCE($3, deliverydate), \
                           pickupmethods = COALESCE($4, pickupmethods), \
                           address = COALESCE($5, address), \
                           province = COALESCE($6, province), \
                           notes = COALESCE($7, notes) \
                         WHERE tenant_id = $8 AND id = $9",
                    )
                    .bind(patch.start_date)
                    .bind(patch.end_date)
                    .bind(patch.delivery_date)
                    .bind(pickup_methods)
                    .bind(patch.address)
                    .bind(patch.province)
                    .bind(patch.notes)
                    .bind(&tenant_id)
                    .bind(&order_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .rows_affected();
                    if changed != 1 {
                        return Err(RepositoryError::ContractViolation(
                            "Order draft update affected no row in the active tenant scope".into(),
                        ));
                    }

                    Ok(())
                })
            })
    }
}

fn map_insert_error(error: sqlx::Error) -> RepositoryError {
    let is_unique = matches!(
        &error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23505")
    );
    if is_unique {
        RepositoryError::ContractViolation(
            "import reconciliation orderNo collision under the legacy global key constraint".into(),
        )
    } else {
        pg_error(error)
    }
}

fn pg_error(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
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
