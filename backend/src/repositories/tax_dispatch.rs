use crate::repositories::tax::{
    SqliteTaxRepository, TaxConfigProjection, TaxExportSnapshot, TaxUpsertOutcome,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::tax_postgres::PostgresTaxRepository;

pub struct ScopedTaxRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedTaxRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn get_configs(&self, tax_type: &str) -> Result<Vec<TaxConfigProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresTaxRepository::new(self.scoped.session()).get_configs(tax_type);
        }
        SqliteTaxRepository::new(self.scoped).get_configs(tax_type)
    }

    pub fn active_rate(&self, tax_type: &str) -> Result<Option<f64>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresTaxRepository::new(self.scoped.session()).active_rate(tax_type);
        }
        SqliteTaxRepository::new(self.scoped).active_rate(tax_type)
    }

    pub fn upsert_config(
        &self,
        tax_type: &str,
        rate: f64,
        effective_from: &str,
        now: &str,
    ) -> Result<TaxUpsertOutcome, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresTaxRepository::new(self.scoped.session()).upsert_config(
                tax_type,
                rate,
                effective_from,
                now,
            );
        }
        SqliteTaxRepository::new(self.scoped).upsert_config(tax_type, rate, effective_from, now)
    }

    pub fn export_snapshot(
        &self,
        period_prefix: &str,
    ) -> Result<TaxExportSnapshot, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresTaxRepository::new(self.scoped.session())
                .export_snapshot(period_prefix);
        }
        SqliteTaxRepository::new(self.scoped).export_snapshot(period_prefix)
    }
}
