use crate::repositories::order_read::{
    OrderListRequest, OrderReadPage, OrderReadProjection,
    ScopedOrderReadRepository as SqliteOrderReadRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::order_read_postgres::PostgresOrderReadRepository;

pub struct ScopedOrderReadRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedOrderReadRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn list(&self, request: &OrderListRequest) -> Result<OrderReadPage, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOrderReadRepository::new(self.scoped.session()).list(request);
        }

        SqliteOrderReadRepository::new(self.scoped).list(request)
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<OrderReadProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOrderReadRepository::new(self.scoped.session()).get_by_id(id);
        }

        SqliteOrderReadRepository::new(self.scoped).get_by_id(id)
    }
}
