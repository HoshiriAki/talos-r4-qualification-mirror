#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::notification::{
    NotificationListProjection, NotificationMessageProjection, NotificationOrderContext,
    NotificationTemplateProjection,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn template(
    session: &RepositorySession,
    event_type: &str,
    channel: &str,
) -> Result<Option<NotificationTemplateProjection>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let event_type = event_type.to_owned();
    let channel = channel.to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            sqlx::query(
                "SELECT id,event_type,channel,subject_template,body_template,
                        (is_enabled <> 0) AS is_enabled,created_at,updated_at
                 FROM notification_templates
                 WHERE tenant_id=$1 AND event_type=$2 AND channel=$3 AND is_enabled=1
                 LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(&event_type)
            .bind(&channel)
            .fetch_optional(&mut *connection)
            .await?
            .map(|row| map_template(&row))
            .transpose()
        })
    })
}

pub(in crate::repositories) fn enabled_channels(
    session: &RepositorySession,
    event_type: &str,
) -> Result<Vec<String>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let event_type = event_type.to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            let channels = sqlx::query_scalar::<_, String>(
                "SELECT channel FROM notification_templates
                 WHERE tenant_id=$1 AND event_type=$2 AND is_enabled=1
                 ORDER BY channel",
            )
            .bind(&tenant_id)
            .bind(&event_type)
            .fetch_all(&mut *connection)
            .await?;
            Ok(channels)
        })
    })
}

pub(in crate::repositories) fn list_in_app(
    session: &RepositorySession,
    user_id: &str,
    page: i64,
    page_size: i64,
) -> Result<NotificationListProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let user_id = user_id.to_owned();
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);
    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;
            let unread_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)::bigint FROM notification_log
                 WHERE tenant_id=$1 AND channel='in_app' AND recipient=$2 AND read_at IS NULL",
            )
            .bind(&tenant_id)
            .bind(&user_id)
            .fetch_one(&mut *connection)
            .await?;
            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)::bigint FROM notification_log
                 WHERE tenant_id=$1 AND channel='in_app' AND recipient=$2",
            )
            .bind(&tenant_id)
            .bind(&user_id)
            .fetch_one(&mut *connection)
            .await?;
            let rows = sqlx::query(
                "SELECT id,event_type,channel,subject,body,status,read_at,created_at
                 FROM notification_log
                 WHERE tenant_id=$1 AND channel='in_app' AND recipient=$2
                 ORDER BY created_at DESC LIMIT $3 OFFSET $4",
            )
            .bind(&tenant_id)
            .bind(&user_id)
            .bind(page_size)
            .bind(offset)
            .fetch_all(&mut *connection)
            .await?;
            let messages = rows
                .iter()
                .map(map_message)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(NotificationListProjection {
                messages,
                unread_count,
                page,
                page_size,
                total,
            })
        })
    })
}

pub(in crate::repositories) fn list_templates(
    session: &RepositorySession,
) -> Result<Vec<NotificationTemplateProjection>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT id,event_type,channel,subject_template,body_template,
                        (is_enabled <> 0) AS is_enabled,created_at,updated_at
                 FROM notification_templates
                 WHERE tenant_id=$1
                 ORDER BY event_type,channel",
            )
            .bind(&tenant_id)
            .fetch_all(&mut *connection)
            .await?;
            rows.iter().map(map_template).collect()
        })
    })
}

pub(in crate::repositories) fn order_context(
    session: &RepositorySession,
    order_id: &str,
) -> Result<Option<NotificationOrderContext>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id.to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            sqlx::query(
                "SELECT o.orderno,o.deviceserialno,COALESCE(c.display_name,'') AS customer_name,
                        o.enddate AS due_date
                 FROM orders o
                 LEFT JOIN customers c
                   ON c.tenant_id=o.tenant_id AND c.id=o.customer_id
                 WHERE o.tenant_id=$1 AND o.id=$2
                 LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(&order_id)
            .fetch_optional(&mut *connection)
            .await?
            .map(|row| {
                Ok(NotificationOrderContext {
                    order_no: row.try_get("orderno")?,
                    device_serial_no: row.try_get("deviceserialno")?,
                    customer_name: row.try_get("customer_name")?,
                    due_date: row.try_get("due_date")?,
                })
            })
            .transpose()
        })
    })
}

fn map_template(
    row: &sqlx::postgres::PgRow,
) -> Result<NotificationTemplateProjection, sqlx::Error> {
    Ok(NotificationTemplateProjection {
        id: row.try_get("id")?,
        event_type: row.try_get("event_type")?,
        channel: row.try_get("channel")?,
        subject_template: row.try_get("subject_template")?,
        body_template: row.try_get("body_template")?,
        is_enabled: row.try_get("is_enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_message(row: &sqlx::postgres::PgRow) -> Result<NotificationMessageProjection, sqlx::Error> {
    Ok(NotificationMessageProjection {
        id: row.try_get("id")?,
        event_type: row.try_get("event_type")?,
        channel: row.try_get("channel")?,
        subject: row.try_get("subject")?,
        body: row.try_get("body")?,
        status: row.try_get("status")?,
        read_at: row.try_get("read_at")?,
        created_at: row.try_get("created_at")?,
    })
}
