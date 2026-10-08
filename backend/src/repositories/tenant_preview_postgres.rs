#![cfg(feature = "postgres")]

use std::future::Future;

use sqlx::{Executor, PgPool, Row};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::RepositoryError;
use super::tenant_preview::{
    PreviewDashboardProjection, PreviewMutationError, PreviewSessionCreate,
    PreviewSessionProjection,
};

#[derive(Clone)]
pub(crate) struct PostgresTenantPreviewRepository {
    pool: PgPool,
}

impl PostgresTenantPreviewRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn create_session(
        &self,
        command: PreviewSessionCreate,
    ) -> Result<PreviewSessionProjection, PreviewMutationError> {
        let pool = self.pool.clone();
        run_pg_preview_mutation(async move {
            let mut transaction = pool.begin().await.map_err(pg_mutation)?;
            transaction
                .execute("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .await
                .map_err(pg_mutation)?;

            let tenant = sqlx::query(
                "SELECT name,slug FROM tenants
                 WHERE id=$1 AND status='active'
                 FOR SHARE",
            )
            .bind(&command.target_tenant_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(pg_mutation)?
            .ok_or(PreviewMutationError::TenantNotFound)?;
            let tenant_name = tenant.try_get::<String, _>("name").map_err(pg_mutation)?;
            let tenant_slug = tenant.try_get::<String, _>("slug").map_err(pg_mutation)?;

            if command.mode == "simulation" {
                let simulation_id = command
                    .simulation_id
                    .as_deref()
                    .ok_or(PreviewMutationError::SimulationNotFound)?;
                let exists = sqlx::query(
                    "SELECT 1 FROM simulation_sessions
                     WHERE id=$1 AND actor_id=$2 AND target_tenant_id=$3
                       AND status='active'
                     FOR SHARE",
                )
                .bind(simulation_id)
                .bind(&command.actor_id)
                .bind(&command.target_tenant_id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(pg_mutation)?
                .is_some();
                if !exists {
                    return Err(PreviewMutationError::SimulationNotFound);
                }
            }

            let capabilities_json =
                serde_json::to_string(&command.capabilities).map_err(|error| {
                    PreviewMutationError::Storage(RepositoryError::ContractViolation(format!(
                        "preview capabilities serialization failed: {error}"
                    )))
                })?;

            sqlx::query(
                "INSERT INTO tenant_preview_sessions
                 (id,actor_id,target_tenant_id,status,created_at,expires_at,ended_at)
                 VALUES ($1,$2,$3,'active',$4::timestamptz,$5::timestamptz,NULL)",
            )
            .bind(&command.id)
            .bind(&command.actor_id)
            .bind(&command.target_tenant_id)
            .bind(&command.created_at)
            .bind(&command.expires_at)
            .execute(&mut *transaction)
            .await
            .map_err(pg_mutation)?;

            sqlx::query(
                "INSERT INTO tenant_workspace_sessions
                 (id,actor_identity_id,tenant_id,mode,preview_session_id,simulation_id,
                  capabilities_json,status,created_at,expires_at,ended_at)
                 VALUES ($1,$2,$3,$4,$1,$5,$6::jsonb,'active',$7,$8,NULL)",
            )
            .bind(&command.id)
            .bind(&command.actor_id)
            .bind(&command.target_tenant_id)
            .bind(&command.mode)
            .bind(&command.simulation_id)
            .bind(&capabilities_json)
            .bind(&command.created_at)
            .bind(&command.expires_at)
            .execute(&mut *transaction)
            .await
            .map_err(pg_mutation)?;

            transaction.commit().await.map_err(pg_mutation)?;

            Ok(PreviewSessionProjection {
                id: command.id,
                actor_id: command.actor_id,
                target_tenant_id: command.target_tenant_id,
                tenant_name,
                tenant_slug,
                status: "active".into(),
                created_at: command.created_at,
                expires_at: command.expires_at,
                ended_at: None,
                mode: command.mode,
                simulation_id: command.simulation_id,
                capabilities: command.capabilities,
            })
        })
    }

    pub(crate) fn load_owned(
        &self,
        id: &str,
        actor_id: &str,
    ) -> Result<Option<PreviewSessionProjection>, RepositoryError> {
        let pool = self.pool.clone();
        let id = id.to_owned();
        let actor_id = actor_id.to_owned();
        run_pg_preview(async move {
            sqlx::query(
                "SELECT s.id,s.actor_id,s.target_tenant_id,t.name,t.slug,s.status,
                        w.created_at,w.expires_at,w.ended_at,
                        w.mode,w.simulation_id,w.capabilities_json::text AS capabilities_json
                 FROM tenant_preview_sessions s
                 JOIN tenants t ON t.id=s.target_tenant_id
                 JOIN tenant_workspace_sessions w ON w.preview_session_id=s.id
                 WHERE s.id=$1 AND s.actor_id=$2",
            )
            .bind(id)
            .bind(actor_id)
            .fetch_optional(&pool)
            .await
            .map_err(pg_storage)?
            .map(map_session)
            .transpose()
        })
    }

    pub(crate) fn end_session(
        &self,
        id: &str,
        actor_id: &str,
        ended_at: &str,
    ) -> Result<PreviewSessionProjection, PreviewMutationError> {
        let pool = self.pool.clone();
        let id = id.to_owned();
        let actor_id = actor_id.to_owned();
        let reload_id = id.clone();
        let reload_actor_id = actor_id.clone();
        let ended_at_text = ended_at.to_owned();

        run_pg_preview_mutation(async move {
            let mut transaction = pool.begin().await.map_err(pg_mutation)?;
            transaction
                .execute("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .await
                .map_err(pg_mutation)?;

            let owned = sqlx::query(
                "SELECT 1 FROM tenant_preview_sessions
                 WHERE id=$1 AND actor_id=$2
                 FOR UPDATE",
            )
            .bind(&id)
            .bind(&actor_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(pg_mutation)?
            .is_some();
            if !owned {
                return Err(PreviewMutationError::NotFound);
            }

            sqlx::query(
                "UPDATE tenant_preview_sessions
                 SET status='ended',ended_at=COALESCE(ended_at,$3::timestamptz)
                 WHERE id=$1 AND actor_id=$2 AND status='active'",
            )
            .bind(&id)
            .bind(&actor_id)
            .bind(&ended_at_text)
            .execute(&mut *transaction)
            .await
            .map_err(pg_mutation)?;

            sqlx::query(
                "UPDATE tenant_workspace_sessions
                 SET status='ended',ended_at=COALESCE(ended_at,$3)
                 WHERE id=$1 AND actor_identity_id=$2 AND status='active'",
            )
            .bind(&id)
            .bind(&actor_id)
            .bind(&ended_at_text)
            .execute(&mut *transaction)
            .await
            .map_err(pg_mutation)?;

            transaction.commit().await.map_err(pg_mutation)?;
            Ok(())
        })?;

        self.load_owned(reload_id.as_str(), reload_actor_id.as_str())
            .map_err(PreviewMutationError::Storage)?
            .ok_or(PreviewMutationError::NotFound)
    }

    pub(crate) fn tenant_is_active(&self, tenant_id: &str) -> Result<bool, RepositoryError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        run_pg_preview(async move {
            sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM tenants WHERE id=$1 AND status='active')",
            )
            .bind(tenant_id)
            .fetch_one(&pool)
            .await
            .map_err(pg_storage)
        })
    }

    pub(crate) fn dashboard_summary(
        &self,
        tenant_id: &str,
    ) -> Result<PreviewDashboardProjection, RepositoryError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        run_pg_preview(async move {
            let row = sqlx::query(
                "SELECT
                   (SELECT COUNT(*)::bigint FROM orders WHERE tenant_id=$1) AS orders,
                   (SELECT COUNT(*)::bigint FROM orders
                    WHERE tenant_id=$1 AND status NOT IN ('completed','closed','cancelled'))
                      AS active_orders,
                   (SELECT COUNT(*)::bigint FROM devices WHERE tenant_id=$1) AS devices,
                   (SELECT COUNT(*)::bigint FROM devices
                    WHERE tenant_id=$1 AND rentalstatus IN ('available','idle','已入库'))
                      AS available_devices",
            )
            .bind(tenant_id)
            .fetch_one(&pool)
            .await
            .map_err(pg_storage)?;

            Ok(PreviewDashboardProjection {
                orders: row.try_get("orders").map_err(pg_storage)?,
                active_orders: row.try_get("active_orders").map_err(pg_storage)?,
                devices: row.try_get("devices").map_err(pg_storage)?,
                available_devices: row.try_get("available_devices").map_err(pg_storage)?,
            })
        })
    }
}

fn map_session(row: sqlx::postgres::PgRow) -> Result<PreviewSessionProjection, RepositoryError> {
    let created_at: String = row.try_get("created_at").map_err(pg_storage)?;
    let expires_at: String = row.try_get("expires_at").map_err(pg_storage)?;
    let ended_at: Option<String> = row.try_get("ended_at").map_err(pg_storage)?;
    let capabilities_json: String = row.try_get("capabilities_json").map_err(pg_storage)?;
    Ok(PreviewSessionProjection {
        id: row.try_get("id").map_err(pg_storage)?,
        actor_id: row.try_get("actor_id").map_err(pg_storage)?,
        target_tenant_id: row.try_get("target_tenant_id").map_err(pg_storage)?,
        tenant_name: row.try_get("name").map_err(pg_storage)?,
        tenant_slug: row.try_get("slug").map_err(pg_storage)?,
        status: row.try_get("status").map_err(pg_storage)?,
        created_at,
        expires_at,
        ended_at,
        mode: row.try_get("mode").map_err(pg_storage)?,
        simulation_id: row.try_get("simulation_id").map_err(pg_storage)?,
        capabilities: serde_json::from_str(&capabilities_json).unwrap_or_default(),
    })
}

fn pg_storage(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

fn pg_mutation(error: sqlx::Error) -> PreviewMutationError {
    PreviewMutationError::Storage(pg_storage(error))
}

fn run_pg_preview<T, F>(future: F) -> Result<T, RepositoryError>
where
    F: Future<Output = Result<T, RepositoryError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        RepositoryError::AdapterUnavailable("tenant preview runtime unavailable".into())
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(RepositoryError::AdapterUnavailable(
            "tenant preview requires the multi-thread runtime".into(),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

fn run_pg_preview_mutation<T, F>(future: F) -> Result<T, PreviewMutationError>
where
    F: Future<Output = Result<T, PreviewMutationError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        PreviewMutationError::Storage(RepositoryError::AdapterUnavailable(
            "tenant preview runtime unavailable".into(),
        ))
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(PreviewMutationError::Storage(
            RepositoryError::AdapterUnavailable(
                "tenant preview requires the multi-thread runtime".into(),
            ),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}
