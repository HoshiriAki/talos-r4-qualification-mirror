use std::future::Future;
use std::pin::Pin;

use sqlx::postgres::{PgConnection, PgPool};
use sqlx::{Acquire, Executor};
use tokio::runtime::{Handle, RuntimeFlavor};

use crate::repositories::{RepositoryAccess, RepositoryBinding, RepositoryError};

pub(in crate::repositories) struct PostgresRepositorySession {
    binding: RepositoryBinding,
    pool: PgPool,
}

impl PostgresRepositorySession {
    pub(in crate::repositories) fn new(binding: RepositoryBinding, pool: PgPool) -> Self {
        Self { binding, pool }
    }

    pub(in crate::repositories) fn binding(&self) -> &RepositoryBinding {
        &self.binding
    }

    pub(in crate::repositories) fn read<T, F>(&self, operation: F) -> Result<T, RepositoryError>
    where
        T: Send,
        F: for<'connection> FnOnce(
                &'connection mut PgConnection,
            ) -> Pin<
                Box<dyn Future<Output = Result<T, sqlx::Error>> + Send + 'connection>,
            > + Send,
    {
        let pool = self.pool.clone();
        let access = self.binding.access();
        run_pg_compat(async move {
            let mut transaction = pool
                .begin()
                .await
                .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
            sqlx::query("SET TRANSACTION READ ONLY")
                .execute(&mut *transaction)
                .await
                .map_err(|error| RepositoryError::Postgres(error.to_string()))?;

            let result = operation(&mut *transaction).await;
            match result {
                Ok(value) => {
                    transaction
                        .commit()
                        .await
                        .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
                    Ok(value)
                }
                Err(error) => {
                    let _ = transaction.rollback().await;
                    Err(map_pg_read_error(access, error))
                }
            }
        })
    }

    pub(in crate::repositories) fn write<T, F>(&self, operation: F) -> Result<T, RepositoryError>
    where
        T: Send,
        F: for<'connection> FnOnce(
                &'connection mut PgConnection,
            ) -> Pin<
                Box<dyn Future<Output = Result<T, sqlx::Error>> + Send + 'connection>,
            > + Send,
    {
        if self.binding.access() != RepositoryAccess::ReadWrite {
            return Err(RepositoryError::PreviewWriteDenied);
        }

        let pool = self.pool.clone();
        run_pg_compat(async move {
            let mut transaction = pool
                .begin()
                .await
                .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
            let result = operation(&mut *transaction).await;
            match result {
                Ok(value) => {
                    transaction
                        .commit()
                        .await
                        .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
                    Ok(value)
                }
                Err(error) => {
                    let _ = transaction.rollback().await;
                    Err(RepositoryError::Postgres(error.to_string()))
                }
            }
        })
    }

    pub(in crate::repositories) fn write_serializable<T, F>(
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
        if self.binding.access() != RepositoryAccess::ReadWrite {
            return Err(RepositoryError::PreviewWriteDenied);
        }

        let pool = self.pool.clone();
        run_pg_compat(async move {
            let mut transaction = pool
                .begin()
                .await
                .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *transaction)
                .await
                .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
            let result = operation(&mut *transaction).await;
            match result {
                Ok(value) => {
                    transaction
                        .commit()
                        .await
                        .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
                    Ok(value)
                }
                Err(error) => {
                    let _ = transaction.rollback().await;
                    Err(RepositoryError::Postgres(error.to_string()))
                }
            }
        })
    }

    pub(in crate::repositories) fn write_serializable_repository<T, F>(
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
        if self.binding.access() != RepositoryAccess::ReadWrite {
            return Err(RepositoryError::PreviewWriteDenied);
        }

        let pool = self.pool.clone();
        run_pg_compat(async move {
            let mut transaction = pool
                .begin()
                .await
                .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *transaction)
                .await
                .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
            let result = operation(&mut *transaction).await;
            match result {
                Ok(value) => {
                    transaction
                        .commit()
                        .await
                        .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
                    Ok(value)
                }
                Err(error) => {
                    let _ = transaction.rollback().await;
                    Err(error)
                }
            }
        })
    }
}

fn map_pg_read_error(access: RepositoryAccess, error: sqlx::Error) -> RepositoryError {
    let is_read_only = matches!(
        &error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("25006")
    );
    if is_read_only {
        if access == RepositoryAccess::ReadOnly {
            RepositoryError::PreviewWriteDenied
        } else {
            RepositoryError::ContractViolation(
                "repository read capability attempted a PostgreSQL write".into(),
            )
        }
    } else {
        RepositoryError::Postgres(error.to_string())
    }
}

fn run_pg_compat<T, F>(future: F) -> Result<T, RepositoryError>
where
    T: Send,
    F: Future<Output = Result<T, RepositoryError>> + Send,
{
    let handle = Handle::try_current().map_err(|_| {
        RepositoryError::PoolUnavailable(
            "PostgreSQL synchronous repository bridge requires an active Tokio runtime".into(),
        )
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(RepositoryError::ContractViolation(
            "PostgreSQL synchronous repository bridge requires the multi-thread Tokio runtime"
                .into(),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}
