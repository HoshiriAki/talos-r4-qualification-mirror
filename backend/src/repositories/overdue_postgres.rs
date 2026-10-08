#![cfg(feature = "postgres")]

use serde_json::Value;

use crate::repositories::RepositoryError;
use crate::repositories::overdue::{OverdueFeeConfig, OverdueFeeConfigPatch, OverdueMutationError};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresOverdueRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresOverdueRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn detect(
        &self,
        today: &str,
        now: &str,
    ) -> Result<Value, RepositoryError> {
        crate::repositories::overdue_postgres_mutation::detect(self.session, today, now)
    }

    pub(in crate::repositories) fn calc(
        &self,
        order_id: &str,
        today: &str,
    ) -> Result<Value, RepositoryError> {
        crate::repositories::overdue_postgres_read::calc(self.session, order_id, today)
    }

    pub(in crate::repositories) fn apply_delegated(
        &self,
        overdue_id: i64,
    ) -> Result<Value, OverdueMutationError> {
        crate::repositories::overdue_postgres_read::apply_delegated(self.session, overdue_id)
    }

    pub(in crate::repositories) fn waive(
        &self,
        overdue_id: i64,
        reason: &str,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<Value, OverdueMutationError> {
        crate::repositories::overdue_postgres_mutation::waive(
            self.session,
            overdue_id,
            reason,
            actor_identity_id,
            now,
        )
    }

    pub(in crate::repositories) fn list(
        &self,
        status: Option<&str>,
        customer_phone: Option<&str>,
        order_id: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Value, RepositoryError> {
        crate::repositories::overdue_postgres_read::list(
            self.session,
            status,
            customer_phone,
            order_id,
            page,
            page_size,
        )
    }

    pub(in crate::repositories) fn get(
        &self,
        overdue_id: i64,
    ) -> Result<Option<Value>, RepositoryError> {
        crate::repositories::overdue_postgres_read::get(self.session, overdue_id)
    }

    pub(in crate::repositories) fn config_get(&self) -> Result<OverdueFeeConfig, RepositoryError> {
        crate::repositories::overdue_postgres_read::config_get(self.session)
    }

    pub(in crate::repositories) fn config_upsert(
        &self,
        patch: OverdueFeeConfigPatch,
        now: &str,
    ) -> Result<OverdueFeeConfig, RepositoryError> {
        crate::repositories::overdue_postgres_mutation::config_upsert(self.session, patch, now)
    }

    pub(in crate::repositories) fn escalate(&self, now: &str) -> Result<Value, RepositoryError> {
        crate::repositories::overdue_postgres_mutation::escalate(self.session, now)
    }

    pub(in crate::repositories) fn escalation_history(
        &self,
        overdue_id: i64,
    ) -> Result<Vec<Value>, RepositoryError> {
        crate::repositories::overdue_postgres_read::escalation_history(self.session, overdue_id)
    }

    pub(in crate::repositories) fn stats(&self) -> Result<Value, RepositoryError> {
        crate::repositories::overdue_postgres_read::stats(self.session)
    }

    pub(in crate::repositories) fn check_before_order(
        &self,
        customer_phone: &str,
    ) -> Result<Value, RepositoryError> {
        crate::repositories::overdue_postgres_read::check_before_order(self.session, customer_phone)
    }
}
