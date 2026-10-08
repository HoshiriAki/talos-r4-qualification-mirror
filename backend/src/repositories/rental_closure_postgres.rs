#![cfg(feature = "postgres")]

use chrono::{SecondsFormat, Utc};
use sha2::{Digest, Sha256};
use sqlx::Row;
use sqlx::postgres::PgConnection;
use uuid::Uuid;

use crate::repositories::rental_closure::{
    InspectionProjection, RentalClosureFacts, ReturnProjection, SettlementProjection,
};
use crate::repositories::{RepositoryError, RepositorySession};

pub(in crate::repositories) struct PostgresRentalClosureRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresRentalClosureRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn receive_allocation(
        &self,
        order_id: &str,
        allocation_id: &str,
        receive_identity: &str,
    ) -> Result<ReturnProjection, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = required(order_id, "orderId")?.to_owned();
        let allocation_id = required(allocation_id, "allocationId")?.to_owned();
        let receive_identity = required(receive_identity, "receiveIdentity")?.to_owned();
        let now = utc_now();

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let allocation = sqlx::query(
                        "SELECT a.id,a.device_serial_no \
                         FROM allocations a \
                         JOIN rental_reservations r \
                           ON r.tenant_id=a.tenant_id AND r.id=a.reservation_id \
                         WHERE a.tenant_id=$1 AND r.order_id=$2 AND a.id=$3 \
                           AND a.status='allocated' \
                         FOR SHARE OF a",
                    )
                    .bind(&tenant)
                    .bind(&order_id)
                    .bind(&allocation_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .ok_or_else(|| {
                        RepositoryError::ContractViolation(
                            "allocation does not belong to the active tenant/order".into(),
                        )
                    })?;
                    let device_serial_no: String =
                        allocation.try_get("device_serial_no").map_err(pg_error)?;

                    let proposed_return_id = Uuid::new_v4().to_string();
                    sqlx::query(
                        "INSERT INTO rental_returns \
                         (id,tenant_id,order_id,status,version,created_at,updated_at) \
                         VALUES ($1,$2,$3,'receiving',1,$4,$4) \
                         ON CONFLICT (tenant_id,order_id) DO NOTHING",
                    )
                    .bind(&proposed_return_id)
                    .bind(&tenant)
                    .bind(&order_id)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    let return_id = sqlx::query_scalar::<_, String>(
                        "SELECT id FROM rental_returns \
                         WHERE tenant_id=$1 AND order_id=$2 FOR UPDATE",
                    )
                    .bind(&tenant)
                    .bind(&order_id)
                    .fetch_one(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    let by_identity = sqlx::query_scalar::<_, String>(
                        "SELECT allocation_id FROM rental_return_items \
                         WHERE tenant_id=$1 AND receive_identity=$2 FOR SHARE",
                    )
                    .bind(&tenant)
                    .bind(&receive_identity)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    if let Some(existing_allocation) = by_identity {
                        if existing_allocation != allocation_id {
                            return Err(RepositoryError::ContractViolation(
                                "receiveIdentity is already bound to another allocation".into(),
                            ));
                        }
                    } else {
                        let existing_identity = sqlx::query_scalar::<_, String>(
                            "SELECT receive_identity FROM rental_return_items \
                             WHERE tenant_id=$1 AND allocation_id=$2 FOR SHARE",
                        )
                        .bind(&tenant)
                        .bind(&allocation_id)
                        .fetch_optional(&mut *connection)
                        .await
                        .map_err(pg_error)?;
                        if let Some(existing_identity) = existing_identity {
                            if existing_identity != receive_identity {
                                return Err(RepositoryError::ContractViolation(
                                    "allocation is already bound to another receiveIdentity".into(),
                                ));
                            }
                        } else {
                            let item_id = Uuid::new_v4().to_string();
                            sqlx::query(
                                "INSERT INTO rental_return_items \
                                 (id,tenant_id,return_id,allocation_id,device_serial_no,receive_identity,status,received_at) \
                                 VALUES ($1,$2,$3,$4,$5,$6,'received',$7)",
                            )
                            .bind(&item_id)
                            .bind(&tenant)
                            .bind(&return_id)
                            .bind(&allocation_id)
                            .bind(&device_serial_no)
                            .bind(&receive_identity)
                            .bind(&now)
                            .execute(&mut *connection)
                            .await
                            .map_err(pg_error)?;

                            sqlx::query(
                                "INSERT INTO rental_inspections \
                                 (id,tenant_id,return_item_id,allocation_id,device_serial_no,status,version,created_at,updated_at) \
                                 VALUES ($1,$2,$3,$4,$5,'pending',1,$6,$6) \
                                 ON CONFLICT (tenant_id,allocation_id) DO NOTHING",
                            )
                            .bind(Uuid::new_v4().to_string())
                            .bind(&tenant)
                            .bind(&item_id)
                            .bind(&allocation_id)
                            .bind(&device_serial_no)
                            .bind(&now)
                            .execute(&mut *connection)
                            .await
                            .map_err(pg_error)?;
                        }
                    }

                    let required_count = sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*)::bigint \
                         FROM allocations a \
                         JOIN rental_reservations r \
                           ON r.tenant_id=a.tenant_id AND r.id=a.reservation_id \
                         WHERE a.tenant_id=$1 AND r.order_id=$2 AND a.status='allocated'",
                    )
                    .bind(&tenant)
                    .bind(&order_id)
                    .fetch_one(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    let item_count = sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*)::bigint FROM rental_return_items \
                         WHERE tenant_id=$1 AND return_id=$2",
                    )
                    .bind(&tenant)
                    .bind(&return_id)
                    .fetch_one(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    if required_count > 0 && item_count == required_count {
                        let changed = sqlx::query(
                            "UPDATE rental_returns \
                             SET status='received',received_at=$3,updated_at=$3,version=version+1 \
                             WHERE tenant_id=$1 AND id=$2 AND status='receiving'",
                        )
                        .bind(&tenant)
                        .bind(&return_id)
                        .bind(&now)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?
                        .rows_affected();

                        if changed == 1 {
                            append_outbox_pg(
                                connection,
                                &tenant,
                                "return",
                                &return_id,
                                "ReturnReceived",
                                &format!("return-received:{return_id}"),
                                &serde_json::json!({
                                    "orderId": order_id,
                                    "returnId": return_id
                                }),
                                &now,
                            )
                            .await?;
                        }
                    }

                    load_return_repo(connection, &tenant, &return_id).await
                })
            })
    }

    pub(in crate::repositories) fn transition_inspection(
        &self,
        inspection_id: &str,
        target: &str,
        expected_version: i64,
    ) -> Result<InspectionProjection, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let inspection_id = required(inspection_id, "inspectionId")?.to_owned();
        let target = required(target, "target")?.to_owned();
        if expected_version < 1 {
            return Err(RepositoryError::ContractViolation(
                "expectedVersion must be positive".into(),
            ));
        }
        let now = utc_now();

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let row = sqlx::query(
                        "SELECT i.status,r.order_id \
                         FROM rental_inspections i \
                         JOIN rental_return_items ri \
                           ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id \
                         JOIN rental_returns r \
                           ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id \
                         WHERE i.tenant_id=$1 AND i.id=$2 AND i.version=$3 \
                         FOR UPDATE OF i",
                    )
                    .bind(&tenant)
                    .bind(&inspection_id)
                    .bind(expected_version)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .ok_or_else(|| {
                        RepositoryError::ContractViolation(
                            "inspection missing or optimistic version conflict".into(),
                        )
                    })?;

                    let current: String = row.try_get("status").map_err(pg_error)?;
                    let order_id: String = row.try_get("order_id").map_err(pg_error)?;
                    let legal = matches!(
                        (current.as_str(), target.as_str()),
                        ("pending", "in_progress")
                            | ("in_progress", "passed")
                            | ("in_progress", "failed")
                    );
                    if !legal {
                        return Err(RepositoryError::ContractViolation(format!(
                            "illegal inspection transition: {current} -> {target}"
                        )));
                    }

                    let changed = sqlx::query(
                        "UPDATE rental_inspections \
                         SET status=$1,version=version+1, \
                             started_at=CASE WHEN $1='in_progress' THEN $4 ELSE started_at END, \
                             completed_at=CASE WHEN $1 IN ('passed','failed') THEN $4 ELSE completed_at END, \
                             updated_at=$4 \
                         WHERE tenant_id=$2 AND id=$3 AND version=$5",
                    )
                    .bind(&target)
                    .bind(&tenant)
                    .bind(&inspection_id)
                    .bind(&now)
                    .bind(expected_version)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .rows_affected();

                    if changed != 1 {
                        return Err(RepositoryError::ContractViolation(
                            "inspection missing or optimistic version conflict".into(),
                        ));
                    }

                    if target == "failed" {
                        sqlx::query(
                            "INSERT INTO rental_damage_reviews \
                             (id,tenant_id,inspection_id,order_id,status,created_at) \
                             VALUES ($1,$2,$3,$4,'open',$5) \
                             ON CONFLICT (tenant_id,inspection_id) DO NOTHING",
                        )
                        .bind(Uuid::new_v4().to_string())
                        .bind(&tenant)
                        .bind(&inspection_id)
                        .bind(&order_id)
                        .bind(&now)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?;
                    }

                    emit_inspection_complete_if_ready_pg(
                        connection,
                        &tenant,
                        &order_id,
                        &now,
                    )
                    .await?;

                    load_inspection_repo(connection, &tenant, &inspection_id).await
                })
            })
    }

    pub(in crate::repositories) fn resolve_damage_review(
        &self,
        inspection_id: &str,
        note: &str,
    ) -> Result<(), RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let inspection_id = required(inspection_id, "inspectionId")?.to_owned();
        let note = required(note, "resolutionNote")?.to_owned();
        let now = utc_now();

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let order_id = sqlx::query_scalar::<_, String>(
                        "SELECT order_id FROM rental_damage_reviews \
                         WHERE tenant_id=$1 AND inspection_id=$2 AND status='open' \
                         FOR UPDATE",
                    )
                    .bind(&tenant)
                    .bind(&inspection_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .ok_or_else(|| {
                        RepositoryError::ContractViolation("open damage review not found".into())
                    })?;

                    let changed = sqlx::query(
                        "UPDATE rental_damage_reviews \
                         SET status='reviewed',resolution_note=$3,resolved_at=$4 \
                         WHERE tenant_id=$1 AND inspection_id=$2 AND status='open'",
                    )
                    .bind(&tenant)
                    .bind(&inspection_id)
                    .bind(&note)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .rows_affected();
                    if changed != 1 {
                        return Err(RepositoryError::ContractViolation(
                            "open damage review not found".into(),
                        ));
                    }

                    let open = sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*)::bigint FROM rental_damage_reviews \
                         WHERE tenant_id=$1 AND order_id=$2 AND status='open'",
                    )
                    .bind(&tenant)
                    .bind(&order_id)
                    .fetch_one(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    if open == 0 {
                        append_outbox_pg(
                            connection,
                            &tenant,
                            "order",
                            &order_id,
                            "RiskCasesResolved",
                            &format!("risk-cases-resolved:{order_id}"),
                            &serde_json::json!({"orderId": order_id}),
                            &now,
                        )
                        .await?;
                    }
                    Ok(())
                })
            })
    }

    pub(in crate::repositories) fn calculate_settlement(
        &self,
        order_id: &str,
        currency: &str,
        amount_minor: i64,
        financial_authority_available: bool,
    ) -> Result<SettlementProjection, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = required(order_id, "orderId")?.to_owned();
        let currency = required(currency, "currency")?.to_ascii_uppercase();
        if currency.len() != 3 {
            return Err(RepositoryError::ContractViolation(
                "currency must be ISO three-letter semantics".into(),
            ));
        }
        let now = utc_now();

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let terminal = sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*)::bigint \
                         FROM rental_inspections i \
                         JOIN rental_return_items ri \
                           ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id \
                         JOIN rental_returns r \
                           ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id \
                         WHERE i.tenant_id=$1 AND r.order_id=$2 \
                           AND i.status IN ('passed','failed')",
                    )
                    .bind(&tenant)
                    .bind(&order_id)
                    .fetch_one(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    let open = sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*)::bigint FROM rental_damage_reviews \
                         WHERE tenant_id=$1 AND order_id=$2 AND status='open'",
                    )
                    .bind(&tenant)
                    .bind(&order_id)
                    .fetch_one(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    let facts = format!(
                        "order={order_id};terminal={terminal};open={open};currency={currency};amount_minor={amount_minor}"
                    );
                    let facts_hash = format!("sha256:{:x}", Sha256::digest(facts.as_bytes()));
                    let (status, blocker): (&str, Option<&str>) =
                        if financial_authority_available && open == 0 {
                            ("calculated", None)
                        } else if open > 0 {
                            ("blocked", Some("DAMAGE_REVIEW_OPEN"))
                        } else {
                            ("blocked", Some("FINANCIAL_AUTHORITY_NOT_AVAILABLE"))
                        };

                    sqlx::query(
                        "INSERT INTO rental_settlements \
                         (id,tenant_id,order_id,currency,amount_minor,facts_hash,status,blocker_code,created_at,updated_at) \
                         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$9) \
                         ON CONFLICT (tenant_id,order_id) DO UPDATE SET \
                           currency=EXCLUDED.currency,amount_minor=EXCLUDED.amount_minor, \
                           facts_hash=EXCLUDED.facts_hash,status=EXCLUDED.status, \
                           blocker_code=EXCLUDED.blocker_code,updated_at=EXCLUDED.updated_at",
                    )
                    .bind(Uuid::new_v4().to_string())
                    .bind(&tenant)
                    .bind(&order_id)
                    .bind(&currency)
                    .bind(amount_minor)
                    .bind(&facts_hash)
                    .bind(status)
                    .bind(blocker)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    load_settlement_repo(connection, &tenant, &order_id).await
                })
            })
    }

    #[cfg(test)]
    pub(in crate::repositories) fn mark_fixture_settlement_terminal(
        &self,
        order_id: &str,
    ) -> Result<(), RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = required(order_id, "orderId")?.to_owned();
        let now = utc_now();

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        "UPDATE rental_settlements \
                         SET status='terminal',blocker_code=NULL,updated_at=$3 \
                         WHERE tenant_id=$1 AND order_id=$2 AND status='calculated'",
                    )
                    .bind(&tenant)
                    .bind(&order_id)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .rows_affected();

                    if changed != 1 {
                        return Err(RepositoryError::ContractViolation(
                            "settlement is not calculated by an explicit deterministic fixture"
                                .into(),
                        ));
                    }

                    append_outbox_pg(
                        connection,
                        &tenant,
                        "order",
                        &order_id,
                        "SettlementComplete",
                        &format!("settlement-complete:{order_id}"),
                        &serde_json::json!({"orderId": order_id}),
                        &now,
                    )
                    .await?;
                    Ok(())
                })
            })
    }

    pub(in crate::repositories) fn inspection(
        &self,
        id: &str,
    ) -> Result<Option<InspectionProjection>, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move { load_inspection_sqlx(connection, &tenant, &id).await })
        })
    }

    pub(in crate::repositories) fn facts(
        &self,
        order_id: &str,
    ) -> Result<RentalClosureFacts, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = required(order_id, "orderId")?.to_owned();

        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let return_received = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*)::bigint FROM rental_returns \
                     WHERE tenant_id=$1 AND order_id=$2 AND status='received'",
                )
                .bind(&tenant)
                .bind(&order_id)
                .fetch_one(&mut *connection)
                .await?;

                let row = sqlx::query(
                    "SELECT COUNT(*)::bigint AS total, \
                            COALESCE(SUM(CASE WHEN i.status IN ('passed','failed') THEN 1 ELSE 0 END),0)::bigint AS terminal \
                     FROM rental_inspections i \
                     JOIN rental_return_items ri \
                       ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id \
                     JOIN rental_returns r \
                       ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id \
                     WHERE i.tenant_id=$1 AND r.order_id=$2",
                )
                .bind(&tenant)
                .bind(&order_id)
                .fetch_one(&mut *connection)
                .await?;
                let total: i64 = row.try_get("total")?;
                let terminal: i64 = row.try_get("terminal")?;

                let open_damage_reviews = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*)::bigint FROM rental_damage_reviews \
                     WHERE tenant_id=$1 AND order_id=$2 AND status='open'",
                )
                .bind(&tenant)
                .bind(&order_id)
                .fetch_one(&mut *connection)
                .await?;

                let settlement_terminal = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*)::bigint FROM rental_settlements \
                     WHERE tenant_id=$1 AND order_id=$2 AND status='terminal'",
                )
                .bind(&tenant)
                .bind(&order_id)
                .fetch_one(&mut *connection)
                .await?;

                Ok(RentalClosureFacts {
                    return_received: return_received == 1,
                    inspection_complete: total > 0 && total == terminal,
                    open_damage_reviews,
                    settlement_terminal: settlement_terminal == 1,
                })
            })
        })
    }
}

async fn emit_inspection_complete_if_ready_pg(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    let row = sqlx::query(
        "SELECT COUNT(*)::bigint AS total, \
                COALESCE(SUM(CASE WHEN i.status IN ('passed','failed') THEN 1 ELSE 0 END),0)::bigint AS terminal \
         FROM rental_inspections i \
         JOIN rental_return_items ri \
           ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id \
         JOIN rental_returns r \
           ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id \
         WHERE i.tenant_id=$1 AND r.order_id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;
    let total: i64 = row.try_get("total").map_err(pg_error)?;
    let terminal: i64 = row.try_get("terminal").map_err(pg_error)?;

    if total > 0 && total == terminal {
        append_outbox_pg(
            connection,
            tenant,
            "order",
            order_id,
            "InspectionComplete",
            &format!("inspection-complete:{order_id}"),
            &serde_json::json!({"orderId": order_id}),
            now,
        )
        .await?;

        let open = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::bigint FROM rental_damage_reviews \
             WHERE tenant_id=$1 AND order_id=$2 AND status='open'",
        )
        .bind(tenant)
        .bind(order_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(pg_error)?;

        if open == 0 {
            append_outbox_pg(
                connection,
                tenant,
                "order",
                order_id,
                "RiskCasesResolved",
                &format!("risk-cases-resolved:{order_id}"),
                &serde_json::json!({"orderId": order_id}),
                now,
            )
            .await?;
        }
    }
    Ok(())
}

async fn load_return_repo(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<ReturnProjection, RepositoryError> {
    let row = sqlx::query(
        "SELECT r.id,r.order_id,r.status,r.version, \
                (SELECT COUNT(*)::bigint FROM rental_return_items ri \
                 WHERE ri.tenant_id=r.tenant_id AND ri.return_id=r.id) AS item_count, \
                (SELECT COUNT(*)::bigint \
                 FROM allocations a \
                 JOIN rental_reservations rr \
                   ON rr.tenant_id=a.tenant_id AND rr.id=a.reservation_id \
                 WHERE a.tenant_id=r.tenant_id AND rr.order_id=r.order_id \
                   AND a.status='allocated') AS required_count \
         FROM rental_returns r WHERE r.tenant_id=$1 AND r.id=$2",
    )
    .bind(tenant)
    .bind(id)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;
    Ok(ReturnProjection {
        id: row.try_get("id").map_err(pg_error)?,
        order_id: row.try_get("order_id").map_err(pg_error)?,
        status: row.try_get("status").map_err(pg_error)?,
        version: row.try_get("version").map_err(pg_error)?,
        item_count: row.try_get("item_count").map_err(pg_error)?,
        required_count: row.try_get("required_count").map_err(pg_error)?,
    })
}

async fn load_inspection_repo(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<InspectionProjection, RepositoryError> {
    load_inspection_sqlx(connection, tenant, id)
        .await
        .map_err(pg_error)?
        .ok_or_else(|| RepositoryError::ContractViolation("inspection not found".into()))
}

async fn load_inspection_sqlx(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<Option<InspectionProjection>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id,allocation_id,device_serial_no,status,version \
         FROM rental_inspections WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?;
    row.map(|row| {
        Ok(InspectionProjection {
            id: row.try_get("id")?,
            allocation_id: row.try_get("allocation_id")?,
            device_serial_no: row.try_get("device_serial_no")?,
            status: row.try_get("status")?,
            version: row.try_get("version")?,
        })
    })
    .transpose()
}

async fn load_settlement_repo(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: &str,
) -> Result<SettlementProjection, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,order_id,currency,amount_minor,facts_hash,status,blocker_code \
         FROM rental_settlements WHERE tenant_id=$1 AND order_id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;
    Ok(SettlementProjection {
        id: row.try_get("id").map_err(pg_error)?,
        order_id: row.try_get("order_id").map_err(pg_error)?,
        currency: row.try_get("currency").map_err(pg_error)?,
        amount_minor: row.try_get("amount_minor").map_err(pg_error)?,
        facts_hash: row.try_get("facts_hash").map_err(pg_error)?,
        status: row.try_get("status").map_err(pg_error)?,
        blocker_code: row.try_get("blocker_code").map_err(pg_error)?,
    })
}

async fn append_outbox_pg(
    connection: &mut PgConnection,
    tenant: &str,
    source_kind: &str,
    source_id: &str,
    message_type: &str,
    idempotency_key: &str,
    payload: &serde_json::Value,
    now: &str,
) -> Result<(), RepositoryError> {
    sqlx::query(
        "INSERT INTO domain_outbox \
         (id,tenant_id,source_kind,source_id,message_type,idempotency_key,payload_json, \
          payload_version,state,available_at,created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,1,'pending',$8,$8) \
         ON CONFLICT (tenant_id,idempotency_key) DO NOTHING",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(tenant)
    .bind(source_kind)
    .bind(source_id)
    .bind(message_type)
    .bind(idempotency_key)
    .bind(payload.to_string())
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(pg_error)?;
    Ok(())
}

fn required<'a>(value: &'a str, name: &str) -> Result<&'a str, RepositoryError> {
    let value = value.trim();
    if value.is_empty() {
        Err(RepositoryError::ContractViolation(format!(
            "{name} must not be blank"
        )))
    } else {
        Ok(value)
    }
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn pg_error(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}
