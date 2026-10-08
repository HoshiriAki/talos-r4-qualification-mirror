use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, ErrorCode, Transaction, TransactionBehavior};

use crate::repositories::{RepositoryAccess, RepositoryBinding, RepositoryError};

pub(in crate::repositories) struct SqliteRepositorySession {
    binding: RepositoryBinding,
    pool: Pool<SqliteConnectionManager>,
}

struct QueryOnlyGuard<'a> {
    connection: &'a Connection,
    previous: i64,
    restored: bool,
}

impl<'a> QueryOnlyGuard<'a> {
    fn enable(connection: &'a Connection) -> Result<Self, rusqlite::Error> {
        let previous =
            connection.pragma_query_value(None, "query_only", |row| row.get::<_, i64>(0))?;
        connection.pragma_update(None, "query_only", 1_i64)?;
        Ok(Self {
            connection,
            previous,
            restored: false,
        })
    }

    fn restore(&mut self) -> Result<(), rusqlite::Error> {
        if !self.restored {
            self.connection
                .pragma_update(None, "query_only", self.previous)?;
            self.restored = true;
        }
        Ok(())
    }
}

impl Drop for QueryOnlyGuard<'_> {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self
                .connection
                .pragma_update(None, "query_only", self.previous);
        }
    }
}

impl SqliteRepositorySession {
    pub(in crate::repositories) fn new(
        binding: RepositoryBinding,
        pool: Pool<SqliteConnectionManager>,
    ) -> Self {
        Self { binding, pool }
    }

    pub(in crate::repositories) fn binding(&self) -> &RepositoryBinding {
        &self.binding
    }

    pub(in crate::repositories) fn read<T, F>(&self, operation: F) -> Result<T, RepositoryError>
    where
        F: FnOnce(&Connection) -> Result<T, rusqlite::Error>,
    {
        let connection = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let mut query_only = QueryOnlyGuard::enable(&connection)
            .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
        let result = operation(&connection);
        query_only
            .restore()
            .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
        result.map_err(|error| self.map_read_error(error))
    }

    pub(in crate::repositories) fn write<T, F>(&self, operation: F) -> Result<T, RepositoryError>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T, rusqlite::Error>,
    {
        if self.binding.access() != RepositoryAccess::ReadWrite {
            return Err(RepositoryError::PreviewWriteDenied);
        }
        let mut connection = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let transaction = connection
            .transaction()
            .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
        let result =
            operation(&transaction).map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
        transaction
            .commit()
            .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
        Ok(result)
    }

    pub(in crate::repositories) fn write_immediate<T, F>(
        &self,
        operation: F,
    ) -> Result<T, RepositoryError>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T, RepositoryError>,
    {
        if self.binding.access() != RepositoryAccess::ReadWrite {
            return Err(RepositoryError::PreviewWriteDenied);
        }
        let mut connection = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
        let result = operation(&transaction)?;
        transaction
            .commit()
            .map_err(|error| RepositoryError::Sqlite(error.to_string()))?;
        Ok(result)
    }

    fn map_read_error(&self, error: rusqlite::Error) -> RepositoryError {
        if is_read_only_error(&error) {
            if self.binding.access() == RepositoryAccess::ReadOnly {
                RepositoryError::PreviewWriteDenied
            } else {
                RepositoryError::ContractViolation(
                    "repository read capability attempted a write".into(),
                )
            }
        } else {
            RepositoryError::Sqlite(error.to_string())
        }
    }
}

fn is_read_only_error(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, _) if failure.code == ErrorCode::ReadOnly
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use rusqlite::params;
    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, Namespace,
        NoopHttpClient, PlatformMembershipId, PlatformRole, PreviewSessionId, RepositoryScope,
        RequestId, Revision, SimulationId, TenantId, TenantScope,
    };

    use crate::repositories::{
        RepositoryAccess, RepositoryError, RepositoryProvider, ScopedRepositories,
        SqliteRepositoryProvider,
    };

    struct FixtureRepository<'a> {
        scoped: &'a ScopedRepositories,
    }

    impl<'a> FixtureRepository<'a> {
        fn new(scoped: &'a ScopedRepositories) -> Self {
            Self { scoped }
        }

        fn create_schema(&self) -> Result<(), RepositoryError> {
            self.scoped.session().write(|transaction| {
                transaction.execute_batch(
                    "CREATE TABLE IF NOT EXISTS repository_scope_fixture (\
                     tenant_id TEXT NOT NULL, value TEXT NOT NULL)",
                )
            })
        }

        fn insert_value(&self, value: &str) -> Result<(), RepositoryError> {
            self.scoped.session().write(|transaction| {
                transaction.execute(
                    "INSERT INTO repository_scope_fixture (tenant_id, value) VALUES (?1, ?2)",
                    params![self.scoped.binding().tenant_id().as_str(), value],
                )?;
                Ok(())
            })
        }

        fn mutate_through_read(&self, value: &str) -> Result<(), RepositoryError> {
            self.scoped.session().read(|connection| {
                connection.execute(
                    "INSERT INTO repository_scope_fixture (tenant_id, value) VALUES (?1, ?2)",
                    params![self.scoped.binding().tenant_id().as_str(), value],
                )?;
                Ok(())
            })
        }

        fn list_values(&self) -> Result<Vec<String>, RepositoryError> {
            self.scoped.session().read(|connection| {
                let mut statement = connection.prepare(
                    "SELECT value FROM repository_scope_fixture \
                     WHERE tenant_id = ?1 ORDER BY value",
                )?;
                let rows = statement
                    .query_map(params![self.scoped.binding().tenant_id().as_str()], |row| {
                        row.get(0)
                    })?;
                rows.collect()
            })
        }
    }

    fn pool() -> Pool<SqliteConnectionManager> {
        Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap()
    }

    fn normal_context(tenant: &str, request: &str, revision: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("tenant-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new(revision).unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn platform_actor() -> ActorIdentity {
        ActorIdentity::with_authority(
            "platform-actor",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap()
    }

    fn preview_context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            platform_actor(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("preview-revision").unwrap()).unwrap(),
            ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("preview-session").unwrap()),
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn simulation_context(tenant: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        let simulation_id = SimulationId::new("simulation-session").unwrap();
        ExecutionContext::new(
            platform_actor(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::new(
                tenant_id,
                Namespace::Simulation(simulation_id.clone()),
                Revision::new("simulation-base").unwrap(),
            )
            .unwrap(),
            ExecutionMode::Simulation(simulation_id),
            RequestId::new("simulation-request").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn platform_context() -> ExecutionContext {
        ExecutionContext::new(
            ActorIdentity::system(),
            TenantScope::platform(),
            DataScope::platform(Revision::new("platform-revision").unwrap()),
            ExecutionMode::Normal,
            RequestId::new("platform-request").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn binding_derives_scope_mode_revision_and_correlation_from_execution() {
        let provider = SqliteRepositoryProvider::new(pool());
        let normal = provider
            .bind(&normal_context(
                "tenant-a",
                "normal-request",
                "normal-revision",
            ))
            .unwrap();
        assert_eq!(normal.binding().tenant_id().as_str(), "tenant-a");
        assert_eq!(normal.binding().namespace(), &Namespace::Production);
        assert_eq!(normal.binding().base_revision().as_str(), "normal-revision");
        assert_eq!(normal.binding().access(), RepositoryAccess::ReadWrite);
        assert_eq!(normal.binding().correlation_id().as_str(), "normal-request");
        assert_eq!(
            RepositoryScope::data_scope(normal.binding()),
            normal.binding().data_scope()
        );

        let preview = provider
            .bind(&preview_context("tenant-a", "preview-request"))
            .unwrap();
        assert_eq!(preview.binding().tenant_id().as_str(), "tenant-a");
        assert_eq!(preview.binding().access(), RepositoryAccess::ReadOnly);
        assert_eq!(
            preview.binding().correlation_id().as_str(),
            "preview-request"
        );
    }

    #[test]
    fn platform_and_simulation_bindings_fail_closed() {
        let provider = SqliteRepositoryProvider::new(pool());
        assert_eq!(
            provider.bind(&platform_context()).err().unwrap().code(),
            "REPOSITORY_TENANT_SCOPE_REQUIRED"
        );
        assert_eq!(
            provider
                .bind(&simulation_context("tenant-a"))
                .err()
                .unwrap()
                .code(),
            "REPOSITORY_SIMULATION_UNSUPPORTED"
        );
    }

    #[test]
    fn tenant_fixture_isolation_uses_only_the_bound_execution_scope() {
        let provider = SqliteRepositoryProvider::new(pool());
        let tenant_a = provider
            .bind(&normal_context("tenant-a", "request-a", "revision-a"))
            .unwrap();
        let tenant_b = provider
            .bind(&normal_context("tenant-b", "request-b", "revision-b"))
            .unwrap();
        let repository_a = FixtureRepository::new(&tenant_a);
        let repository_b = FixtureRepository::new(&tenant_b);
        repository_a.create_schema().unwrap();
        repository_a.insert_value("a-only").unwrap();
        repository_b.insert_value("b-only").unwrap();

        assert_eq!(repository_a.list_values().unwrap(), vec!["a-only"]);
        assert_eq!(repository_b.list_values().unwrap(), vec!["b-only"]);
        assert_ne!(
            tenant_a.binding().correlation_id(),
            tenant_b.binding().correlation_id()
        );
    }

    #[test]
    fn read_capability_is_database_read_only_and_restores_the_connection() {
        let provider = SqliteRepositoryProvider::new(pool());
        let normal = provider
            .bind(&normal_context("tenant-a", "normal", "production-revision"))
            .unwrap();
        let repository = FixtureRepository::new(&normal);
        repository.create_schema().unwrap();

        assert_eq!(
            repository
                .mutate_through_read("forbidden-read-write")
                .unwrap_err()
                .code(),
            "REPOSITORY_CONTRACT_VIOLATION"
        );
        assert!(repository.list_values().unwrap().is_empty());

        repository.insert_value("after-read-error").unwrap();
        assert_eq!(repository.list_values().unwrap(), vec!["after-read-error"]);
    }

    #[test]
    fn preview_reads_its_tenant_and_denies_all_write_paths() {
        let provider = SqliteRepositoryProvider::new(pool());
        let normal = provider
            .bind(&normal_context("tenant-a", "normal", "production-revision"))
            .unwrap();
        let normal_repository = FixtureRepository::new(&normal);
        normal_repository.create_schema().unwrap();
        normal_repository.insert_value("existing").unwrap();

        let preview = provider
            .bind(&preview_context("tenant-a", "preview"))
            .unwrap();
        let preview_repository = FixtureRepository::new(&preview);
        assert_eq!(preview_repository.list_values().unwrap(), vec!["existing"]);
        assert_eq!(
            preview_repository
                .insert_value("forbidden")
                .unwrap_err()
                .code(),
            "REPOSITORY_PREVIEW_WRITE_DENIED"
        );
        assert_eq!(
            preview_repository
                .mutate_through_read("forbidden-read-bypass")
                .unwrap_err()
                .code(),
            "REPOSITORY_PREVIEW_WRITE_DENIED"
        );
        assert_eq!(preview_repository.list_values().unwrap(), vec!["existing"]);
    }
}
