use serde_json::Value;

use crate::repositories::overdue::{
    OverdueFeeConfig, OverdueFeeConfigPatch, OverdueMutationError, SqliteOverdueRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::overdue_postgres::PostgresOverdueRepository;

pub struct ScopedOverdueRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedOverdueRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn detect(&self, today: &str, now: &str) -> Result<Value, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOverdueRepository::new(self.scoped.session()).detect(today, now);
        }
        SqliteOverdueRepository::new(self.scoped).detect(today, now)
    }

    pub fn calc(&self, order_id: &str, today: &str) -> Result<Value, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOverdueRepository::new(self.scoped.session()).calc(order_id, today);
        }
        SqliteOverdueRepository::new(self.scoped).calc(order_id, today)
    }

    pub fn apply_delegated(&self, overdue_id: i64) -> Result<Value, OverdueMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOverdueRepository::new(self.scoped.session())
                .apply_delegated(overdue_id);
        }
        SqliteOverdueRepository::new(self.scoped).apply_delegated(overdue_id)
    }

    pub fn waive(
        &self,
        overdue_id: i64,
        reason: &str,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<Value, OverdueMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOverdueRepository::new(self.scoped.session()).waive(
                overdue_id,
                reason,
                actor_identity_id,
                now,
            );
        }
        SqliteOverdueRepository::new(self.scoped).waive(overdue_id, reason, actor_identity_id, now)
    }

    pub fn list(
        &self,
        status: Option<&str>,
        customer_phone: Option<&str>,
        order_id: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Value, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOverdueRepository::new(self.scoped.session()).list(
                status,
                customer_phone,
                order_id,
                page,
                page_size,
            );
        }
        SqliteOverdueRepository::new(self.scoped).list(
            status,
            customer_phone,
            order_id,
            page,
            page_size,
        )
    }

    pub fn get(&self, overdue_id: i64) -> Result<Option<Value>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOverdueRepository::new(self.scoped.session()).get(overdue_id);
        }
        SqliteOverdueRepository::new(self.scoped).get(overdue_id)
    }

    pub fn config_get(&self) -> Result<OverdueFeeConfig, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOverdueRepository::new(self.scoped.session()).config_get();
        }
        SqliteOverdueRepository::new(self.scoped).config_get()
    }

    pub fn config_upsert(
        &self,
        patch: OverdueFeeConfigPatch,
        now: &str,
    ) -> Result<OverdueFeeConfig, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOverdueRepository::new(self.scoped.session()).config_upsert(patch, now);
        }
        SqliteOverdueRepository::new(self.scoped).config_upsert(patch, now)
    }

    pub fn escalate(&self, now: &str) -> Result<Value, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOverdueRepository::new(self.scoped.session()).escalate(now);
        }
        SqliteOverdueRepository::new(self.scoped).escalate(now)
    }

    pub fn escalation_history(&self, overdue_id: i64) -> Result<Vec<Value>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOverdueRepository::new(self.scoped.session())
                .escalation_history(overdue_id);
        }
        SqliteOverdueRepository::new(self.scoped).escalation_history(overdue_id)
    }

    pub fn stats(&self) -> Result<Value, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOverdueRepository::new(self.scoped.session()).stats();
        }
        SqliteOverdueRepository::new(self.scoped).stats()
    }

    pub fn check_before_order(&self, customer_phone: &str) -> Result<Value, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOverdueRepository::new(self.scoped.session())
                .check_before_order(customer_phone);
        }
        SqliteOverdueRepository::new(self.scoped).check_before_order(customer_phone)
    }
}
