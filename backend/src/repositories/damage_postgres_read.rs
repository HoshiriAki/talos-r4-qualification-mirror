#![cfg(feature = "postgres")]

use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::damage::{DamageListItem, DamageListProjection, DamageProjection};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn get_by_order(
    session: &RepositorySession,
    order_id: &str,
) -> Result<Vec<DamageProjection>, RepositoryError> {
    get_by(session, "order_id", order_id)
}

pub(in crate::repositories) fn get_by_device(
    session: &RepositorySession,
    device_serial_no: &str,
) -> Result<Vec<DamageProjection>, RepositoryError> {
    get_by(session, "device_serial_no", device_serial_no)
}

fn get_by(
    session: &RepositorySession,
    column: &'static str,
    value: &str,
) -> Result<Vec<DamageProjection>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let value = value.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            let sql = format!(
                "SELECT id,order_id,device_serial_no,
                        (appearance_ok <> 0) AS appearance_ok,
                        (accessories_ok <> 0) AS accessories_ok,
                        (function_ok <> 0) AS function_ok,
                        damage_description,
                        estimated_damage_amount::double precision AS estimated_damage_amount,
                        liability,status,reported_by,assessed_by,adjudicated_by,
                        reported_at,assessed_at,adjudicated_at,created_at,updated_at
                 FROM damage_reports
                 WHERE tenant_id=$1 AND {column}=$2
                 ORDER BY created_at DESC"
            );
            let rows = sqlx::query(&sql)
                .bind(&tenant_id)
                .bind(&value)
                .fetch_all(&mut *connection)
                .await?;
            rows.iter().map(map_projection).collect()
        })
    })
}

pub(in crate::repositories) fn list(
    session: &RepositorySession,
    status: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<DamageListProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let status = status.filter(|value| !value.is_empty()).map(str::to_owned);
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;

            let mut count =
                QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM damage_reports WHERE ");
            count.push("tenant_id=").push_bind(&tenant_id);
            if let Some(status) = &status {
                count.push(" AND status=").push_bind(status);
            }
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&mut *connection)
                .await?;

            let mut data = QueryBuilder::<Postgres>::new(
                "SELECT id,order_id,device_serial_no,
                        estimated_damage_amount::double precision AS estimated_damage_amount,
                        liability,status,reported_at,assessed_at,adjudicated_at
                 FROM damage_reports WHERE ",
            );
            data.push("tenant_id=").push_bind(&tenant_id);
            if let Some(status) = &status {
                data.push(" AND status=").push_bind(status);
            }
            data.push(" ORDER BY created_at DESC LIMIT ")
                .push_bind(page_size)
                .push(" OFFSET ")
                .push_bind(offset);

            let rows = data.build().fetch_all(&mut *connection).await?;
            let reports = rows
                .iter()
                .map(map_list_item)
                .collect::<Result<Vec<_>, _>>()?;

            Ok(DamageListProjection {
                reports,
                page,
                page_size,
                total,
            })
        })
    })
}

fn map_projection(row: &sqlx::postgres::PgRow) -> Result<DamageProjection, sqlx::Error> {
    Ok(DamageProjection {
        id: row.try_get("id")?,
        order_id: row.try_get("order_id")?,
        device_serial_no: row.try_get("device_serial_no")?,
        appearance_ok: row.try_get("appearance_ok")?,
        accessories_ok: row.try_get("accessories_ok")?,
        function_ok: row.try_get("function_ok")?,
        damage_description: row.try_get("damage_description")?,
        estimated_damage_amount: row.try_get("estimated_damage_amount")?,
        liability: row.try_get("liability")?,
        status: row.try_get("status")?,
        reported_by: row.try_get("reported_by")?,
        assessed_by: row.try_get("assessed_by")?,
        adjudicated_by: row.try_get("adjudicated_by")?,
        reported_at: row.try_get("reported_at")?,
        assessed_at: row.try_get("assessed_at")?,
        adjudicated_at: row.try_get("adjudicated_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_list_item(row: &sqlx::postgres::PgRow) -> Result<DamageListItem, sqlx::Error> {
    Ok(DamageListItem {
        id: row.try_get("id")?,
        order_id: row.try_get("order_id")?,
        device_serial_no: row.try_get("device_serial_no")?,
        estimated_damage_amount: row.try_get("estimated_damage_amount")?,
        liability: row.try_get("liability")?,
        status: row.try_get("status")?,
        reported_at: row.try_get("reported_at")?,
        assessed_at: row.try_get("assessed_at")?,
        adjudicated_at: row.try_get("adjudicated_at")?,
    })
}
