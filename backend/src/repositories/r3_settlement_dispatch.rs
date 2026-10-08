use crate::repositories::r3_settlement::{
    AdditionalChargeInput, DamageFindingInput, DamageFindingProjection, DepositDeductionInput,
    DisputeProjection, InspectionCompletionInput, InspectionCompletionProjection,
    LiabilityDecisionInput, LiabilityDecisionProjection, OpenDisputeInput, R3ClosureFacts,
    RepairCaseProjection, RepairDecisionInput, RepairTransitionInput, ResolveDisputeInput,
    ScopedR3SettlementRepository as SqliteR3SettlementRepository, SettlementCaseProjection,
    SettlementEffectAdmissionProjection, SettlementProposalInput,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::r3_settlement_postgres::PostgresR3SettlementRepository;

pub struct ScopedR3SettlementRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedR3SettlementRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn configure_business_timezone(
        &self,
        time_zone_id: &str,
        actor: &str,
    ) -> Result<String, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped)
                .configure_business_timezone(time_zone_id, actor);
        }
        SqliteR3SettlementRepository::new(self.scoped)
            .configure_business_timezone(time_zone_id, actor)
    }

    pub fn business_timezone(&self) -> Result<String, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped).business_timezone();
        }
        SqliteR3SettlementRepository::new(self.scoped).business_timezone()
    }

    pub fn complete_inspection(
        &self,
        input: InspectionCompletionInput,
        actor: &str,
    ) -> Result<InspectionCompletionProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped)
                .complete_inspection(input, actor);
        }
        SqliteR3SettlementRepository::new(self.scoped).complete_inspection(input, actor)
    }

    pub fn create_damage_finding(
        &self,
        input: DamageFindingInput,
        actor: &str,
    ) -> Result<DamageFindingProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped)
                .create_damage_finding(input, actor);
        }
        SqliteR3SettlementRepository::new(self.scoped).create_damage_finding(input, actor)
    }

    pub fn decide_liability(
        &self,
        input: LiabilityDecisionInput,
        actor: &str,
    ) -> Result<LiabilityDecisionProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped).decide_liability(input, actor);
        }
        SqliteR3SettlementRepository::new(self.scoped).decide_liability(input, actor)
    }

    pub fn decide_repair(
        &self,
        input: RepairDecisionInput,
        actor: &str,
    ) -> Result<RepairCaseProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped).decide_repair(input, actor);
        }
        SqliteR3SettlementRepository::new(self.scoped).decide_repair(input, actor)
    }

    pub fn transition_repair(
        &self,
        input: RepairTransitionInput,
    ) -> Result<RepairCaseProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped).transition_repair(input);
        }
        SqliteR3SettlementRepository::new(self.scoped).transition_repair(input)
    }

    pub fn propose_settlement(
        &self,
        input: SettlementProposalInput,
        actor: &str,
    ) -> Result<SettlementCaseProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped)
                .propose_settlement(input, actor);
        }
        SqliteR3SettlementRepository::new(self.scoped).propose_settlement(input, actor)
    }

    pub fn accept_settlement(
        &self,
        settlement_case_id: &str,
    ) -> Result<SettlementCaseProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped)
                .accept_settlement(settlement_case_id);
        }
        SqliteR3SettlementRepository::new(self.scoped).accept_settlement(settlement_case_id)
    }

    pub fn deduct_deposit(
        &self,
        input: DepositDeductionInput,
        actor: &str,
    ) -> Result<SettlementEffectAdmissionProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped).deduct_deposit(input, actor);
        }
        SqliteR3SettlementRepository::new(self.scoped).deduct_deposit(input, actor)
    }

    pub fn admit_additional_charge(
        &self,
        input: AdditionalChargeInput,
        actor: &str,
    ) -> Result<SettlementEffectAdmissionProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped)
                .admit_additional_charge(input, actor);
        }
        SqliteR3SettlementRepository::new(self.scoped).admit_additional_charge(input, actor)
    }

    pub fn open_dispute(
        &self,
        input: OpenDisputeInput,
        actor: &str,
    ) -> Result<DisputeProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped).open_dispute(input, actor);
        }
        SqliteR3SettlementRepository::new(self.scoped).open_dispute(input, actor)
    }

    pub fn resolve_dispute(
        &self,
        input: ResolveDisputeInput,
        actor: &str,
    ) -> Result<DisputeProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped).resolve_dispute(input, actor);
        }
        SqliteR3SettlementRepository::new(self.scoped).resolve_dispute(input, actor)
    }

    pub fn complete_settlement(
        &self,
        settlement_case_id: &str,
    ) -> Result<SettlementCaseProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped)
                .complete_settlement(settlement_case_id);
        }
        SqliteR3SettlementRepository::new(self.scoped).complete_settlement(settlement_case_id)
    }

    pub fn closure_facts(&self, order_id: &str) -> Result<R3ClosureFacts, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped).closure_facts(order_id);
        }
        SqliteR3SettlementRepository::new(self.scoped).closure_facts(order_id)
    }

    pub fn close_order_atomic(
        &self,
        order_id: &str,
        expected_version: i64,
        actor: &str,
    ) -> Result<(), RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresR3SettlementRepository::new(self.scoped).close_order_atomic(
                order_id,
                expected_version,
                actor,
            );
        }
        SqliteR3SettlementRepository::new(self.scoped).close_order_atomic(
            order_id,
            expected_version,
            actor,
        )
    }
}
