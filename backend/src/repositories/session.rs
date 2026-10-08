use rusqlite::{Connection, Transaction};

use crate::repositories::sqlite::SqliteSessionBackend;
use crate::repositories::{RepositoryBinding, RepositoryError};

#[cfg(feature = "postgres")]
use crate::repositories::postgres::PostgresRepositorySession;
#[cfg(feature = "postgres")]
use sqlx::postgres::PgConnection;
#[cfg(feature = "postgres")]
use std::future::Future;
#[cfg(feature = "postgres")]
use std::pin::Pin;

pub(in crate::repositories) enum RepositorySession {
    Sqlite(SqliteSessionBackend),
    #[cfg(feature = "postgres")]
    Postgres(PostgresRepositorySession),
}

impl RepositorySession {
    pub(in crate::repositories) fn sqlite(session: SqliteSessionBackend) -> Self {
        Self::Sqlite(session)
    }

    #[cfg(feature = "postgres")]
    pub(in crate::repositories) fn postgres(session: PostgresRepositorySession) -> Self {
        Self::Postgres(session)
    }

    pub(in crate::repositories) fn binding(&self) -> &RepositoryBinding {
        match self {
            Self::Sqlite(session) => session.binding(),
            #[cfg(feature = "postgres")]
            Self::Postgres(session) => session.binding(),
        }
    }

    pub(in crate::repositories) fn read<T, F>(&self, operation: F) -> Result<T, RepositoryError>
    where
        F: FnOnce(&Connection) -> Result<T, rusqlite::Error>,
    {
        match self {
            Self::Sqlite(session) => session.read(operation),
            #[cfg(feature = "postgres")]
            Self::Postgres(_) => Err(RepositoryError::AdapterUnavailable(
                "this repository family has not migrated its read adapter to PostgreSQL".into(),
            )),
        }
    }

    pub(in crate::repositories) fn write<T, F>(&self, operation: F) -> Result<T, RepositoryError>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T, rusqlite::Error>,
    {
        match self {
            Self::Sqlite(session) => session.write(operation),
            #[cfg(feature = "postgres")]
            Self::Postgres(_) => Err(RepositoryError::AdapterUnavailable(
                "this repository family has not migrated its write adapter to PostgreSQL".into(),
            )),
        }
    }

    pub(in crate::repositories) fn write_immediate<T, F>(
        &self,
        operation: F,
    ) -> Result<T, RepositoryError>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T, RepositoryError>,
    {
        match self {
            Self::Sqlite(session) => session.write_immediate(operation),
            #[cfg(feature = "postgres")]
            Self::Postgres(_) => Err(RepositoryError::AdapterUnavailable(
                "this repository family has not migrated its serialized write adapter to PostgreSQL"
                    .into(),
            )),
        }
    }

    #[cfg(feature = "postgres")]
    pub(in crate::repositories) fn pg_read<T, F>(&self, operation: F) -> Result<T, RepositoryError>
    where
        T: Send,
        F: for<'connection> FnOnce(
                &'connection mut PgConnection,
            ) -> Pin<
                Box<dyn Future<Output = Result<T, sqlx::Error>> + Send + 'connection>,
            > + Send,
    {
        self.postgres_backend()?.read(operation)
    }

    #[cfg(feature = "postgres")]
    pub(in crate::repositories) fn pg_write<T, F>(&self, operation: F) -> Result<T, RepositoryError>
    where
        T: Send,
        F: for<'connection> FnOnce(
                &'connection mut PgConnection,
            ) -> Pin<
                Box<dyn Future<Output = Result<T, sqlx::Error>> + Send + 'connection>,
            > + Send,
    {
        self.postgres_backend()?.write(operation)
    }

    #[cfg(feature = "postgres")]
    pub(in crate::repositories) fn pg_write_serializable<T, F>(
        &self,
        operation: F,
    ) -> Result<T, RepositoryError>
    where
        T: Send,
        F: for<'connection> FnOnce(
                &'connection mut PgConnection,
            ) -> Pin<
                Box<dyn Future<Output = Result<T, sqlx::Error>> + Send + 'connection>,
            > + Send,
    {
        self.postgres_backend()?.write_serializable(operation)
    }

    #[cfg(feature = "postgres")]
    pub(in crate::repositories) fn pg_write_serializable_repository<T, F>(
        &self,
        operation: F,
    ) -> Result<T, RepositoryError>
    where
        T: Send,
        F: for<'connection> FnOnce(
                &'connection mut PgConnection,
            ) -> Pin<
                Box<dyn Future<Output = Result<T, RepositoryError>> + Send + 'connection>,
            > + Send,
    {
        self.postgres_backend()?
            .write_serializable_repository(operation)
    }

    #[cfg(feature = "postgres")]
    fn postgres_backend(&self) -> Result<&PostgresRepositorySession, RepositoryError> {
        match self {
            Self::Postgres(session) => Ok(session),
            Self::Sqlite(_) => Err(RepositoryError::AdapterUnavailable(
                "PostgreSQL repository adapter requested from the SQLite local profile".into(),
            )),
        }
    }

    #[cfg(feature = "postgres")]
    pub(in crate::repositories) fn is_postgres(&self) -> bool {
        matches!(self, Self::Postgres(_))
    }
}
