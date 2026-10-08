#![cfg(feature = "postgres")]

use crate::repositories::RepositoryError;
use crate::repositories::session::RepositorySession;
use crate::repositories::tax::{TaxConfigProjection, TaxExportSnapshot, TaxUpsertOutcome};

pub(in crate::repositories) struct PostgresTaxRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresTaxRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn get_configs(
        &self,
        tax_type: &str,
    ) -> Result<Vec<TaxConfigProjection>, RepositoryError> {
        crate::repositories::tax_postgres_read::get_configs(self.session, tax_type)
    }

    pub(in crate::repositories) fn active_rate(
        &self,
        tax_type: &str,
    ) -> Result<Option<f64>, RepositoryError> {
        crate::repositories::tax_postgres_read::active_rate(self.session, tax_type)
    }

    pub(in crate::repositories) fn upsert_config(
        &self,
        tax_type: &str,
        rate: f64,
        effective_from: &str,
        now: &str,
    ) -> Result<TaxUpsertOutcome, RepositoryError> {
        crate::repositories::tax_postgres_mutation::upsert_config(
            self.session,
            tax_type,
            rate,
            effective_from,
            now,
        )
    }

    pub(in crate::repositories) fn export_snapshot(
        &self,
        period_prefix: &str,
    ) -> Result<TaxExportSnapshot, RepositoryError> {
        crate::repositories::tax_postgres_read::export_snapshot(self.session, period_prefix)
    }
}
