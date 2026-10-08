use crate::domain::{Money, QuoteId};
use crate::repositories::quote::{
    AccessoryProjection, NewQuote, OrderFromQuoteProjection, QuoteProjection,
    ScopedQuoteRepository as SqliteQuoteRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::quote_postgres::PostgresQuoteRepository;

pub struct ScopedQuoteRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedQuoteRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn upsert_accessory(
        &self,
        id: &str,
        sku: &str,
        name: &str,
        unit_price: &Money,
        active: bool,
    ) -> Result<AccessoryProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresQuoteRepository::new(self.scoped.session())
                .upsert_accessory(id, sku, name, unit_price, active);
        }
        SqliteQuoteRepository::new(self.scoped).upsert_accessory(id, sku, name, unit_price, active)
    }

    pub fn get_accessory(&self, id: &str) -> Result<Option<AccessoryProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresQuoteRepository::new(self.scoped.session()).get_accessory(id);
        }
        SqliteQuoteRepository::new(self.scoped).get_accessory(id)
    }

    pub fn create(&self, quote: &NewQuote) -> Result<QuoteProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresQuoteRepository::new(self.scoped.session()).create(quote);
        }
        SqliteQuoteRepository::new(self.scoped).create(quote)
    }

    pub fn list(&self, limit: u32) -> Result<Vec<QuoteProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresQuoteRepository::new(self.scoped.session()).list(limit);
        }
        SqliteQuoteRepository::new(self.scoped).list(limit)
    }

    pub fn get(&self, id: &QuoteId) -> Result<Option<QuoteProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresQuoteRepository::new(self.scoped.session()).get(id);
        }
        SqliteQuoteRepository::new(self.scoped).get(id)
    }

    pub fn confirm(&self, id: &QuoteId) -> Result<QuoteProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresQuoteRepository::new(self.scoped.session()).confirm(id);
        }
        SqliteQuoteRepository::new(self.scoped).confirm(id)
    }

    pub fn expire(&self, id: &QuoteId) -> Result<QuoteProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresQuoteRepository::new(self.scoped.session()).expire(id);
        }
        SqliteQuoteRepository::new(self.scoped).expire(id)
    }

    pub fn create_order_from_quote(
        &self,
        quote_id: &QuoteId,
        remark: &str,
    ) -> Result<OrderFromQuoteProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresQuoteRepository::new(self.scoped.session())
                .create_order_from_quote(quote_id, remark);
        }
        SqliteQuoteRepository::new(self.scoped).create_order_from_quote(quote_id, remark)
    }

    pub fn default_expiry() -> String {
        SqliteQuoteRepository::default_expiry()
    }
}
