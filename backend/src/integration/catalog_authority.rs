use std::collections::BTreeSet;
use std::sync::Arc;

use super::catalog::ProviderCatalog;
use super::catalog_repository::IntegrationCatalogRepository;
use super::keystore::{KeyStore, SecretEnvironment, SecretRegistrationScope};
use super::types::{
    IntegrationError, ProviderBinding, ProviderInstance, ProviderManifest, ProviderReadiness,
};

#[derive(Clone)]
pub(crate) struct IntegrationCatalogAuthority {
    repository: IntegrationCatalogRepository,
    key_store: Arc<dyn KeyStore>,
}

impl IntegrationCatalogAuthority {
    pub(crate) fn new(
        repository: IntegrationCatalogRepository,
        key_store: Arc<dyn KeyStore>,
    ) -> Self {
        Self {
            repository,
            key_store,
        }
    }

    pub(crate) fn sqlite(
        pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
        key_store: Arc<dyn KeyStore>,
    ) -> Self {
        Self::new(IntegrationCatalogRepository::new(pool), key_store)
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool, key_store: Arc<dyn KeyStore>) -> Self {
        Self::new(IntegrationCatalogRepository::postgres(pool), key_store)
    }

    pub(crate) fn save_manifest(
        &self,
        manifest: &ProviderManifest,
    ) -> Result<(), IntegrationError> {
        manifest.validate()?;
        self.repository.save_manifest(manifest)
    }

    pub(crate) fn save_instance(
        &self,
        instance: &ProviderInstance,
    ) -> Result<(), IntegrationError> {
        instance.validate()?;
        let catalog = self.repository.catalog()?;
        let manifest = catalog
            .manifest(&instance.provider_id, &instance.manifest_version)
            .ok_or(IntegrationError::BindingUnavailable)?;
        validate_instance_against_manifest(instance, &manifest)?;

        let secret_environment = match instance.readiness {
            ProviderReadiness::Fixture => SecretEnvironment::Fixture,
            ProviderReadiness::Stub
            | ProviderReadiness::Sandbox
            | ProviderReadiness::Production => SecretEnvironment::Deployment,
        };
        let secret_registration =
            SecretRegistrationScope::new(instance.tenant_id.clone(), instance.id.clone())?;
        if instance.secret_refs.values().any(|secret_ref| {
            !self
                .key_store
                .accepts_reference(secret_ref, &secret_registration, secret_environment)
        }) {
            return Err(IntegrationError::SecretUnavailable);
        }

        self.repository.save_instance(instance)
    }

    pub(crate) fn save_binding(
        &self,
        binding: &ProviderBinding,
        actor_ref: &str,
    ) -> Result<(), IntegrationError> {
        binding.validate()?;
        self.repository.save_binding(binding, actor_ref)
    }

    pub(crate) fn catalog(&self) -> Result<ProviderCatalog, IntegrationError> {
        self.repository.catalog()
    }

    pub(crate) fn list_manifests(&self) -> Result<Vec<ProviderManifest>, IntegrationError> {
        Ok(self.catalog()?.manifests())
    }

    pub(crate) fn list_instances(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<ProviderInstance>, IntegrationError> {
        Ok(self.catalog()?.instances_for_tenant(tenant_id))
    }

    pub(crate) fn list_bindings(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<ProviderBinding>, IntegrationError> {
        Ok(self.catalog()?.bindings_for_tenant(tenant_id))
    }
}

fn validate_instance_against_manifest(
    instance: &ProviderInstance,
    manifest: &ProviderManifest,
) -> Result<(), IntegrationError> {
    let declared_config = manifest
        .config_schema
        .iter()
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();
    let declared_secrets = manifest
        .secret_schema
        .iter()
        .map(|requirement| requirement.name.as_str())
        .collect::<BTreeSet<_>>();

    if instance
        .config
        .keys()
        .any(|field| !declared_config.contains(field.as_str()))
    {
        return Err(IntegrationError::InvalidManifest(
            "instance configuration contains a field not declared by the manifest".into(),
        ));
    }
    if instance.config.iter().any(|(name, value)| {
        manifest
            .config_schema
            .iter()
            .find(|field| field.name == *name)
            .is_none_or(|field| !field.value_type.accepts(value))
    }) {
        return Err(IntegrationError::InvalidManifest(
            "instance configuration does not match the manifest's non-secret value type".into(),
        ));
    }
    if manifest
        .config_schema
        .iter()
        .any(|field| field.required && !instance.config.contains_key(&field.name))
    {
        return Err(IntegrationError::InvalidManifest(
            "required instance configuration is missing".into(),
        ));
    }
    if instance
        .secret_refs
        .keys()
        .any(|field| !declared_secrets.contains(field.as_str()))
    {
        return Err(IntegrationError::InvalidManifest(
            "instance secret reference is not declared by the manifest".into(),
        ));
    }
    if manifest.secret_schema.iter().any(|requirement| {
        requirement.required && !instance.secret_refs.contains_key(&requirement.name)
    }) {
        return Err(IntegrationError::RequiredSecretMissing);
    }

    Ok(())
}
