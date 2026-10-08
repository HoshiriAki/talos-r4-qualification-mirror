use crate::repositories::invoice::{
    InvoiceIssueOutcome, InvoiceListProjection, InvoiceMutationError, InvoiceProjection,
    InvoiceRedFlushOutcome, InvoiceVoidOutcome, SqliteInvoiceRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::invoice_postgres::PostgresInvoiceRepository;

pub struct ScopedInvoiceRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedInvoiceRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn issue(
        &self,
        order_id: &str,
        amount: f64,
        invoice_type: &str,
        tax_rate: Option<f64>,
        now: &str,
    ) -> Result<InvoiceIssueOutcome, InvoiceMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresInvoiceRepository::new(self.scoped.session()).issue(
                order_id,
                amount,
                invoice_type,
                tax_rate,
                now,
            );
        }
        SqliteInvoiceRepository::new(self.scoped).issue(
            order_id,
            amount,
            invoice_type,
            tax_rate,
            now,
        )
    }

    pub fn void(
        &self,
        invoice_id: &str,
        now: &str,
    ) -> Result<InvoiceVoidOutcome, InvoiceMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresInvoiceRepository::new(self.scoped.session()).void(invoice_id, now);
        }
        SqliteInvoiceRepository::new(self.scoped).void(invoice_id, now)
    }

    pub fn red_flush(
        &self,
        invoice_id: &str,
        reason: &str,
        now: &str,
    ) -> Result<InvoiceRedFlushOutcome, InvoiceMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresInvoiceRepository::new(self.scoped.session())
                .red_flush(invoice_id, reason, now);
        }
        SqliteInvoiceRepository::new(self.scoped).red_flush(invoice_id, reason, now)
    }

    pub fn get_by_id(
        &self,
        invoice_id: &str,
    ) -> Result<Option<InvoiceProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresInvoiceRepository::new(self.scoped.session()).get_by_id(invoice_id);
        }
        SqliteInvoiceRepository::new(self.scoped).get_by_id(invoice_id)
    }

    pub fn get_by_order(
        &self,
        order_id: &str,
    ) -> Result<Option<InvoiceProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresInvoiceRepository::new(self.scoped.session()).get_by_order(order_id);
        }
        SqliteInvoiceRepository::new(self.scoped).get_by_order(order_id)
    }

    pub fn list(
        &self,
        status: Option<&str>,
        date_from: Option<&str>,
        date_to: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<InvoiceListProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresInvoiceRepository::new(self.scoped.session())
                .list(status, date_from, date_to, page, page_size);
        }
        SqliteInvoiceRepository::new(self.scoped).list(status, date_from, date_to, page, page_size)
    }
}
