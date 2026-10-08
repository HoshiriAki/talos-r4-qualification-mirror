#![cfg(feature = "postgres")]

use crate::repositories::RepositoryError;
use crate::repositories::barcode::{
    BarcodeLabelProjection, BarcodeLookupProjection, BarcodeMutationError, ScanEventProjection,
    ScanHistoryProjection, ScanStatsProjection,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresBarcodeRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresBarcodeRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn generate(
        &self,
        device_serial_no: &str,
        now: &str,
    ) -> Result<(BarcodeLabelProjection, bool), BarcodeMutationError> {
        crate::repositories::barcode_postgres_mutation::generate(
            self.session,
            device_serial_no,
            now,
        )
    }

    pub(in crate::repositories) fn batch_generate(
        &self,
        now: &str,
    ) -> Result<i64, BarcodeMutationError> {
        crate::repositories::barcode_postgres_mutation::batch_generate(self.session, now)
    }

    pub(in crate::repositories) fn record_scan(
        &self,
        device_serial_no: &str,
        barcode_text: Option<&str>,
        scan_type: &str,
        scanned_by: &str,
        warehouse_id: Option<&str>,
        notes: Option<&str>,
        now: &str,
    ) -> Result<ScanEventProjection, BarcodeMutationError> {
        crate::repositories::barcode_postgres_mutation::record_scan(
            self.session,
            device_serial_no,
            barcode_text,
            scan_type,
            scanned_by,
            warehouse_id,
            notes,
            now,
        )
    }

    pub(in crate::repositories) fn lookup(
        &self,
        barcode_text: &str,
    ) -> Result<Option<BarcodeLookupProjection>, RepositoryError> {
        crate::repositories::barcode_postgres_read::lookup(self.session, barcode_text)
    }

    pub(in crate::repositories) fn history(
        &self,
        device_serial_no: Option<&str>,
        scan_type: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<ScanHistoryProjection, RepositoryError> {
        crate::repositories::barcode_postgres_read::history(
            self.session,
            device_serial_no,
            scan_type,
            start_date,
            end_date,
            page,
            page_size,
        )
    }

    pub(in crate::repositories) fn stats(
        &self,
        today: &str,
        week_start: &str,
    ) -> Result<ScanStatsProjection, RepositoryError> {
        crate::repositories::barcode_postgres_read::stats(self.session, today, week_start)
    }
}
