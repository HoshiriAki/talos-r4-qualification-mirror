#![cfg(feature = "postgres")]

use uuid::Uuid;

use crate::repositories::RepositoryError;
use crate::repositories::session::RepositorySession;
use crate::repositories::tax::TaxUpsertOutcome;
use crate::repositories::tax_postgres_common::pg_error;

pub(in crate::repositories) fn upsert_config(
    session: &RepositorySession,
    tax_type: &str,
    rate: f64,
    effective_from: &str,
    now: &str,
) -> Result<TaxUpsertOutcome, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let tax_type = tax_type.to_owned();
    let effective_from = effective_from.to_owned();
    let now = now.to_owned();

    session.pg_write_serializable_repository(move |connection| {
        Box::pin(async move {
            let lock_key = format!("tax-config:{tenant_id}:{tax_type}");
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
                .bind(lock_key)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

            sqlx::query(
                "UPDATE tax_config
                 SET is_active=0
                 WHERE tenant_id=$1 AND tax_type=$2 AND is_active<>0",
            )
            .bind(&tenant_id)
            .bind(&tax_type)
            .execute(&mut *connection)
            .await
            .map_err(pg_error)?;

            let config_id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO tax_config
                 (id,tax_type,rate,effective_from,is_active,created_at,tenant_id)
                 VALUES ($1,$2,$3,$4,1,$5,$6)",
            )
            .bind(&config_id)
            .bind(&tax_type)
            .bind(rate)
            .bind(&effective_from)
            .bind(&now)
            .bind(&tenant_id)
            .execute(&mut *connection)
            .await
            .map_err(pg_error)?;

            Ok(TaxUpsertOutcome {
                config_id,
                tax_type,
                rate,
                effective_from,
            })
        })
    })
}
