#![cfg(feature = "postgres")]

use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::barcode::{
    BarcodeDeviceInfoProjection, BarcodeLookupProjection, ScanEventProjection,
    ScanHistoryProjection, ScanStatsProjection, stats_projection,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn lookup(
    session: &RepositorySession,
    barcode_text: &str,
) -> Result<Option<BarcodeLookupProjection>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let barcode_text = barcode_text.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            sqlx::query(
                "SELECT bl.id,bl.device_serial_no,bl.barcode_text,bl.barcode_type,
                        bl.label_format,bl.generated_at,
                        d.serialno,d.modelid,d.rentalstatus,d.warning_status AS status,d.currentwarehouseid
                 FROM barcode_labels bl
                 JOIN devices d ON d.serialno=bl.device_serial_no
                 WHERE d.tenant_id=$1 AND bl.barcode_text=$2
                 LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(&barcode_text)
            .fetch_optional(&mut *connection)
            .await?
            .map(|row| map_lookup(&row))
            .transpose()
        })
    })
}

pub(in crate::repositories) fn history(
    session: &RepositorySession,
    device_serial_no: Option<&str>,
    scan_type: Option<&str>,
    start_date: Option<&str>,
    end_date: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<ScanHistoryProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let device_serial_no = non_empty(device_serial_no);
    let scan_type = non_empty(scan_type);
    let start_date = non_empty(start_date);
    let end_date = non_empty(end_date);
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;

            let mut count =
                QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM scan_events WHERE ");
            push_filters(
                &mut count,
                &tenant_id,
                device_serial_no.as_deref(),
                scan_type.as_deref(),
                start_date.as_deref(),
                end_date.as_deref(),
            );
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&mut *connection)
                .await?;

            let mut data = QueryBuilder::<Postgres>::new(
                "SELECT id,device_serial_no,barcode_text,scan_type,scanned_by,
                        warehouse_id,notes,created_at
                 FROM scan_events WHERE ",
            );
            push_filters(
                &mut data,
                &tenant_id,
                device_serial_no.as_deref(),
                scan_type.as_deref(),
                start_date.as_deref(),
                end_date.as_deref(),
            );
            data.push(" ORDER BY created_at DESC,id DESC LIMIT ")
                .push_bind(page_size)
                .push(" OFFSET ")
                .push_bind(offset);

            let rows = data.build().fetch_all(&mut *connection).await?;
            let events = rows
                .iter()
                .map(map_scan_event)
                .collect::<Result<Vec<_>, _>>()?;

            Ok(ScanHistoryProjection {
                events,
                total,
                page,
                page_size,
            })
        })
    })
}

pub(in crate::repositories) fn stats(
    session: &RepositorySession,
    today: &str,
    week_start: &str,
) -> Result<ScanStatsProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let today = today.to_owned();
    let week_start = week_start.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            let today_scans: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)::bigint FROM scan_events
                 WHERE tenant_id=$1 AND substring(created_at from 1 for 10)=$2",
            )
            .bind(&tenant_id)
            .bind(&today)
            .fetch_one(&mut *connection)
            .await?;

            let this_week_scans: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)::bigint FROM scan_events
                 WHERE tenant_id=$1
                   AND replace(substring(created_at from 1 for 19),'T',' ') >= $2",
            )
            .bind(&tenant_id)
            .bind(&week_start)
            .fetch_one(&mut *connection)
            .await?;

            let rows = sqlx::query(
                "SELECT scan_type,COUNT(*)::bigint AS count
                 FROM scan_events
                 WHERE tenant_id=$1
                   AND replace(substring(created_at from 1 for 19),'T',' ') >= $2
                 GROUP BY scan_type",
            )
            .bind(&tenant_id)
            .bind(&week_start)
            .fetch_all(&mut *connection)
            .await?;

            let counts = rows
                .iter()
                .map(|row| {
                    Ok((
                        row.try_get::<String, _>("scan_type")?,
                        row.try_get::<i64, _>("count")?,
                    ))
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()?;

            Ok(stats_projection(today_scans, this_week_scans, counts))
        })
    })
}

fn push_filters<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant_id: &'args str,
    device_serial_no: Option<&'args str>,
    scan_type: Option<&'args str>,
    start_date: Option<&'args str>,
    end_date: Option<&'args str>,
) {
    query.push("tenant_id=").push_bind(tenant_id);
    if let Some(value) = device_serial_no {
        query.push(" AND device_serial_no=").push_bind(value);
    }
    if let Some(value) = scan_type {
        query.push(" AND scan_type=").push_bind(value);
    }
    if let Some(value) = start_date {
        query
            .push(" AND substring(created_at from 1 for 10)>=")
            .push_bind(value);
    }
    if let Some(value) = end_date {
        query
            .push(" AND substring(created_at from 1 for 10)<=")
            .push_bind(value);
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value.filter(|value| !value.is_empty()).map(str::to_owned)
}

fn map_lookup(row: &sqlx::postgres::PgRow) -> Result<BarcodeLookupProjection, sqlx::Error> {
    Ok(BarcodeLookupProjection {
        barcode_id: row.try_get("id")?,
        device_serial_no: row.try_get("device_serial_no")?,
        barcode_text: row.try_get("barcode_text")?,
        barcode_type: row.try_get("barcode_type")?,
        label_format: row.try_get("label_format")?,
        generated_at: row.try_get("generated_at")?,
        device_info: BarcodeDeviceInfoProjection {
            serial_no: row.try_get("serialno")?,
            model_id: row.try_get("modelid")?,
            rental_status: row.try_get("rentalstatus")?,
            status: row.try_get("status")?,
            current_warehouse_id: row.try_get("currentwarehouseid")?,
        },
    })
}

fn map_scan_event(row: &sqlx::postgres::PgRow) -> Result<ScanEventProjection, sqlx::Error> {
    Ok(ScanEventProjection {
        id: row.try_get("id")?,
        device_serial_no: row.try_get("device_serial_no")?,
        barcode_text: row.try_get("barcode_text")?,
        scan_type: row.try_get("scan_type")?,
        scanned_by: row.try_get("scanned_by")?,
        warehouse_id: row.try_get("warehouse_id")?,
        notes: row.try_get("notes")?,
        created_at: row.try_get("created_at")?,
    })
}
