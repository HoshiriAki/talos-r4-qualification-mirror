#![cfg(feature = "postgres")]

use serde_json::Value;

use crate::repositories::RepositoryError;
use crate::repositories::contract::{
    ContractGenerateOutcome, ContractListProjection, ContractMutationError, ContractProjection,
    ContractSignOutcome, ContractTemplateCreateOutcome, ContractTemplateListProjection,
    ContractTemplateProjection, ContractTemplateUpdateOutcome, ContractVerifyOutcome,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresContractRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresContractRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn template_create(
        &self,
        name: &str,
        content_json: &Value,
        now: &str,
    ) -> Result<ContractTemplateCreateOutcome, ContractMutationError> {
        crate::repositories::contract_postgres_mutation::template_create(
            self.session,
            name,
            content_json,
            now,
        )
    }

    pub(in crate::repositories) fn template_update(
        &self,
        id: i64,
        name: Option<&str>,
        content_json: Option<&Value>,
        is_active: Option<bool>,
        now: &str,
    ) -> Result<ContractTemplateUpdateOutcome, ContractMutationError> {
        crate::repositories::contract_postgres_mutation::template_update(
            self.session,
            id,
            name,
            content_json,
            is_active,
            now,
        )
    }

    pub(in crate::repositories) fn template_list(
        &self,
        is_active: Option<bool>,
        page: i64,
        page_size: i64,
    ) -> Result<ContractTemplateListProjection, RepositoryError> {
        crate::repositories::contract_postgres_read::template_list(
            self.session,
            is_active,
            page,
            page_size,
        )
    }

    pub(in crate::repositories) fn template_get(
        &self,
        id: i64,
    ) -> Result<Option<ContractTemplateProjection>, RepositoryError> {
        crate::repositories::contract_postgres_read::template_get(self.session, id)
    }

    pub(in crate::repositories) fn generate(
        &self,
        order_id: &str,
        template_id: Option<i64>,
        customer_name: &str,
        customer_phone: &str,
        device_value: f64,
        variables: Option<&Value>,
        now: &str,
    ) -> Result<ContractGenerateOutcome, ContractMutationError> {
        crate::repositories::contract_postgres_mutation::generate(
            self.session,
            order_id,
            template_id,
            customer_name,
            customer_phone,
            device_value,
            variables,
            now,
        )
    }

    pub(in crate::repositories) fn sign(
        &self,
        id: i64,
        signer_name: &str,
        signer_phone: &str,
        signature_data: &str,
        now: &str,
    ) -> Result<ContractSignOutcome, ContractMutationError> {
        crate::repositories::contract_postgres_mutation::sign(
            self.session,
            id,
            signer_name,
            signer_phone,
            signature_data,
            now,
        )
    }

    pub(in crate::repositories) fn verify(
        &self,
        id: i64,
        now: &str,
    ) -> Result<ContractVerifyOutcome, ContractMutationError> {
        crate::repositories::contract_postgres_mutation::verify(self.session, id, now)
    }

    pub(in crate::repositories) fn list(
        &self,
        order_id: Option<&str>,
        customer_phone: Option<&str>,
        status: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<ContractListProjection, RepositoryError> {
        crate::repositories::contract_postgres_read::list(
            self.session,
            order_id,
            customer_phone,
            status,
            page,
            page_size,
        )
    }

    pub(in crate::repositories) fn get(
        &self,
        id: i64,
    ) -> Result<Option<ContractProjection>, RepositoryError> {
        crate::repositories::contract_postgres_read::get(self.session, id)
    }
}
