use crate::repositories::model::{
    ModelMutationError, ModelPatch, ModelProjection, NewModel, SqliteModelRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::model_postgres::PostgresModelRepository;

pub struct ScopedModelRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedModelRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn list(&self) -> Result<Vec<ModelProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresModelRepository::new(self.scoped.session()).list();
        }
        SqliteModelRepository::new(self.scoped).list()
    }

    pub fn get(&self, id: &str) -> Result<Option<ModelProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresModelRepository::new(self.scoped.session()).get(id);
        }
        SqliteModelRepository::new(self.scoped).get(id)
    }

    pub fn find_by_name(&self, name: &str) -> Result<Option<ModelProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresModelRepository::new(self.scoped.session()).find_by_name(name);
        }
        SqliteModelRepository::new(self.scoped).find_by_name(name)
    }

    pub fn create(&self, input: &NewModel) -> Result<ModelProjection, ModelMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresModelRepository::new(self.scoped.session()).create(input);
        }
        SqliteModelRepository::new(self.scoped).create(input)
    }

    pub fn update(
        &self,
        id: &str,
        patch: &ModelPatch,
    ) -> Result<ModelProjection, ModelMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresModelRepository::new(self.scoped.session()).update(id, patch);
        }
        SqliteModelRepository::new(self.scoped).update(id, patch)
    }

    pub fn delete(&self, id: &str) -> Result<(), ModelMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresModelRepository::new(self.scoped.session()).delete(id);
        }
        SqliteModelRepository::new(self.scoped).delete(id)
    }
}
