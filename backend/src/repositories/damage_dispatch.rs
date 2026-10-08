use crate::repositories::damage::{
    DamageListProjection, DamageMutationError, DamageProjection, DamageReportOutcome,
    DamageTransitionOutcome, SqliteDamageRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::damage_postgres::PostgresDamageRepository;

pub struct ScopedDamageRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedDamageRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn report(
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
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDamageRepository::new(self.scoped.session()).report(
                order_id,
                device_serial_no,
                appearance_ok,
                accessories_ok,
                function_ok,
                damage_description,
                operator,
                now,
            );
        }
        SqliteDamageRepository::new(self.scoped).report(
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

    pub fn assess(
        &self,
        damage_id: &str,
        estimated_damage_amount: f64,
        liability: &str,
        notes: &str,
        operator: &str,
        now: &str,
    ) -> Result<DamageTransitionOutcome, DamageMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDamageRepository::new(self.scoped.session()).assess(
                damage_id,
                estimated_damage_amount,
                liability,
                notes,
                operator,
                now,
            );
        }
        SqliteDamageRepository::new(self.scoped).assess(
            damage_id,
            estimated_damage_amount,
            liability,
            notes,
            operator,
            now,
        )
    }

    pub fn adjudicate(
        &self,
        damage_id: &str,
        operator: &str,
        now: &str,
    ) -> Result<DamageTransitionOutcome, DamageMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDamageRepository::new(self.scoped.session())
                .adjudicate(damage_id, operator, now);
        }
        SqliteDamageRepository::new(self.scoped).adjudicate(damage_id, operator, now)
    }

    pub fn get_by_order(&self, order_id: &str) -> Result<Vec<DamageProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDamageRepository::new(self.scoped.session()).get_by_order(order_id);
        }
        SqliteDamageRepository::new(self.scoped).get_by_order(order_id)
    }

    pub fn get_by_device(
        &self,
        device_serial_no: &str,
    ) -> Result<Vec<DamageProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDamageRepository::new(self.scoped.session())
                .get_by_device(device_serial_no);
        }
        SqliteDamageRepository::new(self.scoped).get_by_device(device_serial_no)
    }

    pub fn list(
        &self,
        status: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<DamageListProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDamageRepository::new(self.scoped.session())
                .list(status, page, page_size);
        }
        SqliteDamageRepository::new(self.scoped).list(status, page, page_size)
    }
}
