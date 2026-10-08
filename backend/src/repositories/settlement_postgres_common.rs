#![cfg(feature = "postgres")]

use crate::repositories::RepositoryError;

pub(in crate::repositories) fn pg_error(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}
