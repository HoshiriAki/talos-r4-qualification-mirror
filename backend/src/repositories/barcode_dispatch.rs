use crate::repositories::barcode::{
    BarcodeLabelProjection, BarcodeLookupProjection, BarcodeMutationError, ScanEventProjection,
    ScanHistoryProjection, ScanStatsProjection, SqliteBarcodeRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::barcode_postgres::PostgresBarcodeRepository;

pub struct ScopedBarcodeRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedBarcodeRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn generate(
        &self,
        device_serial_no: &str,
        now: &str,
    ) -> Result<(BarcodeLabelProjection, bool), BarcodeMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresBarcodeRepository::new(self.scoped.session())
                .generate(device_serial_no, now);
        }
        SqliteBarcodeRepository::new(self.scoped).generate(device_serial_no, now)
    }

    pub fn batch_generate(&self, now: &str) -> Result<i64, BarcodeMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresBarcodeRepository::new(self.scoped.session()).batch_generate(now);
        }
        SqliteBarcodeRepository::new(self.scoped).batch_generate(now)
    }

    pub fn lookup(
        &self,
        barcode_text: &str,
    ) -> Result<Option<BarcodeLookupProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresBarcodeRepository::new(self.scoped.session()).lookup(barcode_text);
        }
        SqliteBarcodeRepository::new(self.scoped).lookup(barcode_text)
    }

    pub fn record_scan(
        &self,
        device_serial_no: &str,
        barcode_text: Option<&str>,
        scan_type: &str,
        scanned_by: &str,
        warehouse_id: Option<&str>,
        notes: Option<&str>,
        now: &str,
    ) -> Result<ScanEventProjection, BarcodeMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresBarcodeRepository::new(self.scoped.session()).record_scan(
                device_serial_no,
                barcode_text,
                scan_type,
                scanned_by,
                warehouse_id,
                notes,
                now,
            );
        }
        SqliteBarcodeRepository::new(self.scoped).record_scan(
            device_serial_no,
            barcode_text,
            scan_type,
            scanned_by,
            warehouse_id,
            notes,
            now,
        )
    }

    pub fn history(
        &self,
        device_serial_no: Option<&str>,
        scan_type: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<ScanHistoryProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresBarcodeRepository::new(self.scoped.session()).history(
                device_serial_no,
                scan_type,
                start_date,
                end_date,
                page,
                page_size,
            );
        }
        SqliteBarcodeRepository::new(self.scoped).history(
            device_serial_no,
            scan_type,
            start_date,
            end_date,
            page,
            page_size,
        )
    }

    pub fn stats(
        &self,
        today: &str,
        week_start: &str,
    ) -> Result<ScanStatsProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresBarcodeRepository::new(self.scoped.session()).stats(today, week_start);
        }
        SqliteBarcodeRepository::new(self.scoped).stats(today, week_start)
    }
}
