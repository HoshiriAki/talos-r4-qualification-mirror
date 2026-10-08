#![cfg(feature = "postgres")]

use std::collections::BTreeMap;

use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::optical_sop::{
    OpticalInspectionProjection, OpticalListProjection, OpticalStatsProjection,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn get(
    session: &RepositorySession,
    id: i64,
) -> Result<Option<OpticalInspectionProjection>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            sqlx::query(
                "SELECT id,order_id,device_serial_no,inspector_id,
                        (body_ok<>0) AS body_ok,
                        (lens_ok<>0) AS lens_ok,
                        (screen_ok<>0) AS screen_ok,
                        (accessory_ok<>0) AS accessory_ok,
                        (function_ok<>0) AS function_ok,
                        overall_grade,damage_report_id,
                        body_note,lens_note,screen_note,accessory_note,function_note,
                        photo_urls,notes,completed_at,created_at,updated_at
                 FROM inspection_checklists
                 WHERE tenant_id=$1 AND id=$2
                 LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(id)
            .fetch_optional(&mut *connection)
            .await?
            .map(|row| map_projection(&row))
            .transpose()
        })
    })
}

pub(in crate::repositories) fn list(
    session: &RepositorySession,
    order_id: Option<&str>,
    device_serial_no: Option<&str>,
    overall_grade: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<OpticalListProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = non_empty(order_id);
    let device_serial_no = non_empty(device_serial_no);
    let overall_grade = non_empty(overall_grade);
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;

            let mut count = QueryBuilder::<Postgres>::new(
                "SELECT COUNT(*)::bigint FROM inspection_checklists WHERE ",
            );
            push_filters(
                &mut count,
                &tenant_id,
                order_id.as_deref(),
                device_serial_no.as_deref(),
                overall_grade.as_deref(),
            );
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&mut *connection)
                .await?;

            let mut data = QueryBuilder::<Postgres>::new(
                "SELECT id,order_id,device_serial_no,inspector_id,
                        (body_ok<>0) AS body_ok,
                        (lens_ok<>0) AS lens_ok,
                        (screen_ok<>0) AS screen_ok,
                        (accessory_ok<>0) AS accessory_ok,
                        (function_ok<>0) AS function_ok,
                        overall_grade,damage_report_id,
                        body_note,lens_note,screen_note,accessory_note,function_note,
                        photo_urls,notes,completed_at,created_at,updated_at
                 FROM inspection_checklists WHERE ",
            );
            push_filters(
                &mut data,
                &tenant_id,
                order_id.as_deref(),
                device_serial_no.as_deref(),
                overall_grade.as_deref(),
            );
            data.push(" ORDER BY created_at DESC,id DESC LIMIT ")
                .push_bind(page_size)
                .push(" OFFSET ")
                .push_bind(offset);

            let rows = data.build().fetch_all(&mut *connection).await?;
            let items = rows
                .iter()
                .map(map_projection)
                .collect::<Result<Vec<_>, _>>()?;

            Ok(OpticalListProjection {
                items,
                total,
                page,
                page_size,
            })
        })
    })
}

pub(in crate::repositories) fn stats(
    session: &RepositorySession,
) -> Result<OpticalStatsProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)::bigint
                 FROM inspection_checklists
                 WHERE tenant_id=$1",
            )
            .bind(&tenant_id)
            .fetch_one(&mut *connection)
            .await?;

            let pass_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)::bigint
                 FROM inspection_checklists
                 WHERE tenant_id=$1 AND overall_grade='pass'",
            )
            .bind(&tenant_id)
            .fetch_one(&mut *connection)
            .await?;
            let damage_count = total - pass_count;

            let rows = sqlx::query(
                "SELECT overall_grade,COUNT(*)::bigint AS count
                 FROM inspection_checklists
                 WHERE tenant_id=$1
                 GROUP BY overall_grade
                 ORDER BY overall_grade",
            )
            .bind(&tenant_id)
            .fetch_all(&mut *connection)
            .await?;
            let by_grade = rows
                .iter()
                .map(|row| {
                    Ok((
                        row.try_get::<String, _>("overall_grade")?,
                        row.try_get::<i64, _>("count")?,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, sqlx::Error>>()?;

            Ok(OpticalStatsProjection {
                total,
                pass_count,
                damage_count,
                by_grade,
            })
        })
    })
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value.filter(|value| !value.is_empty()).map(str::to_owned)
}

fn push_filters<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant_id: &'args str,
    order_id: Option<&'args str>,
    device_serial_no: Option<&'args str>,
    overall_grade: Option<&'args str>,
) {
    query.push("tenant_id=").push_bind(tenant_id);
    if let Some(value) = order_id {
        query.push(" AND order_id=").push_bind(value);
    }
    if let Some(value) = device_serial_no {
        query.push(" AND device_serial_no=").push_bind(value);
    }
    if let Some(value) = overall_grade {
        query.push(" AND overall_grade=").push_bind(value);
    }
}

fn map_projection(row: &sqlx::postgres::PgRow) -> Result<OpticalInspectionProjection, sqlx::Error> {
    Ok(OpticalInspectionProjection {
        id: row.try_get("id")?,
        order_id: row.try_get("order_id")?,
        device_serial_no: row.try_get("device_serial_no")?,
        inspector_id: row.try_get("inspector_id")?,
        body_ok: row.try_get("body_ok")?,
        lens_ok: row.try_get("lens_ok")?,
        screen_ok: row.try_get("screen_ok")?,
        accessory_ok: row.try_get("accessory_ok")?,
        function_ok: row.try_get("function_ok")?,
        overall_grade: row.try_get("overall_grade")?,
        damage_report_id: row.try_get("damage_report_id")?,
        body_note: row.try_get("body_note")?,
        lens_note: row.try_get("lens_note")?,
        screen_note: row.try_get("screen_note")?,
        accessory_note: row.try_get("accessory_note")?,
        function_note: row.try_get("function_note")?,
        photo_urls: row.try_get("photo_urls")?,
        notes: row.try_get("notes")?,
        completed_at: row.try_get("completed_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
