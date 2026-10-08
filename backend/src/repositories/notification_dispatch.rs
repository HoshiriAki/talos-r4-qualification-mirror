use crate::repositories::notification::{
    NotificationListProjection, NotificationLogOutcome, NotificationMutationError,
    NotificationOrderContext, NotificationTemplateProjection, NotificationTemplateUpsertOutcome,
    SqliteNotificationRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::notification_postgres::PostgresNotificationRepository;

pub struct ScopedNotificationRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedNotificationRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn template(
        &self,
        event_type: &str,
        channel: &str,
    ) -> Result<Option<NotificationTemplateProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresNotificationRepository::new(self.scoped.session())
                .template(event_type, channel);
        }
        SqliteNotificationRepository::new(self.scoped).template(event_type, channel)
    }

    pub fn enabled_channels(&self, event_type: &str) -> Result<Vec<String>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresNotificationRepository::new(self.scoped.session())
                .enabled_channels(event_type);
        }
        SqliteNotificationRepository::new(self.scoped).enabled_channels(event_type)
    }

    pub fn insert_log(
        &self,
        template_id: &str,
        event_type: &str,
        channel: &str,
        recipient: &str,
        subject: &str,
        body: &str,
        now: &str,
    ) -> Result<NotificationLogOutcome, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresNotificationRepository::new(self.scoped.session()).insert_log(
                template_id,
                event_type,
                channel,
                recipient,
                subject,
                body,
                now,
            );
        }
        SqliteNotificationRepository::new(self.scoped).insert_log(
            template_id,
            event_type,
            channel,
            recipient,
            subject,
            body,
            now,
        )
    }

    pub fn list_in_app(
        &self,
        user_id: &str,
        page: i64,
        page_size: i64,
    ) -> Result<NotificationListProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresNotificationRepository::new(self.scoped.session())
                .list_in_app(user_id, page, page_size);
        }
        SqliteNotificationRepository::new(self.scoped).list_in_app(user_id, page, page_size)
    }

    pub fn mark_read(
        &self,
        message_id: Option<&str>,
        user_id: &str,
        now: &str,
    ) -> Result<u64, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresNotificationRepository::new(self.scoped.session())
                .mark_read(message_id, user_id, now);
        }
        SqliteNotificationRepository::new(self.scoped).mark_read(message_id, user_id, now)
    }

    pub fn list_templates(&self) -> Result<Vec<NotificationTemplateProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresNotificationRepository::new(self.scoped.session()).list_templates();
        }
        SqliteNotificationRepository::new(self.scoped).list_templates()
    }

    pub fn upsert_template(
        &self,
        id: Option<&str>,
        event_type: &str,
        channel: &str,
        subject_template: &str,
        body_template: &str,
        is_enabled: bool,
        now: &str,
    ) -> Result<NotificationTemplateUpsertOutcome, NotificationMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresNotificationRepository::new(self.scoped.session()).upsert_template(
                id,
                event_type,
                channel,
                subject_template,
                body_template,
                is_enabled,
                now,
            );
        }
        SqliteNotificationRepository::new(self.scoped).upsert_template(
            id,
            event_type,
            channel,
            subject_template,
            body_template,
            is_enabled,
            now,
        )
    }

    pub fn order_context(
        &self,
        order_id: &str,
    ) -> Result<Option<NotificationOrderContext>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresNotificationRepository::new(self.scoped.session())
                .order_context(order_id);
        }
        SqliteNotificationRepository::new(self.scoped).order_context(order_id)
    }
}
