use serde_json::Value;

use crate::repositories::contract::{
    ContractGenerateOutcome, ContractListProjection, ContractMutationError, ContractProjection,
    ContractSignOutcome, ContractTemplateCreateOutcome, ContractTemplateListProjection,
    ContractTemplateProjection, ContractTemplateUpdateOutcome, ContractVerifyOutcome,
    SqliteContractRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::contract_postgres::PostgresContractRepository;

pub struct ScopedContractRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedContractRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn template_create(
        &self,
        name: &str,
        content_json: &Value,
        now: &str,
    ) -> Result<ContractTemplateCreateOutcome, ContractMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresContractRepository::new(self.scoped.session()).template_create(
                name,
                content_json,
                now,
            );
        }
        SqliteContractRepository::new(self.scoped).template_create(name, content_json, now)
    }

    pub fn template_update(
        &self,
        id: i64,
        name: Option<&str>,
        content_json: Option<&Value>,
        is_active: Option<bool>,
        now: &str,
    ) -> Result<ContractTemplateUpdateOutcome, ContractMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresContractRepository::new(self.scoped.session()).template_update(
                id,
                name,
                content_json,
                is_active,
                now,
            );
        }
        SqliteContractRepository::new(self.scoped).template_update(
            id,
            name,
            content_json,
            is_active,
            now,
        )
    }

    pub fn template_list(
        &self,
        is_active: Option<bool>,
        page: i64,
        page_size: i64,
    ) -> Result<ContractTemplateListProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresContractRepository::new(self.scoped.session())
                .template_list(is_active, page, page_size);
        }
        SqliteContractRepository::new(self.scoped).template_list(is_active, page, page_size)
    }

    pub fn template_get(
        &self,
        id: i64,
    ) -> Result<Option<ContractTemplateProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresContractRepository::new(self.scoped.session()).template_get(id);
        }
        SqliteContractRepository::new(self.scoped).template_get(id)
    }

    pub fn generate(
        &self,
        order_id: &str,
        template_id: Option<i64>,
        customer_name: &str,
        customer_phone: &str,
        device_value: f64,
        variables: Option<&Value>,
        now: &str,
    ) -> Result<ContractGenerateOutcome, ContractMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresContractRepository::new(self.scoped.session()).generate(
                order_id,
                template_id,
                customer_name,
                customer_phone,
                device_value,
                variables,
                now,
            );
        }
        SqliteContractRepository::new(self.scoped).generate(
            order_id,
            template_id,
            customer_name,
            customer_phone,
            device_value,
            variables,
            now,
        )
    }

    pub fn sign(
        &self,
        id: i64,
        signer_name: &str,
        signer_phone: &str,
        signature_data: &str,
        now: &str,
    ) -> Result<ContractSignOutcome, ContractMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresContractRepository::new(self.scoped.session()).sign(
                id,
                signer_name,
                signer_phone,
                signature_data,
                now,
            );
        }
        SqliteContractRepository::new(self.scoped).sign(
            id,
            signer_name,
            signer_phone,
            signature_data,
            now,
        )
    }

    pub fn verify(
        &self,
        id: i64,
        now: &str,
    ) -> Result<ContractVerifyOutcome, ContractMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresContractRepository::new(self.scoped.session()).verify(id, now);
        }
        SqliteContractRepository::new(self.scoped).verify(id, now)
    }

    pub fn list(
        &self,
        order_id: Option<&str>,
        customer_phone: Option<&str>,
        status: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<ContractListProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresContractRepository::new(self.scoped.session()).list(
                order_id,
                customer_phone,
                status,
                page,
                page_size,
            );
        }
        SqliteContractRepository::new(self.scoped).list(
            order_id,
            customer_phone,
            status,
            page,
            page_size,
        )
    }

    pub fn get(&self, id: i64) -> Result<Option<ContractProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresContractRepository::new(self.scoped.session()).get(id);
        }
        SqliteContractRepository::new(self.scoped).get(id)
    }
}
