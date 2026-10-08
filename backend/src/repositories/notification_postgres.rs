#![cfg(feature = "postgres")]

use crate::repositories::RepositoryError;
use crate::repositories::notification::{
    NotificationListProjection, NotificationLogOutcome, NotificationMutationError,
    NotificationOrderContext, NotificationTemplateProjection, NotificationTemplateUpsertOutcome,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresNotificationRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresNotificationRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn template(
        &self,
        event_type: &str,
        channel: &str,
    ) -> Result<Option<NotificationTemplateProjection>, RepositoryError> {
        crate::repositories::notification_postgres_read::template(self.session, event_type, channel)
    }

    pub(in crate::repositories) fn enabled_channels(
        &self,
        event_type: &str,
    ) -> Result<Vec<String>, RepositoryError> {
        crate::repositories::notification_postgres_read::enabled_channels(self.session, event_type)
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
        crate::repositories::notification_postgres_mutation::insert_log(
            self.session,
            template_id,
            event_type,
            channel,
            recipient,
            subject,
            body,
            now,
        )
    }

    pub(in crate::repositories) fn list_in_app(
        &self,
        user_id: &str,
        page: i64,
        page_size: i64,
    ) -> Result<NotificationListProjection, RepositoryError> {
        crate::repositories::notification_postgres_read::list_in_app(
            self.session,
            user_id,
            page,
            page_size,
        )
    }

    pub(in crate::repositories) fn mark_read(
        &self,
        message_id: Option<&str>,
        user_id: &str,
        now: &str,
    ) -> Result<u64, RepositoryError> {
        crate::repositories::notification_postgres_mutation::mark_read(
            self.session,
            message_id,
            user_id,
            now,
        )
    }

    pub(in crate::repositories) fn list_templates(
        &self,
    ) -> Result<Vec<NotificationTemplateProjection>, RepositoryError> {
        crate::repositories::notification_postgres_read::list_templates(self.session)
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
        crate::repositories::notification_postgres_mutation::upsert_template(
            self.session,
            id,
            event_type,
            channel,
            subject_template,
            body_template,
            is_enabled,
            now,
        )
    }

    pub(in crate::repositories) fn order_context(
        &self,
        order_id: &str,
    ) -> Result<Option<NotificationOrderContext>, RepositoryError> {
        crate::repositories::notification_postgres_read::order_context(self.session, order_id)
    }
}
