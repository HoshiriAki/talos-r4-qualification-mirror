use crate::domain::{AllocationId, ReservationId};
use crate::repositories::reservation::{
    AllocationProjection, CapacityProjection, DeviceAllocationRequest,
    ReservationMigrationExceptionProjection, ReservationProjection,
    ScopedReservationRepository as SqliteReservationRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::reservation_postgres::PostgresReservationRepository;
#[cfg(feature = "postgres")]
use crate::repositories::reservation_postgres_write::PostgresReservationWriteRepository;

pub struct ScopedReservationRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedReservationRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn create_from_order(
        &self,
        id: ReservationId,
        order_id: &str,
        hold_minutes: i64,
    ) -> Result<ReservationProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            let reload_id = id.clone();
            PostgresReservationWriteRepository::new(self.scoped.session()).create_from_order(
                id,
                order_id,
                hold_minutes,
            )?;
            return PostgresReservationRepository::new(self.scoped.session())
                .get(&reload_id)?
                .ok_or_else(|| {
                    RepositoryError::ContractViolation(
                        "created reservation could not be reloaded".into(),
                    )
                });
        }
        SqliteReservationRepository::new(self.scoped).create_from_order(id, order_id, hold_minutes)
    }

    pub fn find_by_order(
        &self,
        order_id: &str,
    ) -> Result<Option<ReservationProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresReservationRepository::new(self.scoped.session())
                .find_by_order(order_id);
        }
        SqliteReservationRepository::new(self.scoped).find_by_order(order_id)
    }

    pub fn allocation_complete_for_order(&self, order_id: &str) -> Result<bool, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresReservationRepository::new(self.scoped.session())
                .allocation_complete_for_order(order_id);
        }
        SqliteReservationRepository::new(self.scoped).allocation_complete_for_order(order_id)
    }

    pub fn create_legacy_device_hold(
        &self,
        id: ReservationId,
        device_serial_no: &str,
        start_date: &str,
        end_date: &str,
        hold_minutes: i64,
    ) -> Result<ReservationProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            let reload_id = id.clone();
            PostgresReservationWriteRepository::new(self.scoped.session())
                .create_legacy_device_hold(
                    id,
                    device_serial_no,
                    start_date,
                    end_date,
                    hold_minutes,
                )?;
            return PostgresReservationRepository::new(self.scoped.session())
                .get(&reload_id)?
                .ok_or_else(|| {
                    RepositoryError::ContractViolation(
                        "created legacy hold could not be reloaded".into(),
                    )
                });
        }
        SqliteReservationRepository::new(self.scoped).create_legacy_device_hold(
            id,
            device_serial_no,
            start_date,
            end_date,
            hold_minutes,
        )
    }

    pub fn capacity(
        &self,
        model_id: &str,
        start_date: &str,
        end_date: &str,
        quantity: u32,
        device_serial_no: Option<&str>,
    ) -> Result<CapacityProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresReservationRepository::new(self.scoped.session()).capacity(
                model_id,
                start_date,
                end_date,
                quantity,
                device_serial_no,
            );
        }
        SqliteReservationRepository::new(self.scoped).capacity(
            model_id,
            start_date,
            end_date,
            quantity,
            device_serial_no,
        )
    }

    pub fn confirm(
        &self,
        id: &ReservationId,
        attach_order_id: Option<&str>,
    ) -> Result<ReservationProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            PostgresReservationWriteRepository::new(self.scoped.session())
                .confirm(id, attach_order_id)?;
            return PostgresReservationRepository::new(self.scoped.session())
                .get(id)?
                .ok_or_else(|| RepositoryError::ContractViolation("reservation not found".into()));
        }
        SqliteReservationRepository::new(self.scoped).confirm(id, attach_order_id)
    }

    pub fn expire_due(&self) -> Result<u64, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresReservationWriteRepository::new(self.scoped.session()).expire_due();
        }
        SqliteReservationRepository::new(self.scoped).expire_due()
    }

    pub fn allocate_device(
        &self,
        reservation_id: &ReservationId,
        allocation_id: AllocationId,
        device_serial_no: &str,
    ) -> Result<AllocationProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresReservationWriteRepository::new(self.scoped.session()).allocate_device(
                reservation_id,
                allocation_id,
                device_serial_no,
            );
        }
        SqliteReservationRepository::new(self.scoped).allocate_device(
            reservation_id,
            allocation_id,
            device_serial_no,
        )
    }

    pub fn allocate_devices_batch(
        &self,
        requests: &[DeviceAllocationRequest],
    ) -> Result<Vec<AllocationProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresReservationWriteRepository::new(self.scoped.session())
                .allocate_devices_batch(requests);
        }
        SqliteReservationRepository::new(self.scoped).allocate_devices_batch(requests)
    }

    pub fn release_allocation(
        &self,
        id: &AllocationId,
    ) -> Result<AllocationProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresReservationWriteRepository::new(self.scoped.session())
                .release_allocation(id);
        }
        SqliteReservationRepository::new(self.scoped).release_allocation(id)
    }

    pub fn get(
        &self,
        id: &ReservationId,
    ) -> Result<Option<ReservationProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresReservationRepository::new(self.scoped.session()).get(id);
        }
        SqliteReservationRepository::new(self.scoped).get(id)
    }

    pub fn list_migration_exceptions(
        &self,
    ) -> Result<Vec<ReservationMigrationExceptionProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresReservationRepository::new(self.scoped.session())
                .list_migration_exceptions();
        }
        SqliteReservationRepository::new(self.scoped).list_migration_exceptions()
    }
}
