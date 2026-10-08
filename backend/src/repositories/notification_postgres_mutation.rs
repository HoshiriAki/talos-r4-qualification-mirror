#![cfg(feature = "postgres")]

use uuid::Uuid;

use crate::repositories::RepositoryError;
use crate::repositories::notification::{
    NotificationLogOutcome, NotificationMutationError, NotificationTemplateUpsertOutcome,
    map_mutation_error,
};
use crate::repositories::notification_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn insert_log(
    session: &RepositorySession,
    template_id: &str,
    event_type: &str,
    channel: &str,
    recipient: &str,
    subject: &str,
    body: &str,
    now: &str,
) -> Result<NotificationLogOutcome, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let template_id = template_id.to_owned();
    let event_type = event_type.to_owned();
    let channel = channel.to_owned();
    let recipient = recipient.to_owned();
    let subject = subject.to_owned();
    let body = body.to_owned();
    let now = now.to_owned();
    let message_id = Uuid::new_v4().to_string();
    let result_id = message_id.clone();
    let result_event_type = event_type.clone();
    let result_channel = channel.clone();

    session.pg_write_serializable_repository(move |connection| {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO notification_log
                 (id,template_id,event_type,channel,recipient,subject,body,status,read_at,created_at,tenant_id)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,'sent',NULL,$8,$9)",
            )
            .bind(&message_id)
            .bind(&template_id)
            .bind(&event_type)
            .bind(&channel)
            .bind(&recipient)
            .bind(&subject)
            .bind(&body)
            .bind(&now)
            .bind(&tenant_id)
            .execute(&mut *connection)
            .await
            .map_err(pg_error)?;
            Ok(())
        })
    })?;

    Ok(NotificationLogOutcome {
        message_id: result_id,
        event_type: result_event_type,
        channel: result_channel,
    })
}

pub(in crate::repositories) fn mark_read(
    session: &RepositorySession,
    message_id: Option<&str>,
    user_id: &str,
    now: &str,
) -> Result<u64, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let message_id = message_id.map(str::to_owned);
    let user_id = user_id.to_owned();
    let now = now.to_owned();
    session.pg_write_serializable_repository(move |connection| {
        Box::pin(async move {
            let affected = if let Some(message_id) = message_id {
                sqlx::query(
                    "UPDATE notification_log SET read_at=$1
                     WHERE tenant_id=$2 AND id=$3 AND recipient=$4 AND read_at IS NULL",
                )
                .bind(&now)
                .bind(&tenant_id)
                .bind(&message_id)
                .bind(&user_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?
                .rows_affected()
            } else {
                sqlx::query(
                    "UPDATE notification_log SET read_at=$1
                     WHERE tenant_id=$2 AND channel='in_app' AND recipient=$3 AND read_at IS NULL",
                )
                .bind(&now)
                .bind(&tenant_id)
                .bind(&user_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?
                .rows_affected()
            };
            Ok(affected)
        })
    })
}

pub(in crate::repositories) fn upsert_template(
    session: &RepositorySession,
    id: Option<&str>,
    event_type: &str,
    channel: &str,
    subject_template: &str,
    body_template: &str,
    is_enabled: bool,
    now: &str,
) -> Result<NotificationTemplateUpsertOutcome, NotificationMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let id = id.map(str::to_owned);
    let event_type = event_type.to_owned();
    let channel = channel.to_owned();
    let subject_template = subject_template.to_owned();
    let body_template = body_template.to_owned();
    let enabled = if is_enabled { 1_i32 } else { 0_i32 };
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                if let Some(id) = id {
                    let affected = sqlx::query(
                        "UPDATE notification_templates
                         SET event_type=$1,channel=$2,subject_template=$3,body_template=$4,
                             is_enabled=$5,updated_at=$6
                         WHERE tenant_id=$7 AND id=$8",
                    )
                    .bind(&event_type)
                    .bind(&channel)
                    .bind(&subject_template)
                    .bind(&body_template)
                    .bind(enabled)
                    .bind(&now)
                    .bind(&tenant_id)
                    .bind(&id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .rows_affected();
                    if affected == 0 {
                        return Err(RepositoryError::ContractViolation(
                            "notification-template-not-found".into(),
                        ));
                    }
                    Ok(NotificationTemplateUpsertOutcome { id, created: false })
                } else {
                    let id = Uuid::new_v4().to_string();
                    sqlx::query(
                        "INSERT INTO notification_templates
                         (id,event_type,channel,subject_template,body_template,is_enabled,created_at,updated_at,tenant_id)
                         VALUES ($1,$2,$3,$4,$5,$6,$7,$7,$8)",
                    )
                    .bind(&id)
                    .bind(&event_type)
                    .bind(&channel)
                    .bind(&subject_template)
                    .bind(&body_template)
                    .bind(enabled)
                    .bind(&now)
                    .bind(&tenant_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    Ok(NotificationTemplateUpsertOutcome { id, created: true })
                }
            })
        })
        .map_err(map_mutation_error)
}
