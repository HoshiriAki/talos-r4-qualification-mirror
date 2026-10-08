use std::future::Future;

use sha2::{Digest, Sha256};
use sqlx::postgres::PgPool;
use sqlx::{Row, Transaction};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::types::{
    IntegrationError, ProviderBindingId, ProviderId, WebhookEndpointId, WebhookInboxId,
};
use super::webhook::{ClaimedWebhook, WebhookEndpointContext};
use super::webhook_persistence_contract::{
    WebhookAdminPersistence, WebhookDeadLetterSummary, WebhookEndpointSummary,
    WebhookReplayPersistence, WebhookRuntimePersistence,
};

#[derive(Clone)]
pub(crate) struct PostgresWebhookPersistence {
    pool: PgPool,
}

impl PostgresWebhookPersistence {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn finish_webhook_attempt(
        &self,
        webhook: &ClaimedWebhook,
        inbox_state: &str,
        canonical_event_type: Option<&str>,
        classification: Option<&str>,
        attempt_state: &str,
    ) -> Result<(), IntegrationError> {
        let pool = self.pool.clone();
        let webhook = webhook.clone();
        let inbox_state = inbox_state.to_owned();
        let canonical_event_type = canonical_event_type.map(str::to_owned);
        let classification = classification.map(str::to_owned);
        let attempt_state = attempt_state.to_owned();

        run_pg_webhook(async move {
            let mut transaction = serializable(&pool).await?;
            let timestamp = now();
            let changed = sqlx::query(
                "UPDATE webhook_inbox
                 SET status=$1,
                     canonical_event_type=COALESCE($2,canonical_event_type),
                     error_classification=$3
                 WHERE id=$4 AND tenant_id=$5 AND status='processing'",
            )
            .bind(&inbox_state)
            .bind(canonical_event_type.as_deref())
            .bind(classification.as_deref())
            .bind(webhook.inbox_id.as_str())
            .bind(&webhook.tenant_id)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?
            .rows_affected();
            if changed != 1 {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::InvalidOperationTransition);
            }

            sqlx::query(
                "UPDATE webhook_processing_attempts
                 SET state=$1,
                     canonical_event_type=$2,
                     classification=$3,
                     completed_at=$4
                 WHERE tenant_id=$5
                   AND inbox_id=$6
                   AND attempt_number=$7
                   AND state='processing'",
            )
            .bind(&attempt_state)
            .bind(canonical_event_type.as_deref())
            .bind(classification.as_deref())
            .bind(&timestamp)
            .bind(&webhook.tenant_id)
            .bind(webhook.inbox_id.as_str())
            .bind(i64::from(webhook.attempt_number))
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            transaction.commit().await.map_err(persistence)
        })
    }
}

impl WebhookReplayPersistence for PostgresWebhookPersistence {
    fn replay_webhook(
        &self,
        tenant_id: &str,
        inbox_id: &WebhookInboxId,
        actor_ref: &str,
        reason: &str,
    ) -> Result<(), IntegrationError> {
        if actor_ref.trim().is_empty() || reason.trim().is_empty() {
            return Err(IntegrationError::InvalidOperationTransition);
        }

        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let inbox_id = inbox_id.clone();
        let actor_ref = actor_ref.to_owned();
        let reason = reason.to_owned();

        run_pg_webhook(async move {
            let mut transaction = serializable(&pool).await?;
            let changed = sqlx::query(
                "UPDATE webhook_inbox
                 SET status='verified',error_classification=NULL
                 WHERE id=$1 AND tenant_id=$2 AND status='dead_letter'",
            )
            .bind(inbox_id.as_str())
            .bind(&tenant_id)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?
            .rows_affected();
            if changed != 1 {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::InvalidOperationTransition);
            }

            let timestamp = now();
            sqlx::query(
                "UPDATE webhook_dead_letters
                 SET replay_count=replay_count+1,replayed_at=$1
                 WHERE tenant_id=$2 AND inbox_id=$3",
            )
            .bind(&timestamp)
            .bind(&tenant_id)
            .bind(inbox_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            sqlx::query(
                "INSERT INTO webhook_replay_audit
                 (id,tenant_id,inbox_id,actor_ref,reason,occurred_at)
                 VALUES ($1,$2,$3,$4,$5,$6)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&tenant_id)
            .bind(inbox_id.as_str())
            .bind(&actor_ref)
            .bind(&reason)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            transaction.commit().await.map_err(persistence)
        })
    }
}

impl WebhookRuntimePersistence for PostgresWebhookPersistence {
    fn resolve_webhook_endpoint(
        &self,
        token: &[u8],
    ) -> Result<WebhookEndpointContext, IntegrationError> {
        if token.is_empty() {
            return Err(IntegrationError::WebhookUnverifiable);
        }

        let pool = self.pool.clone();
        let token_hash = hex::encode(Sha256::digest(token));

        run_pg_webhook(async move {
            let mut transaction = serializable_read_only(&pool).await?;
            let row = sqlx::query(
                "SELECT e.id,e.tenant_id,e.binding_id,e.provider_id,
                        e.enabled,b.enabled,i.lifecycle,i.health,i.readiness,m.readiness
                 FROM webhook_endpoints e
                 JOIN provider_bindings b
                   ON b.tenant_id=e.tenant_id AND b.id=e.binding_id
                 JOIN provider_instances i
                   ON i.tenant_id=b.tenant_id AND i.id=b.provider_instance_id
                 JOIN provider_manifests m
                   ON m.provider_id=i.provider_id AND m.version=i.manifest_version
                 WHERE e.token_hash=$1",
            )
            .bind(&token_hash)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence)?;

            let Some(row) = row else {
                transaction.commit().await.map_err(persistence)?;
                return Err(IntegrationError::BindingUnavailable);
            };

            let endpoint_enabled: bool = row.try_get(4).map_err(persistence)?;
            let binding_enabled: bool = row.try_get(5).map_err(persistence)?;
            let lifecycle: String = row.try_get(6).map_err(persistence)?;
            let health: String = row.try_get(7).map_err(persistence)?;
            let instance_readiness: String = row.try_get(8).map_err(persistence)?;
            let manifest_readiness: String = row.try_get(9).map_err(persistence)?;

            if !endpoint_enabled
                || !binding_enabled
                || lifecycle != "active"
                || health != "ready"
                || instance_readiness != "fixture"
                || manifest_readiness != "fixture"
            {
                transaction.commit().await.map_err(persistence)?;
                return Err(IntegrationError::BindingUnavailable);
            }

            let context = WebhookEndpointContext {
                endpoint_id: WebhookEndpointId::new(
                    row.try_get::<String, _>(0).map_err(persistence)?,
                )?,
                tenant_id: row.try_get(1).map_err(persistence)?,
                binding_id: ProviderBindingId::new(
                    row.try_get::<String, _>(2).map_err(persistence)?,
                )?,
                provider_id: ProviderId::new(row.try_get::<String, _>(3).map_err(persistence)?)?,
            };
            transaction.commit().await.map_err(persistence)?;
            Ok(context)
        })
    }

    fn record_verified_webhook(
        &self,
        endpoint: &WebhookEndpointContext,
        provider_event_id: &str,
        headers_json: &str,
        raw_payload: &[u8],
    ) -> Result<(WebhookInboxId, bool), IntegrationError> {
        if provider_event_id.trim().is_empty() || raw_payload.is_empty() {
            return Err(IntegrationError::WebhookUnverifiable);
        }

        let pool = self.pool.clone();
        let endpoint = endpoint.clone();
        let provider_event_id = provider_event_id.to_owned();
        let headers_json = headers_json.to_owned();
        let raw_payload = raw_payload.to_vec();
        let payload_hash = hex::encode(Sha256::digest(&raw_payload));

        run_pg_webhook(async move {
            let mut transaction = serializable(&pool).await?;
            let existing = find_webhook_identity(
                &mut transaction,
                &endpoint.tenant_id,
                &endpoint.endpoint_id,
                &provider_event_id,
            )
            .await?;

            if let Some((id, stored_hash, status)) = existing {
                if stored_hash != payload_hash {
                    let _ = transaction.rollback().await;
                    return Err(IntegrationError::WebhookUnverifiable);
                }
                if status == "rejected" {
                    sqlx::query(
                        "UPDATE webhook_inbox
                         SET headers_json=$1,
                             raw_payload=$2,
                             received_at=$3,
                             status='verified',
                             canonical_event_type=NULL,
                             error_classification=NULL
                         WHERE tenant_id=$4 AND endpoint_id=$5 AND provider_event_id=$6
                           AND status='rejected'",
                    )
                    .bind(&headers_json)
                    .bind(&raw_payload)
                    .bind(now())
                    .bind(&endpoint.tenant_id)
                    .bind(endpoint.endpoint_id.as_str())
                    .bind(&provider_event_id)
                    .execute(&mut *transaction)
                    .await
                    .map_err(persistence)?;
                }
                transaction.commit().await.map_err(persistence)?;
                return Ok((WebhookInboxId::new(id)?, true));
            }

            let inbox_id = WebhookInboxId::new(uuid::Uuid::new_v4().to_string())?;
            let insert = sqlx::query(
                "INSERT INTO webhook_inbox
                 (id,tenant_id,endpoint_id,provider_event_id,payload_hash,headers_json,
                  raw_payload,received_at,status)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'verified')
                 ON CONFLICT (tenant_id,endpoint_id,provider_event_id) DO NOTHING",
            )
            .bind(inbox_id.as_str())
            .bind(&endpoint.tenant_id)
            .bind(endpoint.endpoint_id.as_str())
            .bind(&provider_event_id)
            .bind(&payload_hash)
            .bind(&headers_json)
            .bind(&raw_payload)
            .bind(now())
            .execute(&mut *transaction)
            .await;

            match insert {
                Ok(result) if result.rows_affected() == 1 => {
                    transaction.commit().await.map_err(persistence)?;
                    Ok((inbox_id, false))
                }
                Ok(_) => {
                    let Some((id, stored_hash, _)) = find_webhook_identity(
                        &mut transaction,
                        &endpoint.tenant_id,
                        &endpoint.endpoint_id,
                        &provider_event_id,
                    )
                    .await?
                    else {
                        let _ = transaction.rollback().await;
                        return Err(IntegrationError::Persistence);
                    };
                    if stored_hash != payload_hash {
                        let _ = transaction.rollback().await;
                        return Err(IntegrationError::WebhookUnverifiable);
                    }
                    transaction.commit().await.map_err(persistence)?;
                    Ok((WebhookInboxId::new(id)?, true))
                }
                Err(error) if is_webhook_payload_conflict(&error) => {
                    let _ = transaction.rollback().await;
                    Err(IntegrationError::WebhookUnverifiable)
                }
                Err(error) => {
                    let _ = transaction.rollback().await;
                    Err(persistence(error))
                }
            }
        })
    }

    fn record_rejected_webhook(
        &self,
        endpoint: &WebhookEndpointContext,
        provider_event_id: &str,
        headers_json: &str,
        raw_payload: &[u8],
        classification: &str,
    ) -> Result<(), IntegrationError> {
        if provider_event_id.trim().is_empty()
            || raw_payload.is_empty()
            || classification.trim().is_empty()
        {
            return Err(IntegrationError::WebhookUnverifiable);
        }

        let pool = self.pool.clone();
        let endpoint = endpoint.clone();
        let provider_event_id = provider_event_id.to_owned();
        let headers_json = headers_json.to_owned();
        let raw_payload = raw_payload.to_vec();
        let classification = classification.to_owned();
        let payload_hash = hex::encode(Sha256::digest(&raw_payload));

        run_pg_webhook(async move {
            let mut transaction = serializable(&pool).await?;
            let existing = find_webhook_identity(
                &mut transaction,
                &endpoint.tenant_id,
                &endpoint.endpoint_id,
                &provider_event_id,
            )
            .await?;
            if let Some((_id, stored_hash, _status)) = existing {
                if stored_hash != payload_hash {
                    let _ = transaction.rollback().await;
                    return Err(IntegrationError::WebhookUnverifiable);
                }
                transaction.commit().await.map_err(persistence)?;
                return Ok(());
            }

            let result = sqlx::query(
                "INSERT INTO webhook_inbox
                 (id,tenant_id,endpoint_id,provider_event_id,payload_hash,headers_json,
                  raw_payload,received_at,status,error_classification)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'rejected',$9)
                 ON CONFLICT (tenant_id,endpoint_id,provider_event_id) DO NOTHING",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&endpoint.tenant_id)
            .bind(endpoint.endpoint_id.as_str())
            .bind(&provider_event_id)
            .bind(&payload_hash)
            .bind(&headers_json)
            .bind(&raw_payload)
            .bind(now())
            .bind(&classification)
            .execute(&mut *transaction)
            .await;

            match result {
                Ok(_) => transaction.commit().await.map_err(persistence),
                Err(error) if is_webhook_payload_conflict(&error) => {
                    let _ = transaction.rollback().await;
                    Err(IntegrationError::WebhookUnverifiable)
                }
                Err(error) => {
                    let _ = transaction.rollback().await;
                    Err(persistence(error))
                }
            }
        })
    }

    fn claim_next_webhook(
        &self,
        tenant_id: &str,
    ) -> Result<Option<ClaimedWebhook>, IntegrationError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();

        run_pg_webhook(async move {
            let mut transaction = serializable(&pool).await?;
            let row = sqlx::query(
                "SELECT i.id,i.endpoint_id,e.binding_id,e.provider_id,
                        i.provider_event_id,i.headers_json,i.raw_payload
                 FROM webhook_inbox i
                 JOIN webhook_endpoints e
                   ON e.tenant_id=i.tenant_id AND e.id=i.endpoint_id
                 JOIN provider_bindings b
                   ON b.tenant_id=e.tenant_id AND b.id=e.binding_id
                 JOIN provider_instances p
                   ON p.tenant_id=b.tenant_id AND p.id=b.provider_instance_id
                 JOIN provider_manifests m
                   ON m.provider_id=p.provider_id AND m.version=p.manifest_version
                 WHERE i.tenant_id=$1
                   AND i.status='verified'
                   AND e.enabled
                   AND b.enabled
                   AND p.lifecycle='active'
                   AND p.health='ready'
                   AND p.readiness='fixture'
                   AND m.readiness='fixture'
                 ORDER BY i.received_at,i.id
                 LIMIT 1
                 FOR UPDATE OF i,e,b,p SKIP LOCKED",
            )
            .bind(&tenant_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence)?;

            let Some(row) = row else {
                transaction.commit().await.map_err(persistence)?;
                return Ok(None);
            };

            let inbox_id = WebhookInboxId::new(row.try_get::<String, _>(0).map_err(persistence)?)?;
            let endpoint_id =
                WebhookEndpointId::new(row.try_get::<String, _>(1).map_err(persistence)?)?;
            let binding_id =
                ProviderBindingId::new(row.try_get::<String, _>(2).map_err(persistence)?)?;
            let provider_id = ProviderId::new(row.try_get::<String, _>(3).map_err(persistence)?)?;
            let provider_event_id: String = row.try_get(4).map_err(persistence)?;
            let headers_json: String = row.try_get(5).map_err(persistence)?;
            let raw_payload: Vec<u8> = row.try_get(6).map_err(persistence)?;

            let attempts: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)
                 FROM webhook_processing_attempts
                 WHERE tenant_id=$1 AND inbox_id=$2",
            )
            .bind(&tenant_id)
            .bind(inbox_id.as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(persistence)?;
            let attempt_number = u32::try_from(attempts)
                .map_err(persistence)?
                .checked_add(1)
                .ok_or(IntegrationError::InvalidOperationTransition)?;

            let timestamp = now();
            let changed = sqlx::query(
                "UPDATE webhook_inbox
                 SET status='processing',error_classification=NULL
                 WHERE tenant_id=$1 AND id=$2 AND status='verified'",
            )
            .bind(&tenant_id)
            .bind(inbox_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?
            .rows_affected();
            if changed != 1 {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::InvalidOperationTransition);
            }

            sqlx::query(
                "INSERT INTO webhook_processing_attempts
                 (id,tenant_id,inbox_id,attempt_number,state,started_at)
                 VALUES ($1,$2,$3,$4,'processing',$5)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&tenant_id)
            .bind(inbox_id.as_str())
            .bind(i64::from(attempt_number))
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            transaction.commit().await.map_err(persistence)?;
            Ok(Some(ClaimedWebhook {
                tenant_id: tenant_id.clone(),
                inbox_id,
                endpoint: WebhookEndpointContext {
                    endpoint_id,
                    tenant_id,
                    binding_id,
                    provider_id,
                },
                provider_event_id,
                headers_json,
                raw_payload,
                attempt_number,
            }))
        })
    }

    fn complete_webhook(
        &self,
        webhook: &ClaimedWebhook,
        canonical_event_type: &str,
    ) -> Result<(), IntegrationError> {
        if canonical_event_type.trim().is_empty() {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        self.finish_webhook_attempt(
            webhook,
            "processed",
            Some(canonical_event_type),
            None,
            "processed",
        )
    }

    fn retry_webhook(
        &self,
        webhook: &ClaimedWebhook,
        classification: &str,
    ) -> Result<(), IntegrationError> {
        self.finish_webhook_attempt(
            webhook,
            "verified",
            None,
            Some(classification),
            "retryable_failure",
        )
    }

    fn dead_letter_webhook(
        &self,
        webhook: &ClaimedWebhook,
        reason: &str,
    ) -> Result<(), IntegrationError> {
        if reason.trim().is_empty() {
            return Err(IntegrationError::InvalidOperationTransition);
        }

        let pool = self.pool.clone();
        let webhook = webhook.clone();
        let reason = reason.to_owned();

        run_pg_webhook(async move {
            let mut transaction = serializable(&pool).await?;
            let timestamp = now();
            let changed = sqlx::query(
                "UPDATE webhook_inbox
                 SET status='dead_letter',error_classification=$1
                 WHERE id=$2 AND tenant_id=$3 AND status='processing'",
            )
            .bind(&reason)
            .bind(webhook.inbox_id.as_str())
            .bind(&webhook.tenant_id)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?
            .rows_affected();
            if changed != 1 {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::InvalidOperationTransition);
            }

            sqlx::query(
                "UPDATE webhook_processing_attempts
                 SET state='dead_letter',classification=$1,completed_at=$2
                 WHERE tenant_id=$3
                   AND inbox_id=$4
                   AND attempt_number=$5
                   AND state='processing'",
            )
            .bind(&reason)
            .bind(&timestamp)
            .bind(&webhook.tenant_id)
            .bind(webhook.inbox_id.as_str())
            .bind(i64::from(webhook.attempt_number))
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            sqlx::query(
                "INSERT INTO webhook_dead_letters
                 (id,tenant_id,inbox_id,reason,created_at)
                 VALUES ($1,$2,$3,$4,$5)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&webhook.tenant_id)
            .bind(webhook.inbox_id.as_str())
            .bind(&reason)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            transaction.commit().await.map_err(persistence)
        })
    }
}

impl WebhookAdminPersistence for PostgresWebhookPersistence {
    fn register_webhook_endpoint(
        &self,
        tenant_id: &str,
        binding_id: &ProviderBindingId,
        endpoint_token: &[u8],
    ) -> Result<WebhookEndpointId, IntegrationError> {
        if endpoint_token.is_empty() {
            return Err(IntegrationError::WebhookUnverifiable);
        }

        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let binding_id = binding_id.clone();
        let token_hash = hex::encode(Sha256::digest(endpoint_token));

        run_pg_webhook(async move {
            let mut transaction = serializable(&pool).await?;
            let row = sqlx::query(
                "SELECT i.provider_id,b.enabled,i.lifecycle,i.health,i.readiness,m.readiness
                 FROM provider_bindings b
                 JOIN provider_instances i
                   ON i.tenant_id=b.tenant_id AND i.id=b.provider_instance_id
                 JOIN provider_manifests m
                   ON m.provider_id=i.provider_id AND m.version=i.manifest_version
                 WHERE b.tenant_id=$1 AND b.id=$2
                 FOR UPDATE OF b,i",
            )
            .bind(&tenant_id)
            .bind(binding_id.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence)?;

            let Some(row) = row else {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::BindingUnavailable);
            };
            let provider_id: String = row.try_get(0).map_err(persistence)?;
            let binding_enabled: bool = row.try_get(1).map_err(persistence)?;
            let lifecycle: String = row.try_get(2).map_err(persistence)?;
            let health: String = row.try_get(3).map_err(persistence)?;
            let instance_readiness: String = row.try_get(4).map_err(persistence)?;
            let manifest_readiness: String = row.try_get(5).map_err(persistence)?;

            if !binding_enabled
                || lifecycle != "active"
                || health != "ready"
                || instance_readiness != "fixture"
                || manifest_readiness != "fixture"
            {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::BindingUnavailable);
            }

            let endpoint_id = WebhookEndpointId::new(uuid::Uuid::new_v4().to_string())?;
            sqlx::query(
                "INSERT INTO webhook_endpoints
                 (id,tenant_id,binding_id,provider_id,token_hash,enabled,created_at)
                 VALUES ($1,$2,$3,$4,$5,TRUE,$6)",
            )
            .bind(endpoint_id.as_str())
            .bind(&tenant_id)
            .bind(binding_id.as_str())
            .bind(&provider_id)
            .bind(&token_hash)
            .bind(now())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;
            transaction.commit().await.map_err(persistence)?;
            Ok(endpoint_id)
        })
    }

    fn list_webhook_endpoints(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<WebhookEndpointSummary>, IntegrationError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();

        run_pg_webhook(async move {
            let mut transaction = serializable_read_only(&pool).await?;
            let rows = sqlx::query(
                "SELECT id,binding_id,provider_id,enabled,created_at
                 FROM webhook_endpoints
                 WHERE tenant_id=$1
                 ORDER BY created_at DESC,id DESC",
            )
            .bind(&tenant_id)
            .fetch_all(&mut *transaction)
            .await
            .map_err(persistence)?;

            let mut summaries = Vec::with_capacity(rows.len());
            for row in rows {
                summaries.push(WebhookEndpointSummary {
                    endpoint_id: WebhookEndpointId::new(
                        row.try_get::<String, _>(0).map_err(persistence)?,
                    )?,
                    binding_id: ProviderBindingId::new(
                        row.try_get::<String, _>(1).map_err(persistence)?,
                    )?,
                    provider_id: ProviderId::new(
                        row.try_get::<String, _>(2).map_err(persistence)?,
                    )?,
                    enabled: row.try_get(3).map_err(persistence)?,
                    created_at: row.try_get(4).map_err(persistence)?,
                });
            }
            transaction.commit().await.map_err(persistence)?;
            Ok(summaries)
        })
    }

    fn set_webhook_endpoint_enabled(
        &self,
        tenant_id: &str,
        endpoint_id: &WebhookEndpointId,
        enabled: bool,
    ) -> Result<(), IntegrationError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let endpoint_id = endpoint_id.clone();

        run_pg_webhook(async move {
            let mut transaction = serializable(&pool).await?;
            let changed = sqlx::query(
                "UPDATE webhook_endpoints
                 SET enabled=$1
                 WHERE tenant_id=$2 AND id=$3",
            )
            .bind(enabled)
            .bind(&tenant_id)
            .bind(endpoint_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?
            .rows_affected();
            if changed != 1 {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::BindingUnavailable);
            }
            transaction.commit().await.map_err(persistence)
        })
    }

    fn list_webhook_dead_letters(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<WebhookDeadLetterSummary>, IntegrationError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();

        run_pg_webhook(async move {
            let mut transaction = serializable_read_only(&pool).await?;
            let rows = sqlx::query(
                "SELECT d.inbox_id,i.endpoint_id,i.provider_event_id,d.reason,
                        d.replay_count,d.created_at,d.replayed_at
                 FROM webhook_dead_letters d
                 JOIN webhook_inbox i
                   ON i.id=d.inbox_id AND i.tenant_id=d.tenant_id
                 WHERE d.tenant_id=$1
                 ORDER BY d.created_at DESC,d.id DESC",
            )
            .bind(&tenant_id)
            .fetch_all(&mut *transaction)
            .await
            .map_err(persistence)?;

            let mut summaries = Vec::with_capacity(rows.len());
            for row in rows {
                let replay_count: i64 = row.try_get(4).map_err(persistence)?;
                summaries.push(WebhookDeadLetterSummary {
                    inbox_id: WebhookInboxId::new(
                        row.try_get::<String, _>(0).map_err(persistence)?,
                    )?,
                    endpoint_id: WebhookEndpointId::new(
                        row.try_get::<String, _>(1).map_err(persistence)?,
                    )?,
                    provider_event_id: row.try_get(2).map_err(persistence)?,
                    reason: row.try_get(3).map_err(persistence)?,
                    replay_count: u64::try_from(replay_count).map_err(persistence)?,
                    created_at: row.try_get(5).map_err(persistence)?,
                    replayed_at: row.try_get(6).map_err(persistence)?,
                });
            }
            transaction.commit().await.map_err(persistence)?;
            Ok(summaries)
        })
    }
}

async fn find_webhook_identity(
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    tenant_id: &str,
    endpoint_id: &WebhookEndpointId,
    provider_event_id: &str,
) -> Result<Option<(String, String, String)>, IntegrationError> {
    let row = sqlx::query(
        "SELECT id,payload_hash,status
         FROM webhook_inbox
         WHERE tenant_id=$1 AND endpoint_id=$2 AND provider_event_id=$3
         FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(endpoint_id.as_str())
    .bind(provider_event_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence)?;

    row.map(|row| {
        Ok((
            row.try_get(0).map_err(persistence)?,
            row.try_get(1).map_err(persistence)?,
            row.try_get(2).map_err(persistence)?,
        ))
    })
    .transpose()
}

fn is_webhook_payload_conflict(error: &sqlx::Error) -> bool {
    error.as_database_error().is_some_and(|database| {
        database
            .message()
            .contains("webhook event id reused with different payload")
    })
}

async fn serializable(pool: &PgPool) -> Result<Transaction<'_, sqlx::Postgres>, IntegrationError> {
    let mut transaction = pool.begin().await.map_err(persistence)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *transaction)
        .await
        .map_err(persistence)?;
    Ok(transaction)
}

async fn serializable_read_only(
    pool: &PgPool,
) -> Result<Transaction<'_, sqlx::Postgres>, IntegrationError> {
    let mut transaction = pool.begin().await.map_err(persistence)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(persistence)?;
    Ok(transaction)
}

fn run_pg_webhook<T, F>(future: F) -> Result<T, IntegrationError>
where
    T: Send,
    F: Future<Output = Result<T, IntegrationError>> + Send,
{
    let handle = Handle::try_current().map_err(persistence)?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(IntegrationError::Persistence);
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn persistence<T>(_error: T) -> IntegrationError {
    IntegrationError::Persistence
}
