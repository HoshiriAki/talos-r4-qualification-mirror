use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use sha2::{Digest, Sha256};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use super::operation::ExternalOperation;
use super::store::IntegrationStore;
use super::types::{
    CapabilityId, ExternalOperationId, IntegrationError, Money, ProviderBinding, ProviderBindingId,
    ProviderHealth, ProviderId, ProviderInstance, ProviderInstanceId, ProviderLifecycle,
    ProviderManifest, ProviderReadiness,
};
use super::webhook::{WebhookReceiptInput, WebhookRuntime, WebhookVerifier};
use crate::db::migrations::run_migrations;

struct Fixture {
    pool: Pool<SqliteConnectionManager>,
    store: IntegrationStore,
    capability: CapabilityId,
    binding: ProviderBinding,
}

fn fixture() -> Fixture {
    let pool = Pool::builder()
        .max_size(1)
        .build(SqliteConnectionManager::memory())
        .unwrap();
    run_migrations(&pool.get().unwrap()).unwrap();
    let store = IntegrationStore::new(pool.clone());
    let provider = ProviderId::new("fixture-hardening").unwrap();
    let capability = CapabilityId::new("fixture.hardening").unwrap();
    store
        .save_manifest(&ProviderManifest {
            provider_id: provider.clone(),
            version: "1".into(),
            capabilities: BTreeSet::from([capability.clone()]),
            config_schema: vec![],
            secret_schema: vec![],
            api_versions: BTreeMap::new(),
            webhook_types: BTreeSet::from(["fixture.hardening.updated".into()]),
            simulation_capabilities: BTreeSet::from([capability.clone()]),
            readiness: ProviderReadiness::Fixture,
            compatibility: BTreeMap::new(),
        })
        .unwrap();
    let instance = ProviderInstance {
        id: ProviderInstanceId::new("hardening-instance").unwrap(),
        tenant_id: "tenant-hardening".into(),
        provider_id: provider,
        manifest_version: "1".into(),
        config_revision: "revision-1".into(),
        config: BTreeMap::new(),
        secret_refs: BTreeMap::new(),
        lifecycle: ProviderLifecycle::Active,
        health: ProviderHealth::Ready,
        readiness: ProviderReadiness::Fixture,
    };
    store.save_instance(&instance).unwrap();
    let binding = ProviderBinding {
        id: ProviderBindingId::new("hardening-binding").unwrap(),
        tenant_id: "tenant-hardening".into(),
        provider_instance_id: instance.id,
        capability: capability.clone(),
        config_revision: "revision-1".into(),
        enabled: true,
    };
    store.save_binding(&binding, "hardening-test").unwrap();
    store
        .register_webhook_endpoint("tenant-hardening", &binding.id, b"hardening-webhook-token")
        .unwrap();
    Fixture {
        pool,
        store,
        capability,
        binding,
    }
}

fn context() -> ExecutionContext {
    let tenant = TenantId::new("tenant-hardening").unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("hardening-test", "admin").unwrap(),
        TenantScope::tenant(tenant.clone()),
        DataScope::production(tenant, Revision::new("hardening-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new("hardening-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn ready_operation(
    fixture: &Fixture,
    id: &str,
    operation_type: &str,
    idempotency_key: String,
) -> ExternalOperation {
    let resolved = fixture
        .store
        .catalog()
        .unwrap()
        .resolve(&context(), &fixture.capability)
        .unwrap();
    let mut operation = ExternalOperation::planned(
        ExternalOperationId::new(id).unwrap(),
        &resolved,
        operation_type,
        idempotency_key,
        format!("request-hash-{id}"),
    )
    .unwrap();
    operation.ready().unwrap();
    operation
}

fn operation_state_and_classification(fixture: &Fixture, id: &str) -> (String, Option<String>) {
    fixture
        .pool
        .get()
        .unwrap()
        .query_row(
            "SELECT state, classification FROM external_operations WHERE tenant_id = ?1 AND id = ?2",
            ["tenant-hardening", id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

#[test]
fn provider_and_binding_identity_cannot_drift_without_new_identity_or_revision() {
    let fixture = fixture();
    let conn = fixture.pool.get().unwrap();

    let same_revision_readiness_change = conn.execute(
        "UPDATE provider_instances SET readiness = 'stub' WHERE tenant_id = ?1 AND id = ?2",
        ["tenant-hardening", "hardening-instance"],
    );
    assert!(same_revision_readiness_change.is_err());

    let binding_retarget = conn.execute(
        "UPDATE provider_bindings SET capability_id = 'fixture.retargeted' WHERE tenant_id = ?1 AND id = ?2",
        ["tenant-hardening", fixture.binding.id.as_str()],
    );
    assert!(binding_retarget.is_err());
}

#[test]
fn provider_availability_change_invalidates_queued_effect_and_does_not_auto_resume() {
    let fixture = fixture();
    let operation = ready_operation(
        &fixture,
        "operation-invalidated",
        "fixture.operation",
        "invalidated-idempotency".into(),
    );
    fixture.store.persist_operation(&operation).unwrap();

    fixture
        .pool
        .get()
        .unwrap()
        .execute(
            "UPDATE provider_instances SET health = 'degraded', updated_at = '2026-09-02T03:00:00Z' WHERE tenant_id = ?1 AND id = ?2",
            ["tenant-hardening", "hardening-instance"],
        )
        .unwrap();
    let (state, classification) =
        operation_state_and_classification(&fixture, operation.id.as_str());
    assert_eq!(state, "manual_resolution_required");
    assert_eq!(
        classification.as_deref(),
        Some("provider_instance_availability_changed")
    );

    fixture
        .pool
        .get()
        .unwrap()
        .execute(
            "UPDATE provider_instances SET health = 'ready', updated_at = '2026-09-02T03:01:00Z' WHERE tenant_id = ?1 AND id = ?2",
            ["tenant-hardening", "hardening-instance"],
        )
        .unwrap();
    assert_eq!(
        operation_state_and_classification(&fixture, operation.id.as_str()).0,
        "manual_resolution_required"
    );
}

#[test]
fn circuit_open_defers_due_work_until_open_window_expires() {
    let fixture = fixture();
    let operation = ready_operation(
        &fixture,
        "operation-circuit-deferred",
        "fixture.operation",
        "circuit-deferred-idempotency".into(),
    );
    fixture.store.persist_operation(&operation).unwrap();
    let open_until = "9998-01-01T00:00:00Z";
    fixture
        .pool
        .get()
        .unwrap()
        .execute(
            "INSERT INTO integration_circuit_state (tenant_id, binding_id, state, failure_count, opened_until, updated_at) VALUES (?1, ?2, 'open', 3, ?3, ?4)",
            [
                "tenant-hardening",
                fixture.binding.id.as_str(),
                open_until,
                "2026-09-02T03:00:00Z",
            ],
        )
        .unwrap();

    let next_retry_at: Option<String> = fixture
        .pool
        .get()
        .unwrap()
        .query_row(
            "SELECT next_retry_at FROM external_operations WHERE tenant_id = ?1 AND id = ?2",
            ["tenant-hardening", operation.id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(next_retry_at.as_deref(), Some(open_until));
    assert!(
        fixture
            .store
            .claim_next_operation(
                "tenant-hardening",
                &crate::integration::runtime::OperationRuntimePolicy::default(),
            )
            .unwrap()
            .is_none()
    );
}

#[test]
fn half_open_circuit_allows_only_one_dispatch_probe() {
    let fixture = fixture();
    let first = ready_operation(
        &fixture,
        "operation-half-open-a",
        "fixture.operation",
        "half-open-a".into(),
    );
    let second = ready_operation(
        &fixture,
        "operation-half-open-b",
        "fixture.operation",
        "half-open-b".into(),
    );
    fixture.store.persist_operation(&first).unwrap();
    fixture.store.persist_operation(&second).unwrap();
    fixture
        .pool
        .get()
        .unwrap()
        .execute(
            "INSERT INTO integration_circuit_state (tenant_id, binding_id, state, failure_count, opened_until, updated_at) VALUES (?1, ?2, 'half_open', 3, NULL, ?3)",
            [
                "tenant-hardening",
                fixture.binding.id.as_str(),
                "2026-09-02T03:00:00Z",
            ],
        )
        .unwrap();
    fixture
        .pool
        .get()
        .unwrap()
        .execute(
            "UPDATE external_operations SET state = 'dispatching' WHERE tenant_id = ?1 AND id = ?2",
            ["tenant-hardening", first.id.as_str()],
        )
        .unwrap();
    assert!(
        fixture
            .pool
            .get()
            .unwrap()
            .execute(
                "UPDATE external_operations SET state = 'dispatching' WHERE tenant_id = ?1 AND id = ?2",
                ["tenant-hardening", second.id.as_str()],
            )
            .is_err()
    );
}

#[test]
fn external_operation_intent_fields_are_immutable_after_admission() {
    let fixture = fixture();
    let operation = ready_operation(
        &fixture,
        "operation-immutable",
        "fixture.operation",
        "immutable-idempotency".into(),
    );
    fixture.store.persist_operation(&operation).unwrap();

    let changed = fixture.pool.get().unwrap().execute(
        "UPDATE external_operations SET request_hash = 'mutated' WHERE tenant_id = ?1 AND id = ?2",
        ["tenant-hardening", operation.id.as_str()],
    );
    assert!(changed.is_err());
}

#[test]
fn refund_operation_and_refund_intent_are_admitted_atomically() {
    let fixture = fixture();
    let deposit = fixture
        .store
        .create_deposit(
            "tenant-hardening",
            "order",
            "order-refund-atomic",
            Money::new(10_000, "CNY").unwrap(),
        )
        .unwrap();
    fixture
        .store
        .record_deposit_received(
            "tenant-hardening",
            &deposit,
            Money::new(10_000, "CNY").unwrap(),
            "receipt-a",
        )
        .unwrap();
    let refund = fixture
        .store
        .request_refund(
            "tenant-hardening",
            &deposit,
            Money::new(1_000, "CNY").unwrap(),
            "refund-request-a",
            "customer request",
        )
        .unwrap();

    let operation = ready_operation(
        &fixture,
        "operation-refund-atomic",
        "refund",
        format!("refund:{}", refund.as_str()),
    );
    fixture.store.persist_operation(&operation).unwrap();

    let (state, linked): (String, Option<String>) = fixture
        .pool
        .get()
        .unwrap()
        .query_row(
            "SELECT state, external_operation_id FROM refund_intents WHERE tenant_id = ?1 AND id = ?2",
            ["tenant-hardening", refund.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "approved");
    assert_eq!(linked.as_deref(), Some(operation.id.as_str()));

    let orphan = ready_operation(
        &fixture,
        "operation-refund-orphan",
        "refund",
        "refund:missing-refund-intent".into(),
    );
    assert!(fixture.store.persist_operation(&orphan).is_err());
    let orphan_count: i64 = fixture
        .pool
        .get()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM external_operations WHERE tenant_id = ?1 AND id = ?2",
            ["tenant-hardening", orphan.id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(orphan_count, 0);
}

#[test]
fn generic_fixture_refund_operation_does_not_claim_refund_authority() {
    let fixture = fixture();
    let operation = ready_operation(
        &fixture,
        "operation-generic-refund",
        "refund",
        "generic-refund-fixture".into(),
    );
    fixture.store.persist_operation(&operation).unwrap();
}

struct AcceptingVerifier;

#[async_trait]
impl WebhookVerifier for AcceptingVerifier {
    async fn verify(
        &self,
        _endpoint: &super::webhook::WebhookEndpointContext,
        _headers_json: &str,
        _raw_payload: &[u8],
    ) -> Result<(), IntegrationError> {
        Ok(())
    }
}

struct RejectingVerifier;

#[async_trait]
impl WebhookVerifier for RejectingVerifier {
    async fn verify(
        &self,
        _endpoint: &super::webhook::WebhookEndpointContext,
        _headers_json: &str,
        _raw_payload: &[u8],
    ) -> Result<(), IntegrationError> {
        Err(IntegrationError::WebhookUnverifiable)
    }
}

fn webhook_input(event_id: &str, payload: &[u8]) -> WebhookReceiptInput {
    WebhookReceiptInput {
        endpoint_token: b"hardening-webhook-token".to_vec(),
        provider_event_id: event_id.into(),
        verification_headers_json: "{}".into(),
        stored_headers_json: "{}".into(),
        raw_payload: payload.to_vec(),
    }
}

#[tokio::test]
async fn verified_retry_promotes_same_payload_after_rejected_receipt() {
    let fixture = fixture();
    let runtime = WebhookRuntime::new(fixture.store.clone());
    let payload = br#"{"eventType":"fixture.hardening.updated"}"#;

    assert!(
        runtime
            .receive(
                webhook_input("provider-event-promote", payload),
                &RejectingVerifier,
            )
            .await
            .is_err()
    );
    let receipt = runtime
        .receive(
            webhook_input("provider-event-promote", payload),
            &AcceptingVerifier,
        )
        .await
        .unwrap();
    assert!(receipt.duplicate);

    let status: String = fixture
        .pool
        .get()
        .unwrap()
        .query_row(
            "SELECT status FROM webhook_inbox WHERE tenant_id = ?1 AND provider_event_id = ?2",
            ["tenant-hardening", "provider-event-promote"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "verified");
}

#[tokio::test]
async fn reused_webhook_event_id_with_different_payload_is_rejected() {
    let fixture = fixture();
    let runtime = WebhookRuntime::new(fixture.store.clone());
    runtime
        .receive(
            webhook_input("provider-event-conflict", br#"{"value":1}"#),
            &AcceptingVerifier,
        )
        .await
        .unwrap();

    assert!(
        runtime
            .receive(
                webhook_input("provider-event-conflict", br#"{"value":2}"#),
                &AcceptingVerifier,
            )
            .await
            .is_err()
    );

    let expected_hash = hex::encode(Sha256::digest(br#"{"value":1}"#));
    let stored_hash: String = fixture
        .pool
        .get()
        .unwrap()
        .query_row(
            "SELECT payload_hash FROM webhook_inbox WHERE tenant_id = ?1 AND provider_event_id = ?2",
            ["tenant-hardening", "provider-event-conflict"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_hash, expected_hash);
}

#[test]
fn late_deposit_receipt_cannot_append_ledger_after_release() {
    let fixture = fixture();
    let deposit = fixture
        .store
        .create_deposit(
            "tenant-hardening",
            "order",
            "order-deposit-state",
            Money::new(2_000, "CNY").unwrap(),
        )
        .unwrap();
    fixture
        .store
        .record_deposit_received(
            "tenant-hardening",
            &deposit,
            Money::new(2_000, "CNY").unwrap(),
            "receipt-before-release",
        )
        .unwrap();
    fixture
        .store
        .release_deposit(
            "tenant-hardening",
            &deposit,
            Money::new(2_000, "CNY").unwrap(),
            "release-a",
        )
        .unwrap();

    assert!(
        fixture
            .store
            .record_deposit_received(
                "tenant-hardening",
                &deposit,
                Money::new(500, "CNY").unwrap(),
                "late-receipt",
            )
            .is_err()
    );

    let receipt_count: i64 = fixture
        .pool
        .get()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM integration_deposit_ledger WHERE tenant_id = ?1 AND deposit_id = ?2 AND entry_type = 'received'",
            ["tenant-hardening", deposit.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(receipt_count, 1);
}
