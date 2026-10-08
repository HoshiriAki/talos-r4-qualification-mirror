#![cfg(feature = "postgres")]

use crate::repositories::RepositoryError;
use crate::repositories::damage::{
    DamageListProjection, DamageMutationError, DamageProjection, DamageReportOutcome,
    DamageTransitionOutcome,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresDamageRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresDamageRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn report(
        &self,
        order_id: &str,
        device_serial_no: &str,
        appearance_ok: bool,
        accessories_ok: bool,
        function_ok: bool,
        damage_description: &str,
        operator: &str,
        now: &str,
    ) -> Result<DamageReportOutcome, DamageMutationError> {
        crate::repositories::damage_postgres_report::report(
            self.session,
            order_id,
            device_serial_no,
            appearance_ok,
            accessories_ok,
            function_ok,
            damage_description,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn assess(
        &self,
        damage_id: &str,
        estimated_damage_amount: f64,
        liability: &str,
        notes: &str,
        operator: &str,
        now: &str,
    ) -> Result<DamageTransitionOutcome, DamageMutationError> {
        crate::repositories::damage_postgres_transition::assess(
            self.session,
            damage_id,
            estimated_damage_amount,
            liability,
            notes,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn adjudicate(
        &self,
        damage_id: &str,
        operator: &str,
        now: &str,
    ) -> Result<DamageTransitionOutcome, DamageMutationError> {
        crate::repositories::damage_postgres_transition::adjudicate(
            self.session,
            damage_id,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn get_by_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<DamageProjection>, RepositoryError> {
        crate::repositories::damage_postgres_read::get_by_order(self.session, order_id)
    }

    pub(in crate::repositories) fn get_by_device(
        &self,
        device_serial_no: &str,
    ) -> Result<Vec<DamageProjection>, RepositoryError> {
        crate::repositories::damage_postgres_read::get_by_device(self.session, device_serial_no)
    }

    pub(in crate::repositories) fn list(
        &self,
        status: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<DamageListProjection, RepositoryError> {
        crate::repositories::damage_postgres_read::list(self.session, status, page, page_size)
    }
}
