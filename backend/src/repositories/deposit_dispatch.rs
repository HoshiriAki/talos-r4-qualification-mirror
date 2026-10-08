use crate::repositories::deposit::{
    DepositCalculateOutcome, DepositCollectOutcome, DepositDetailProjection, DepositForfeitOutcome,
    DepositMutationError, DepositReleaseOutcome, SqliteDepositRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::deposit_postgres::PostgresDepositRepository;

pub struct ScopedDepositRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedDepositRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn calculate(
        &self,
        order_id: &str,
        default_per_device: f64,
        now: &str,
    ) -> Result<DepositCalculateOutcome, DepositMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDepositRepository::new(self.scoped.session()).calculate(
                order_id,
                default_per_device,
                now,
            );
        }
        SqliteDepositRepository::new(self.scoped).calculate(order_id, default_per_device, now)
    }

    pub fn collect(
        &self,
        order_id: &str,
        amount: f64,
        operator: &str,
        now: &str,
    ) -> Result<DepositCollectOutcome, DepositMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDepositRepository::new(self.scoped.session())
                .collect(order_id, amount, operator, now);
        }
        SqliteDepositRepository::new(self.scoped).collect(order_id, amount, operator, now)
    }

    pub fn release(
        &self,
        order_id: &str,
        reason: &str,
        operator: &str,
        now: &str,
    ) -> Result<DepositReleaseOutcome, DepositMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDepositRepository::new(self.scoped.session())
                .release(order_id, reason, operator, now);
        }
        SqliteDepositRepository::new(self.scoped).release(order_id, reason, operator, now)
    }

    pub fn forfeit(
        &self,
        order_id: &str,
        amount: f64,
        reason: &str,
        operator: &str,
        now: &str,
    ) -> Result<DepositForfeitOutcome, DepositMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDepositRepository::new(self.scoped.session())
                .forfeit(order_id, amount, reason, operator, now);
        }
        SqliteDepositRepository::new(self.scoped).forfeit(order_id, amount, reason, operator, now)
    }

    pub fn get(&self, order_id: &str) -> Result<DepositDetailProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDepositRepository::new(self.scoped.session()).get(order_id);
        }
        SqliteDepositRepository::new(self.scoped).get(order_id)
    }
}
