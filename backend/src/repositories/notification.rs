use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationTemplateProjection {
    pub id: String,
    pub event_type: String,
    pub channel: String,
    pub subject_template: String,
    pub body_template: String,
    pub is_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationMessageProjection {
    pub id: String,
    pub event_type: String,
    pub channel: String,
    pub subject: String,
    pub body: String,
    pub status: String,
    pub read_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NotificationListProjection {
    pub messages: Vec<NotificationMessageProjection>,
    pub unread_count: i64,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NotificationLogOutcome {
    pub message_id: String,
    pub event_type: String,
    pub channel: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NotificationTemplateUpsertOutcome {
    pub id: String,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NotificationOrderContext {
    pub order_no: String,
    pub device_serial_no: String,
    pub customer_name: String,
    pub due_date: String,
}

#[derive(Debug, thiserror::Error)]
pub enum NotificationMutationError {
    #[error("notification template not found")]
    TemplateNotFound,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteNotificationRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteNotificationRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn template(
        &self,
        event_type: &str,
        channel: &str,
    ) -> Result<Option<NotificationTemplateProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let event_type = event_type.to_owned();
        let channel = channel.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,event_type,channel,subject_template,body_template,is_enabled,created_at,updated_at
                     FROM notification_templates
                     WHERE tenant_id=?1 AND event_type=?2 AND channel=?3 AND is_enabled=1
                     LIMIT 1",
                    params![tenant_id, event_type, channel],
                    map_template,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn enabled_channels(
        &self,
        event_type: &str,
    ) -> Result<Vec<String>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let event_type = event_type.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT channel FROM notification_templates
                 WHERE tenant_id=?1 AND event_type=?2 AND is_enabled=1
                 ORDER BY channel",
            )?;
            statement
                .query_map(params![tenant_id, event_type], |row| {
                    row.get::<_, String>(0)
                })?
                .collect()
        })
    }

    pub(in crate::repositories) fn insert_log(
        &self,
        template_id: &str,
        event_type: &str,
        channel: &str,
        recipient: &str,
        subject: &str,
        body: &str,
        now: &str,
    ) -> Result<NotificationLogOutcome, RepositoryError> {
        let tenant_id = self.tenant_id();
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

        self.session.write_immediate(move |transaction| {
            transaction.execute(
                "INSERT INTO notification_log
                 (id,template_id,event_type,channel,recipient,subject,body,status,read_at,created_at,tenant_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,'sent',NULL,?8,?9)",
                params![
                    message_id,
                    template_id,
                    event_type,
                    channel,
                    recipient,
                    subject,
                    body,
                    now,
                    tenant_id,
                ],
            )
            .map_err(sqlite_error)?;
            Ok(())
        })?;

        Ok(NotificationLogOutcome {
            message_id: result_id,
            event_type: result_event_type,
            channel: result_channel,
        })
    }

    pub(in crate::repositories) fn list_in_app(
        &self,
        user_id: &str,
        page: i64,
        page_size: i64,
    ) -> Result<NotificationListProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let user_id = user_id.to_owned();
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let unread_count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM notification_log
                 WHERE tenant_id=?1 AND channel='in_app' AND recipient=?2 AND read_at IS NULL",
                params![tenant_id, user_id],
                |row| row.get(0),
            )?;
            let total: i64 = connection.query_row(
                "SELECT COUNT(*) FROM notification_log
                 WHERE tenant_id=?1 AND channel='in_app' AND recipient=?2",
                params![tenant_id, user_id],
                |row| row.get(0),
            )?;
            let mut statement = connection.prepare(
                "SELECT id,event_type,channel,subject,body,status,read_at,created_at
                 FROM notification_log
                 WHERE tenant_id=?1 AND channel='in_app' AND recipient=?2
                 ORDER BY created_at DESC LIMIT ?3 OFFSET ?4",
            )?;
            let messages = statement
                .query_map(params![tenant_id, user_id, page_size, offset], map_message)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(NotificationListProjection {
                messages,
                unread_count,
                page,
                page_size,
                total,
            })
        })
    }

    pub(in crate::repositories) fn mark_read(
        &self,
        message_id: Option<&str>,
        user_id: &str,
        now: &str,
    ) -> Result<u64, RepositoryError> {
        let tenant_id = self.tenant_id();
        let message_id = message_id.map(str::to_owned);
        let user_id = user_id.to_owned();
        let now = now.to_owned();
        self.session.write_immediate(move |transaction| {
            let affected = if let Some(message_id) = message_id {
                transaction
                    .execute(
                        "UPDATE notification_log SET read_at=?1
                     WHERE tenant_id=?2 AND id=?3 AND recipient=?4 AND read_at IS NULL",
                        params![now, tenant_id, message_id, user_id],
                    )
                    .map_err(sqlite_error)?
            } else {
                transaction
                    .execute(
                        "UPDATE notification_log SET read_at=?1
                     WHERE tenant_id=?2 AND channel='in_app' AND recipient=?3 AND read_at IS NULL",
                        params![now, tenant_id, user_id],
                    )
                    .map_err(sqlite_error)?
            };
            Ok(affected as u64)
        })
    }

    pub(in crate::repositories) fn list_templates(
        &self,
    ) -> Result<Vec<NotificationTemplateProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT id,event_type,channel,subject_template,body_template,is_enabled,created_at,updated_at
                 FROM notification_templates
                 WHERE tenant_id=?1
                 ORDER BY event_type,channel",
            )?;
            statement
                .query_map(params![tenant_id], map_template)?
                .collect()
        })
    }

    pub(in crate::repositories) fn upsert_template(
        &self,
        id: Option<&str>,
        event_type: &str,
        channel: &str,
        subject_template: &str,
        body_template: &str,
        is_enabled: bool,
        now: &str,
    ) -> Result<NotificationTemplateUpsertOutcome, NotificationMutationError> {
        let tenant_id = self.tenant_id();
        let id = id.map(str::to_owned);
        let event_type = event_type.to_owned();
        let channel = channel.to_owned();
        let subject_template = subject_template.to_owned();
        let body_template = body_template.to_owned();
        let enabled = if is_enabled { 1_i64 } else { 0_i64 };
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                if let Some(id) = id {
                    let affected = transaction.execute(
                        "UPDATE notification_templates
                         SET event_type=?1,channel=?2,subject_template=?3,body_template=?4,
                             is_enabled=?5,updated_at=?6
                         WHERE tenant_id=?7 AND id=?8",
                        params![
                            event_type,
                            channel,
                            subject_template,
                            body_template,
                            enabled,
                            now,
                            tenant_id,
                            id,
                        ],
                    )
                    .map_err(sqlite_error)?;
                    if affected == 0 {
                        return Err(RepositoryError::ContractViolation(
                            "notification-template-not-found".into(),
                        ));
                    }
                    Ok(NotificationTemplateUpsertOutcome { id, created: false })
                } else {
                    let id = Uuid::new_v4().to_string();
                    transaction.execute(
                        "INSERT INTO notification_templates
                         (id,event_type,channel,subject_template,body_template,is_enabled,created_at,updated_at,tenant_id)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?7,?8)",
                        params![
                            id,
                            event_type,
                            channel,
                            subject_template,
                            body_template,
                            enabled,
                            now,
                            tenant_id,
                        ],
                    )
                    .map_err(sqlite_error)?;
                    Ok(NotificationTemplateUpsertOutcome { id, created: true })
                }
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn order_context(
        &self,
        order_id: &str,
    ) -> Result<Option<NotificationOrderContext>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT o.orderNo,o.deviceSerialNo,COALESCE(c.display_name,''),o.endDate
                     FROM orders o
                     LEFT JOIN customers c
                       ON c.tenant_id=o.tenant_id AND c.id=o.customer_id
                     WHERE o.tenant_id=?1 AND o.id=?2
                     LIMIT 1",
                    params![tenant_id, order_id],
                    |row| {
                        Ok(NotificationOrderContext {
                            order_no: row.get(0)?,
                            device_serial_no: row.get(1)?,
                            customer_name: row.get(2)?,
                            due_date: row.get(3)?,
                        })
                    },
                )
                .optional()
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

pub(in crate::repositories) fn map_mutation_error(
    error: RepositoryError,
) -> NotificationMutationError {
    if matches!(
        &error,
        RepositoryError::ContractViolation(message)
            if message == "notification-template-not-found"
    ) {
        return NotificationMutationError::TemplateNotFound;
    }
    NotificationMutationError::Storage(error)
}

fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn map_template(row: &rusqlite::Row<'_>) -> rusqlite::Result<NotificationTemplateProjection> {
    Ok(NotificationTemplateProjection {
        id: row.get(0)?,
        event_type: row.get(1)?,
        channel: row.get(2)?,
        subject_template: row.get(3)?,
        body_template: row.get(4)?,
        is_enabled: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn map_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<NotificationMessageProjection> {
    Ok(NotificationMessageProjection {
        id: row.get(0)?,
        event_type: row.get(1)?,
        channel: row.get(2)?,
        subject: row.get(3)?,
        body: row.get(4)?,
        status: row.get(5)?,
        read_at: row.get(6)?,
        created_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantScope,
    };

    use crate::repositories::{
        NotificationMutationError, RepositoryProvider, SqliteRepositoryProvider,
    };

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("notify-test-user", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("notify-test-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_notification_authority_preserves_scope_templates_messages_and_order_hydration() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE notification_templates (
                    id TEXT PRIMARY KEY,
                    event_type TEXT NOT NULL,
                    channel TEXT NOT NULL,
                    subject_template TEXT NOT NULL,
                    body_template TEXT NOT NULL,
                    is_enabled INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE notification_log (
                    id TEXT PRIMARY KEY,
                    template_id TEXT,
                    event_type TEXT NOT NULL,
                    channel TEXT NOT NULL,
                    recipient TEXT NOT NULL,
                    subject TEXT NOT NULL,
                    body TEXT NOT NULL,
                    status TEXT NOT NULL,
                    read_at TEXT,
                    error_message TEXT DEFAULT '',
                    created_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE customers (
                    id TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL,
                    display_name TEXT NOT NULL
                );
                CREATE TABLE orders (
                    id TEXT PRIMARY KEY,
                    orderNo TEXT NOT NULL,
                    endDate TEXT NOT NULL,
                    deviceSerialNo TEXT NOT NULL,
                    customer_id TEXT,
                    tenant_id TEXT NOT NULL
                );
                INSERT INTO notification_templates VALUES
                    ('tpl-a','shipped','in_app','Tenant A','Order {orderNo}',1,'','', 'tenant-a'),
                    ('tpl-b','shipped','in_app','Tenant B','Private {orderNo}',1,'','', 'tenant-b');
                INSERT INTO customers VALUES
                    ('customer-a','tenant-a','Customer A'),
                    ('customer-b','tenant-b','Customer B');
                INSERT INTO orders VALUES
                    ('order-a','ORDER-A','2026-10-10','DEVICE-A','customer-a','tenant-a'),
                    ('order-b','ORDER-B','2026-10-11','DEVICE-B','customer-b','tenant-b');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool);
        let scoped_a = provider.bind(&context("tenant-a", "notify-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "notify-b")).unwrap();

        let template = scoped_a
            .notifications()
            .template("shipped", "in_app")
            .unwrap()
            .unwrap();
        assert_eq!(template.id, "tpl-a");
        assert_eq!(
            scoped_a
                .notifications()
                .enabled_channels("shipped")
                .unwrap(),
            vec!["in_app"]
        );

        let logged = scoped_a
            .notifications()
            .insert_log(
                "tpl-a",
                "shipped",
                "in_app",
                "notify-test-user",
                "Tenant A",
                "Order ORDER-A",
                "2026-10-01T18:00:00+08:00",
            )
            .unwrap();

        let list_a = scoped_a
            .notifications()
            .list_in_app("notify-test-user", 1, 20)
            .unwrap();
        assert_eq!(list_a.total, 1);
        assert_eq!(list_a.unread_count, 1);
        assert_eq!(list_a.messages[0].id, logged.message_id);
        assert_eq!(
            scoped_b
                .notifications()
                .list_in_app("notify-test-user", 1, 20)
                .unwrap()
                .total,
            0
        );

        assert_eq!(
            scoped_b
                .notifications()
                .mark_read(
                    Some(&logged.message_id),
                    "notify-test-user",
                    "2026-10-01T18:01:00+08:00",
                )
                .unwrap(),
            0
        );
        assert_eq!(
            scoped_a
                .notifications()
                .mark_read(
                    Some(&logged.message_id),
                    "notify-test-user",
                    "2026-10-01T18:02:00+08:00",
                )
                .unwrap(),
            1
        );

        let order = scoped_a
            .notifications()
            .order_context("order-a")
            .unwrap()
            .unwrap();
        assert_eq!(order.order_no, "ORDER-A");
        assert_eq!(order.customer_name, "Customer A");
        assert_eq!(order.due_date, "2026-10-10");
        assert!(
            scoped_a
                .notifications()
                .order_context("order-b")
                .unwrap()
                .is_none()
        );

        let cross_tenant_update = scoped_b.notifications().upsert_template(
            Some("tpl-a"),
            "shipped",
            "in_app",
            "mutated",
            "mutated",
            true,
            "2026-10-01T18:03:00+08:00",
        );
        assert!(matches!(
            cross_tenant_update,
            Err(NotificationMutationError::TemplateNotFound)
        ));

        let created = scoped_a
            .notifications()
            .upsert_template(
                None,
                "return_reminder",
                "sms",
                "",
                "Return {orderNo}",
                true,
                "2026-10-01T18:04:00+08:00",
            )
            .unwrap();
        assert!(created.created);
        assert_eq!(scoped_a.notifications().list_templates().unwrap().len(), 2);
        assert_eq!(scoped_b.notifications().list_templates().unwrap().len(), 1);
    }
}
