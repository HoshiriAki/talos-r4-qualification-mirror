use chrono::NaiveDate;

use crate::repositories::booking_read::{
    BookingAvailabilityProjection, BookingPriceProjection, BookingSearchItem,
    SqliteBookingReadRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::booking_read_postgres::PostgresBookingReadRepository;

pub struct ScopedBookingReadRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedBookingReadRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn availability(
        &self,
        device_serial_no: Option<&str>,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<BookingAvailabilityProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresBookingReadRepository::new(self.scoped.session()).availability(
                device_serial_no,
                start_date,
                end_date,
            );
        }
        SqliteBookingReadRepository::new(self.scoped).availability(
            device_serial_no,
            start_date,
            end_date,
        )
    }

    pub fn price(
        &self,
        device_serial_no: &str,
    ) -> Result<Option<BookingPriceProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresBookingReadRepository::new(self.scoped.session())
                .price(device_serial_no);
        }
        SqliteBookingReadRepository::new(self.scoped).price(device_serial_no)
    }

    pub fn search(
        &self,
        query: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<BookingSearchItem>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresBookingReadRepository::new(self.scoped.session()).search(query, model);
        }
        SqliteBookingReadRepository::new(self.scoped).search(query, model)
    }
}
