use crate::repositories::device::{
    DeviceCheckinStatusMutation, DeviceCompatibilityReadRequest, DeviceCompatibilityReadRow,
    DeviceListRequest, DevicePage, DevicePagedRequest, DevicePatch, DeviceProjection,
    ImportedDeviceDraft, NewDevice, SqliteDeviceRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::device_postgres::PostgresDeviceRepository;

pub struct ScopedDeviceRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedDeviceRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn list(
        &self,
        request: &DeviceListRequest,
    ) -> Result<Vec<DeviceProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session()).list(request);
        }
        SqliteDeviceRepository::new(self.scoped).list(request)
    }

    pub fn list_paged(&self, request: &DevicePagedRequest) -> Result<DevicePage, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session()).list_paged(request);
        }
        SqliteDeviceRepository::new(self.scoped).list_paged(request)
    }

    pub fn compatibility_count(
        &self,
        request: &DeviceCompatibilityReadRequest,
    ) -> Result<u32, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session())
                .compatibility_count(request);
        }
        SqliteDeviceRepository::new(self.scoped).compatibility_count(request)
    }

    pub fn compatibility_rows(
        &self,
        request: &DeviceCompatibilityReadRequest,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<DeviceCompatibilityReadRow>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session())
                .compatibility_rows(request, limit, offset);
        }
        SqliteDeviceRepository::new(self.scoped).compatibility_rows(request, limit, offset)
    }

    pub fn compatibility_rows_by_serials(
        &self,
        serial_nos: &[String],
    ) -> Result<Vec<DeviceCompatibilityReadRow>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session())
                .compatibility_rows_by_serials(serial_nos);
        }
        SqliteDeviceRepository::new(self.scoped).compatibility_rows_by_serials(serial_nos)
    }

    pub fn checkin_status(
        &self,
        serial_no: &str,
        checked_in_status: &str,
    ) -> Result<Option<DeviceCheckinStatusMutation>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session())
                .checkin_status(serial_no, checked_in_status);
        }
        SqliteDeviceRepository::new(self.scoped).checkin_status(serial_no, checked_in_status)
    }

    pub fn complete_legacy_orders_after_checkin(
        &self,
        serial_no: &str,
        checked_in_status: &str,
    ) -> Result<Vec<String>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session())
                .complete_legacy_orders_after_checkin(serial_no, checked_in_status);
        }
        SqliteDeviceRepository::new(self.scoped)
            .complete_legacy_orders_after_checkin(serial_no, checked_in_status)
    }

    pub fn restore_status(
        &self,
        serial_no: &str,
        previous_status: &str,
    ) -> Result<bool, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session())
                .restore_status(serial_no, previous_status);
        }
        SqliteDeviceRepository::new(self.scoped).restore_status(serial_no, previous_status)
    }

    pub fn update_status_and_notes(
        &self,
        serial_no: &str,
        status: &str,
        notes: &str,
    ) -> Result<bool, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session())
                .update_status_and_notes(serial_no, status, notes);
        }
        SqliteDeviceRepository::new(self.scoped).update_status_and_notes(serial_no, status, notes)
    }

    pub fn import_device(&self, draft: &ImportedDeviceDraft) -> Result<bool, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session()).import_device(draft);
        }
        SqliteDeviceRepository::new(self.scoped).import_device(draft)
    }

    pub fn get(&self, serial_no: &str) -> Result<Option<DeviceProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session()).get(serial_no);
        }
        SqliteDeviceRepository::new(self.scoped).get(serial_no)
    }

    pub fn create(&self, input: &NewDevice) -> Result<DeviceProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session()).create(input);
        }
        SqliteDeviceRepository::new(self.scoped).create(input)
    }

    pub fn update(
        &self,
        serial_no: &str,
        patch: &DevicePatch,
    ) -> Result<Option<DeviceProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session()).update(serial_no, patch);
        }
        SqliteDeviceRepository::new(self.scoped).update(serial_no, patch)
    }

    pub fn delete(&self, serial_no: &str) -> Result<bool, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session()).delete(serial_no);
        }
        SqliteDeviceRepository::new(self.scoped).delete(serial_no)
    }

    pub fn list_serials(&self) -> Result<Vec<String>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDeviceRepository::new(self.scoped.session()).list_serials();
        }
        SqliteDeviceRepository::new(self.scoped).list_serials()
    }
}
