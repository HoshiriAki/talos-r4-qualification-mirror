#![cfg(feature = "postgres")]

use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::repair::{
    RepairDeviceStatsProjection, RepairHistoryItem, RepairListItem, RepairListProjection,
    RepairProjection,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn get(
    session: &RepositorySession,
    repair_id: &str,
) -> Result<Option<RepairProjection>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let repair_id = repair_id.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            sqlx::query(
                "SELECT id,damage_report_id,device_serial_no,status,repair_description,
                        repair_cost::double precision AS repair_cost,vendor,created_by,
                        completed_by,returned_by,started_at,completed_at,returned_at,
                        created_at,updated_at
                 FROM repair_orders
                 WHERE tenant_id=$1 AND id=$2
                 LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(&repair_id)
            .fetch_optional(&mut *connection)
            .await?
            .map(|row| map_projection(&row))
            .transpose()
        })
    })
}

pub(in crate::repositories) fn list(
    session: &RepositorySession,
    status: Option<&str>,
    device_serial_no: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<RepairListProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let status = status.filter(|value| !value.is_empty()).map(str::to_owned);
    let device_serial_no = device_serial_no
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;

            let mut count =
                QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM repair_orders WHERE ");
            push_filters(
                &mut count,
                &tenant_id,
                status.as_deref(),
                device_serial_no.as_deref(),
            );
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&mut *connection)
                .await?;

            let mut data = QueryBuilder::<Postgres>::new(
                "SELECT id,damage_report_id,device_serial_no,status,
                        repair_cost::double precision AS repair_cost,vendor,
                        started_at,completed_at,returned_at,created_at
                 FROM repair_orders WHERE ",
            );
            push_filters(
                &mut data,
                &tenant_id,
                status.as_deref(),
                device_serial_no.as_deref(),
            );
            data.push(" ORDER BY created_at DESC LIMIT ")
                .push_bind(page_size)
                .push(" OFFSET ")
                .push_bind(offset);

            let rows = data.build().fetch_all(&mut *connection).await?;
            let repairs = rows
                .iter()
                .map(map_list_item)
                .collect::<Result<Vec<_>, _>>()?;

            Ok(RepairListProjection {
                repairs,
                page,
                page_size,
                total,
            })
        })
    })
}

pub(in crate::repositories) fn device_stats(
    session: &RepositorySession,
    device_serial_no: &str,
) -> Result<RepairDeviceStatsProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let device_serial_no = device_serial_no.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            let totals = sqlx::query(
                "SELECT COUNT(*)::bigint AS total_repairs,
                        COALESCE(SUM(repair_cost),0)::double precision AS total_repair_cost
                 FROM repair_orders
                 WHERE tenant_id=$1 AND device_serial_no=$2",
            )
            .bind(&tenant_id)
            .bind(&device_serial_no)
            .fetch_one(&mut *connection)
            .await?;
            let total_repairs = totals.try_get::<i64, _>("total_repairs")?;
            let total_repair_cost = totals.try_get::<f64, _>("total_repair_cost")?;

            let rows = sqlx::query(
                "SELECT id,status,repair_cost::double precision AS repair_cost,
                        vendor,created_at,completed_at
                 FROM repair_orders
                 WHERE tenant_id=$1 AND device_serial_no=$2
                 ORDER BY created_at DESC LIMIT 10",
            )
            .bind(&tenant_id)
            .bind(&device_serial_no)
            .fetch_all(&mut *connection)
            .await?;
            let recent_repairs = rows
                .iter()
                .map(map_history_item)
                .collect::<Result<Vec<_>, _>>()?;

            Ok(RepairDeviceStatsProjection {
                total_repairs,
                total_repair_cost,
                recent_repairs,
            })
        })
    })
}

fn push_filters<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant_id: &'args str,
    status: Option<&'args str>,
    device_serial_no: Option<&'args str>,
) {
    query.push("tenant_id=").push_bind(tenant_id);
    if let Some(status) = status {
        query.push(" AND status=").push_bind(status);
    }
    if let Some(device_serial_no) = device_serial_no {
        query
            .push(" AND device_serial_no=")
            .push_bind(device_serial_no);
    }
}

fn map_projection(row: &sqlx::postgres::PgRow) -> Result<RepairProjection, sqlx::Error> {
    Ok(RepairProjection {
        id: row.try_get("id")?,
        damage_report_id: row.try_get("damage_report_id")?,
        device_serial_no: row.try_get("device_serial_no")?,
        status: row.try_get("status")?,
        repair_description: row.try_get("repair_description")?,
        repair_cost: row.try_get("repair_cost")?,
        vendor: row.try_get("vendor")?,
        created_by: row.try_get("created_by")?,
        completed_by: row.try_get("completed_by")?,
        returned_by: row.try_get("returned_by")?,
        started_at: row.try_get("started_at")?,
        completed_at: row.try_get("completed_at")?,
        returned_at: row.try_get("returned_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_list_item(row: &sqlx::postgres::PgRow) -> Result<RepairListItem, sqlx::Error> {
    Ok(RepairListItem {
        id: row.try_get("id")?,
        damage_report_id: row.try_get("damage_report_id")?,
        device_serial_no: row.try_get("device_serial_no")?,
        status: row.try_get("status")?,
        repair_cost: row.try_get("repair_cost")?,
        vendor: row.try_get("vendor")?,
        started_at: row.try_get("started_at")?,
        completed_at: row.try_get("completed_at")?,
        returned_at: row.try_get("returned_at")?,
        created_at: row.try_get("created_at")?,
    })
}

fn map_history_item(row: &sqlx::postgres::PgRow) -> Result<RepairHistoryItem, sqlx::Error> {
    Ok(RepairHistoryItem {
        id: row.try_get("id")?,
        status: row.try_get("status")?,
        repair_cost: row.try_get("repair_cost")?,
        vendor: row.try_get("vendor")?,
        created_at: row.try_get("created_at")?,
        completed_at: row.try_get("completed_at")?,
    })
}
