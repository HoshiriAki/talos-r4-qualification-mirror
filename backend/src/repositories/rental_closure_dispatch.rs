use crate::repositories::rental_closure::{
    InspectionProjection, RentalClosureFacts, ReturnProjection,
    ScopedRentalClosureRepository as SqliteRentalClosureRepository, SettlementProjection,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::rental_closure_postgres::PostgresRentalClosureRepository;

pub struct ScopedRentalClosureRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedRentalClosureRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn receive_allocation(
        &self,
        order_id: &str,
        allocation_id: &str,
        receive_identity: &str,
    ) -> Result<ReturnProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRentalClosureRepository::new(self.scoped.session()).receive_allocation(
                order_id,
                allocation_id,
                receive_identity,
            );
        }
        SqliteRentalClosureRepository::new(self.scoped).receive_allocation(
            order_id,
            allocation_id,
            receive_identity,
        )
    }

    pub fn transition_inspection(
        &self,
        inspection_id: &str,
        target: &str,
        expected_version: i64,
    ) -> Result<InspectionProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRentalClosureRepository::new(self.scoped.session())
                .transition_inspection(inspection_id, target, expected_version);
        }
        SqliteRentalClosureRepository::new(self.scoped).transition_inspection(
            inspection_id,
            target,
            expected_version,
        )
    }

    pub fn resolve_damage_review(
        &self,
        inspection_id: &str,
        note: &str,
    ) -> Result<(), RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRentalClosureRepository::new(self.scoped.session())
                .resolve_damage_review(inspection_id, note);
        }
        SqliteRentalClosureRepository::new(self.scoped).resolve_damage_review(inspection_id, note)
    }

    pub fn calculate_settlement(
        &self,
        order_id: &str,
        currency: &str,
        amount_minor: i64,
        financial_authority_available: bool,
    ) -> Result<SettlementProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRentalClosureRepository::new(self.scoped.session())
                .calculate_settlement(
                    order_id,
                    currency,
                    amount_minor,
                    financial_authority_available,
                );
        }
        SqliteRentalClosureRepository::new(self.scoped).calculate_settlement(
            order_id,
            currency,
            amount_minor,
            financial_authority_available,
        )
    }

    #[cfg(test)]
    pub fn mark_fixture_settlement_terminal(&self, order_id: &str) -> Result<(), RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRentalClosureRepository::new(self.scoped.session())
                .mark_fixture_settlement_terminal(order_id);
        }
        SqliteRentalClosureRepository::new(self.scoped).mark_fixture_settlement_terminal(order_id)
    }

    pub fn inspection(&self, id: &str) -> Result<Option<InspectionProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRentalClosureRepository::new(self.scoped.session()).inspection(id);
        }
        SqliteRentalClosureRepository::new(self.scoped).inspection(id)
    }

    pub fn facts(&self, order_id: &str) -> Result<RentalClosureFacts, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRentalClosureRepository::new(self.scoped.session()).facts(order_id);
        }
        SqliteRentalClosureRepository::new(self.scoped).facts(order_id)
    }
}
