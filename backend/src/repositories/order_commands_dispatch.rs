use crate::repositories::order_commands::{
    DraftOrderPatch, ImportedOrderDraft, ImportedOrderProjection,
    ScopedOrderCommandRepository as SqliteOrderCommandRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::order_commands_postgres::PostgresOrderCommandRepository;

pub struct ScopedOrderCommandRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedOrderCommandRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn create_imported_draft(
        &self,
        draft: ImportedOrderDraft,
    ) -> Result<ImportedOrderProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOrderCommandRepository::new(self.scoped.session())
                .create_imported_draft(draft);
        }

        SqliteOrderCommandRepository::new(self.scoped).create_imported_draft(draft)
    }

    pub fn update_draft(
        &self,
        order_id: &str,
        expected_version: i64,
        patch: DraftOrderPatch,
    ) -> Result<(), RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresOrderCommandRepository::new(self.scoped.session()).update_draft(
                order_id,
                expected_version,
                patch,
            );
        }

        SqliteOrderCommandRepository::new(self.scoped).update_draft(
            order_id,
            expected_version,
            patch,
        )
    }
}
