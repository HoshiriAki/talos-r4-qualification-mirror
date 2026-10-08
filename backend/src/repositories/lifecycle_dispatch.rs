use crate::repositories::lifecycle::{
    LifecycleHistoryProjection, LifecycleMigrationExceptionProjection, LifecycleOperationalView,
    OrderLifecycleProjection, ScopedOrderLifecycleRepository as SqliteOrderLifecycleRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::lifecycle_postgres::PostgresOrderLifecycleRepository;
#[cfg(feature = "postgres")]
use crate::repositories::lifecycle_postgres_write::PostgresLifecycleWriteRepository;

pub struct ScopedOrderLifecycleRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedOrderLifecycleRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn get(&self, order_id: &str) -> Result<Option<OrderLifecycleProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOrderLifecycleRepository::new(self.scoped.session()).get(order_id);
        }

        SqliteOrderLifecycleRepository::new(self.scoped).get(order_id)
    }

    pub fn operational_view(
        &self,
        order_id: &str,
    ) -> Result<LifecycleOperationalView, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOrderLifecycleRepository::new(self.scoped.session())
                .operational_view(order_id);
        }

        SqliteOrderLifecycleRepository::new(self.scoped).operational_view(order_id)
    }

    pub fn history(
        &self,
        order_id: &str,
    ) -> Result<Vec<LifecycleHistoryProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOrderLifecycleRepository::new(self.scoped.session()).history(order_id);
        }

        SqliteOrderLifecycleRepository::new(self.scoped).history(order_id)
    }

    pub fn list_migration_exceptions(
        &self,
    ) -> Result<Vec<LifecycleMigrationExceptionProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOrderLifecycleRepository::new(self.scoped.session())
                .list_migration_exceptions();
        }

        SqliteOrderLifecycleRepository::new(self.scoped).list_migration_exceptions()
    }

    pub fn apply_action(
        &self,
        order_id: &str,
        action: &str,
        expected_version: i64,
        actor_user_id: &str,
        reason: &str,
    ) -> Result<LifecycleOperationalView, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            PostgresLifecycleWriteRepository::new(self.scoped.session()).apply_action(
                order_id,
                action,
                expected_version,
                actor_user_id,
                reason,
            )?;
            return PostgresOrderLifecycleRepository::new(self.scoped.session())
                .operational_view(order_id);
        }

        SqliteOrderLifecycleRepository::new(self.scoped).apply_action(
            order_id,
            action,
            expected_version,
            actor_user_id,
            reason,
        )
    }
}
