#![cfg(feature = "postgres")]

use crate::repositories::RepositoryError;
use crate::repositories::invoice::{
    InvoiceIssueOutcome, InvoiceListProjection, InvoiceMutationError, InvoiceProjection,
    InvoiceRedFlushOutcome, InvoiceVoidOutcome,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresInvoiceRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresInvoiceRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn issue(
        &self,
        order_id: &str,
        amount: f64,
        invoice_type: &str,
        tax_rate: Option<f64>,
        now: &str,
    ) -> Result<InvoiceIssueOutcome, InvoiceMutationError> {
        crate::repositories::invoice_postgres_mutation::issue(
            self.session,
            order_id,
            amount,
            invoice_type,
            tax_rate,
            now,
        )
    }

    pub(in crate::repositories) fn void(
        &self,
        invoice_id: &str,
        now: &str,
    ) -> Result<InvoiceVoidOutcome, InvoiceMutationError> {
        crate::repositories::invoice_postgres_mutation::void(self.session, invoice_id, now)
    }

    pub(in crate::repositories) fn red_flush(
        &self,
        invoice_id: &str,
        reason: &str,
        now: &str,
    ) -> Result<InvoiceRedFlushOutcome, InvoiceMutationError> {
        crate::repositories::invoice_postgres_mutation::red_flush(
            self.session,
            invoice_id,
            reason,
            now,
        )
    }

    pub(in crate::repositories) fn get_by_id(
        &self,
        invoice_id: &str,
    ) -> Result<Option<InvoiceProjection>, RepositoryError> {
        crate::repositories::invoice_postgres_read::get_by_id(self.session, invoice_id)
    }

    pub(in crate::repositories) fn get_by_order(
        &self,
        order_id: &str,
    ) -> Result<Option<InvoiceProjection>, RepositoryError> {
        crate::repositories::invoice_postgres_read::get_by_order(self.session, order_id)
    }

    pub(in crate::repositories) fn list(
        &self,
        status: Option<&str>,
        date_from: Option<&str>,
        date_to: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<InvoiceListProjection, RepositoryError> {
        crate::repositories::invoice_postgres_read::list(
            self.session,
            status,
            date_from,
            date_to,
            page,
            page_size,
        )
    }
}
