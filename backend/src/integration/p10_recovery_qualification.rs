#![cfg(all(test, feature = "postgres"))]

use sqlx::postgres::PgPoolOptions;

use super::operation_runtime_contract::{OperationRuntimePersistence, OperationRuntimePolicy};
use super::operation_runtime_postgres::PostgresOperationRuntimePersistence;
use super::types::ExternalOperationId;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_P10_RECOVERY_DATABASE_URL and a controlled unknown-outcome fixture"]
async fn p10_unknown_outcome_requires_reconciliation_before_retry() -> anyhow::Result<()> {
    let database_url = std::env::var("TALOS_P10_RECOVERY_DATABASE_URL")?;
    let tenant_id = std::env::var("TALOS_P10_RECOVERY_TENANT_ID")?;
    let operation_id = ExternalOperationId::new(std::env::var("TALOS_P10_RECOVERY_OPERATION_ID")?)?;
    let evidence_ref = std::env::var("TALOS_P10_RECOVERY_EVIDENCE_REF")
        .unwrap_or_else(|_| "p10-sp05-provider-query-confirmed-effect".to_string());
    let actor_ref = std::env::var("TALOS_P10_RECOVERY_ACTOR_REF")
        .unwrap_or_else(|_| "p10-sp05-recovery-operator".to_string());

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await?;
    let persistence = PostgresOperationRuntimePersistence::new(pool.clone());

    let before: (String, i64, Option<String>) = sqlx::query_as(
        "SELECT state,attempt_count,classification
         FROM external_operations
         WHERE tenant_id=$1 AND id=$2",
    )
    .bind(&tenant_id)
    .bind(operation_id.as_str())
    .fetch_one(&pool)
    .await?;

    anyhow::ensure!(
        before.0 == "unknown_outcome",
        "fixture must be unknown_outcome"
    );
    anyhow::ensure!(
        before.1 == 1,
        "fixture must have exactly one dispatched attempt"
    );
    anyhow::ensure!(
        before.2.as_deref() == Some("worker_restarted_after_dispatch"),
        "fixture must carry restart-after-dispatch classification"
    );

    let claim = persistence.claim_next_operation(&tenant_id, &OperationRuntimePolicy::default())?;
    anyhow::ensure!(
        claim.is_none(),
        "unknown outcome must not be claimable for blind redispatch"
    );

    let attempt_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint
         FROM external_operation_attempts
         WHERE tenant_id=$1 AND operation_id=$2",
    )
    .bind(&tenant_id)
    .bind(operation_id.as_str())
    .fetch_one(&pool)
    .await?;
    anyhow::ensure!(attempt_before == 1, "blind retry created a second attempt");

    persistence.begin_reconciliation(
        &tenant_id,
        &operation_id,
        &evidence_ref,
    )?;

    let reconciling: String =
        sqlx::query_scalar("SELECT state FROM external_operations WHERE tenant_id=$1 AND id=$2")
            .bind(&tenant_id)
            .bind(operation_id.as_str())
            .fetch_one(&pool)
            .await?;
    anyhow::ensure!(reconciling == "reconciling");

    let pending: (String, String) = sqlx::query_as(
        "SELECT outcome,evidence_ref
         FROM external_operation_reconciliations
         WHERE tenant_id=$1 AND operation_id=$2
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind(&tenant_id)
    .bind(operation_id.as_str())
    .fetch_one(&pool)
    .await?;
    anyhow::ensure!(pending.0 == "pending");
    anyhow::ensure!(pending.1 == evidence_ref);

    persistence.resolve_reconciliation(
        &tenant_id,
        &operation_id,
        true,
        &actor_ref,
    )?;

    let resolved: (String, i64) = sqlx::query_as(
        "SELECT state,attempt_count
         FROM external_operations
         WHERE tenant_id=$1 AND id=$2",
    )
    .bind(&tenant_id)
    .bind(operation_id.as_str())
    .fetch_one(&pool)
    .await?;
    anyhow::ensure!(resolved.0 == "resolved");
    anyhow::ensure!(
        resolved.1 == 1,
        "reconciliation must not create a retry attempt"
    );

    let reconciliation: (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT outcome,resolved_by,resolved_at
         FROM external_operation_reconciliations
         WHERE tenant_id=$1 AND operation_id=$2
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind(&tenant_id)
    .bind(operation_id.as_str())
    .fetch_one(&pool)
    .await?;
    anyhow::ensure!(reconciliation.0 == "effect_confirmed");
    anyhow::ensure!(reconciliation.1.as_deref() == Some(actor_ref.as_str()));
    anyhow::ensure!(reconciliation.2.is_some());

    println!(
        "P10_EFFECT_RECONCILIATION operation={} before=unknown_outcome after=resolved attempts=1 effect_confirmed=true blind_retry=false compensation=not_applicable_nonfinancial_fixture",
        operation_id.as_str()
    );

    pool.close().await;
    Ok(())
}
