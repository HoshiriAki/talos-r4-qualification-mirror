use crate::domain::{ContactKind, CustomerId};
use crate::repositories::customer::{
    CustomerMigrationExceptionProjection, CustomerProjection, DuplicateCustomerCandidate,
    NewCustomerContact, NewCustomerRecord, ScopedCustomerRepository as SqliteCustomerRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::customer_postgres::PostgresCustomerRepository;

pub struct ScopedCustomerRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedCustomerRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn create(
        &self,
        record: &NewCustomerRecord,
    ) -> Result<CustomerProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCustomerRepository::new(self.scoped.session()).create(record);
        }
        SqliteCustomerRepository::new(self.scoped).create(record)
    }

    pub fn list(&self, limit: u32) -> Result<Vec<CustomerProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCustomerRepository::new(self.scoped.session()).list(limit);
        }
        SqliteCustomerRepository::new(self.scoped).list(limit)
    }

    pub fn get(&self, id: &CustomerId) -> Result<Option<CustomerProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCustomerRepository::new(self.scoped.session()).get(id);
        }
        SqliteCustomerRepository::new(self.scoped).get(id)
    }

    pub fn add_contact(
        &self,
        customer_id: &CustomerId,
        contact: &NewCustomerContact,
        actor_identity_id: Option<&str>,
    ) -> Result<CustomerProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCustomerRepository::new(self.scoped.session()).add_contact(
                customer_id,
                contact,
                actor_identity_id,
            );
        }
        SqliteCustomerRepository::new(self.scoped).add_contact(
            customer_id,
            contact,
            actor_identity_id,
        )
    }

    pub fn add_external_identity(
        &self,
        customer_id: &CustomerId,
        provider: &str,
        external_subject: &str,
        actor_identity_id: Option<&str>,
    ) -> Result<(), RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCustomerRepository::new(self.scoped.session()).add_external_identity(
                customer_id,
                provider,
                external_subject,
                actor_identity_id,
            );
        }
        SqliteCustomerRepository::new(self.scoped).add_external_identity(
            customer_id,
            provider,
            external_subject,
            actor_identity_id,
        )
    }

    pub fn duplicate_candidates(
        &self,
        kind: ContactKind,
        normalized_value: &str,
    ) -> Result<Vec<DuplicateCustomerCandidate>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCustomerRepository::new(self.scoped.session())
                .duplicate_candidates(kind, normalized_value);
        }
        SqliteCustomerRepository::new(self.scoped).duplicate_candidates(kind, normalized_value)
    }

    pub fn list_migration_exceptions(
        &self,
        limit: u32,
    ) -> Result<Vec<CustomerMigrationExceptionProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCustomerRepository::new(self.scoped.session())
                .list_migration_exceptions(limit);
        }
        SqliteCustomerRepository::new(self.scoped).list_migration_exceptions(limit)
    }

    pub fn resolve_migration_exception(
        &self,
        exception_id: &str,
        customer_id: &CustomerId,
        actor_identity_id: Option<&str>,
    ) -> Result<(), RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCustomerRepository::new(self.scoped.session())
                .resolve_migration_exception(exception_id, customer_id, actor_identity_id);
        }
        SqliteCustomerRepository::new(self.scoped).resolve_migration_exception(
            exception_id,
            customer_id,
            actor_identity_id,
        )
    }

    pub fn anonymize(
        &self,
        customer_id: &CustomerId,
        actor_identity_id: Option<&str>,
    ) -> Result<CustomerProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCustomerRepository::new(self.scoped.session())
                .anonymize(customer_id, actor_identity_id);
        }
        SqliteCustomerRepository::new(self.scoped).anonymize(customer_id, actor_identity_id)
    }
}
