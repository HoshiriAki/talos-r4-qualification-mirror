-- 059: tenant-scoped Provider Integration Fabric.
-- Manifest/config/secret-requirement schemas are descriptive. Provider
-- secrets are represented only by secret refs and never stored in plaintext.

CREATE TABLE provider_manifests (
    provider_id TEXT NOT NULL,
    version TEXT NOT NULL,
    capabilities_json TEXT NOT NULL,
    config_schema_json TEXT NOT NULL,
    secret_schema_json TEXT NOT NULL,
    api_versions_json TEXT NOT NULL,
    webhook_types_json TEXT NOT NULL,
    simulation_capabilities_json TEXT NOT NULL,
    readiness TEXT NOT NULL CHECK (readiness IN ('stub', 'fixture', 'sandbox', 'production')),
    compatibility_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (provider_id, version)
);

CREATE TABLE provider_instances (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    manifest_version TEXT NOT NULL,
    config_revision TEXT NOT NULL,
    config_json TEXT NOT NULL,
    secret_refs_json TEXT NOT NULL,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('draft', 'active', 'suspended', 'retired')),
    readiness TEXT NOT NULL CHECK (readiness IN ('stub', 'fixture', 'sandbox', 'production')),
    health TEXT NOT NULL CHECK (health IN ('unknown', 'ready', 'degraded', 'unhealthy')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (provider_id, manifest_version) REFERENCES provider_manifests(provider_id, version)
);

-- A ProviderInstance ID identifies one Provider + immutable Manifest lineage.
-- Configuration/secret/readiness changes are allowed only with a new revision,
-- so queued ExternalOperations can pin a stable execution snapshot.
CREATE TRIGGER provider_instance_identity_immutable
BEFORE UPDATE OF provider_id, manifest_version ON provider_instances
WHEN NEW.provider_id <> OLD.provider_id OR NEW.manifest_version <> OLD.manifest_version
BEGIN
    SELECT RAISE(ABORT, 'provider instance identity is immutable');
END;
CREATE TRIGGER provider_instance_revision_guard
BEFORE UPDATE OF config_revision, config_json, secret_refs_json, readiness ON provider_instances
WHEN NEW.config_revision = OLD.config_revision
 AND (
    NEW.config_json <> OLD.config_json
    OR NEW.secret_refs_json <> OLD.secret_refs_json
    OR NEW.readiness <> OLD.readiness
 )
BEGIN
    SELECT RAISE(ABORT, 'provider instance configuration change requires a new revision');
END;

CREATE TABLE provider_bindings (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    provider_instance_id TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    config_revision TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, capability_id),
    FOREIGN KEY (tenant_id, provider_instance_id) REFERENCES provider_instances(tenant_id, id)
);

-- Reusing a Binding ID for another instance/capability would silently reroute
-- already-admitted work. Rebind by creating a new Binding identity instead.
CREATE TRIGGER provider_binding_identity_immutable
BEFORE UPDATE OF provider_instance_id, capability_id ON provider_bindings
WHEN NEW.provider_instance_id <> OLD.provider_instance_id
  OR NEW.capability_id <> OLD.capability_id
BEGIN
    SELECT RAISE(ABORT, 'provider binding identity is immutable');
END;

CREATE TABLE provider_binding_history (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    binding_id TEXT NOT NULL,
    revision TEXT NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('created', 'updated', 'disabled', 'enabled', 'retired')),
    occurred_at TEXT NOT NULL,
    actor_ref TEXT NOT NULL,
    UNIQUE (tenant_id, binding_id, revision, action)
);

CREATE TABLE external_operations (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    provider_instance_id TEXT NOT NULL,
    binding_id TEXT NOT NULL,
    binding_revision TEXT NOT NULL,
    capability_id TEXT NOT NULL,
    operation_type TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'planned', 'ready', 'dispatching', 'succeeded', 'rejected',
        'retryable_failure', 'non_retryable_failure', 'unknown_outcome',
        'reconciling', 'resolved', 'manual_resolution_required'
    )),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    next_retry_at TEXT,
    result_ref TEXT,
    classification TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, binding_id, idempotency_key),
    FOREIGN KEY (tenant_id, binding_id) REFERENCES provider_bindings(tenant_id, id)
);

-- These columns are the durable external-effect intent snapshot. Runtime state
-- may advance, but identity/routing/idempotency/request identity may not mutate.
CREATE TRIGGER external_operation_intent_immutable
BEFORE UPDATE OF tenant_id, provider_instance_id, binding_id, binding_revision,
                 capability_id, operation_type, idempotency_key, request_hash
ON external_operations
WHEN NEW.tenant_id <> OLD.tenant_id
  OR NEW.provider_instance_id <> OLD.provider_instance_id
  OR NEW.binding_id <> OLD.binding_id
  OR NEW.binding_revision <> OLD.binding_revision
  OR NEW.capability_id <> OLD.capability_id
  OR NEW.operation_type <> OLD.operation_type
  OR NEW.idempotency_key <> OLD.idempotency_key
  OR NEW.request_hash <> OLD.request_hash
BEGIN
    SELECT RAISE(ABORT, 'external operation intent is immutable');
END;

CREATE TABLE external_operation_attempts (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    attempt_number INTEGER NOT NULL CHECK (attempt_number > 0),
    state TEXT NOT NULL,
    classification TEXT,
    request_hash TEXT NOT NULL,
    provider_result_ref TEXT,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    UNIQUE (tenant_id, operation_id, attempt_number),
    FOREIGN KEY (tenant_id, operation_id) REFERENCES external_operations(tenant_id, id)
);

CREATE TABLE external_operation_reconciliations (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('pending', 'effect_confirmed', 'effect_absent', 'manual_required')),
    evidence_ref TEXT,
    resolved_by TEXT,
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    FOREIGN KEY (tenant_id, operation_id) REFERENCES external_operations(tenant_id, id)
);

CREATE TABLE integration_circuit_state (
    tenant_id TEXT NOT NULL,
    binding_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('closed', 'open', 'half_open')),
    failure_count INTEGER NOT NULL DEFAULT 0 CHECK (failure_count >= 0),
    opened_until TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, binding_id),
    FOREIGN KEY (tenant_id, binding_id) REFERENCES provider_bindings(tenant_id, id)
);

CREATE TABLE webhook_endpoints (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    binding_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    token_hash TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    created_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (token_hash),
    FOREIGN KEY (tenant_id, binding_id) REFERENCES provider_bindings(tenant_id, id)
);

CREATE TABLE webhook_inbox (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    endpoint_id TEXT NOT NULL,
    provider_event_id TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    headers_json TEXT NOT NULL,
    raw_payload BLOB NOT NULL,
    received_at TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('verified', 'processing', 'processed', 'rejected', 'dead_letter')),
    canonical_event_type TEXT,
    error_classification TEXT,
    UNIQUE (tenant_id, endpoint_id, provider_event_id),
    FOREIGN KEY (tenant_id, endpoint_id) REFERENCES webhook_endpoints(tenant_id, id)
);

-- Provider event IDs are identities, not a license to replace payloads. The
-- same ID with different bytes is an integrity conflict. A callback that was
-- previously rejected may be promoted only when a later verified retry carries
-- exactly the same payload; this prevents an invalid first attempt from
-- permanently squatting on a legitimate event ID.
CREATE TRIGGER webhook_event_payload_conflict
BEFORE INSERT ON webhook_inbox
WHEN EXISTS (
    SELECT 1 FROM webhook_inbox existing
    WHERE existing.tenant_id = NEW.tenant_id
      AND existing.endpoint_id = NEW.endpoint_id
      AND existing.provider_event_id = NEW.provider_event_id
      AND existing.payload_hash <> NEW.payload_hash
)
BEGIN
    SELECT RAISE(ABORT, 'webhook event id reused with different payload');
END;
CREATE TRIGGER webhook_verified_promotes_rejected
BEFORE INSERT ON webhook_inbox
WHEN NEW.status = 'verified'
 AND EXISTS (
    SELECT 1 FROM webhook_inbox existing
    WHERE existing.tenant_id = NEW.tenant_id
      AND existing.endpoint_id = NEW.endpoint_id
      AND existing.provider_event_id = NEW.provider_event_id
      AND existing.payload_hash = NEW.payload_hash
      AND existing.status = 'rejected'
 )
BEGIN
    UPDATE webhook_inbox
       SET headers_json = NEW.headers_json,
           raw_payload = NEW.raw_payload,
           received_at = NEW.received_at,
           status = 'verified',
           canonical_event_type = NULL,
           error_classification = NULL
     WHERE tenant_id = NEW.tenant_id
       AND endpoint_id = NEW.endpoint_id
       AND provider_event_id = NEW.provider_event_id
       AND payload_hash = NEW.payload_hash
       AND status = 'rejected';
END;

CREATE TABLE webhook_dead_letters (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    inbox_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    replay_count INTEGER NOT NULL DEFAULT 0 CHECK (replay_count >= 0),
    created_at TEXT NOT NULL,
    replayed_at TEXT,
    FOREIGN KEY (inbox_id) REFERENCES webhook_inbox(id)
);

CREATE TABLE integration_deposits (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    authority_kind TEXT NOT NULL CHECK (authority_kind IN ('order', 'settlement')),
    authority_id TEXT NOT NULL,
    expected_amount_minor INTEGER NOT NULL CHECK (expected_amount_minor >= 0),
    currency TEXT NOT NULL CHECK (length(currency) = 3),
    state TEXT NOT NULL CHECK (state IN ('expected', 'recorded', 'held', 'partially_deducted', 'released', 'manual_resolution_required')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, authority_kind, authority_id)
);

CREATE TABLE integration_deposit_ledger (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    deposit_id TEXT NOT NULL,
    entry_type TEXT NOT NULL CHECK (entry_type IN ('expected', 'received', 'held', 'deducted', 'released', 'refund_completed', 'manual_adjustment')),
    amount_minor INTEGER NOT NULL,
    currency TEXT NOT NULL CHECK (length(currency) = 3),
    external_operation_id TEXT,
    audit_ref TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (tenant_id, deposit_id) REFERENCES integration_deposits(tenant_id, id),
    FOREIGN KEY (tenant_id, external_operation_id) REFERENCES external_operations(tenant_id, id)
);

-- Ledger append and aggregate transition must agree. In particular, a late
-- receipt cannot append money after the deposit has already left the
-- expected/recorded states even if application code forgets to inspect an
-- UPDATE row count.
CREATE TRIGGER integration_deposit_received_state_guard
BEFORE INSERT ON integration_deposit_ledger
WHEN NEW.entry_type = 'received'
 AND NOT EXISTS (
    SELECT 1 FROM integration_deposits d
    WHERE d.tenant_id = NEW.tenant_id
      AND d.id = NEW.deposit_id
      AND d.state IN ('expected', 'recorded')
 )
BEGIN
    SELECT RAISE(ABORT, 'deposit receipt is not allowed in the current state');
END;

CREATE TRIGGER integration_deposit_ledger_immutable_update
BEFORE UPDATE ON integration_deposit_ledger
BEGIN
    SELECT RAISE(ABORT, 'integration deposit ledger is immutable');
END;
CREATE TRIGGER integration_deposit_ledger_immutable_delete
BEFORE DELETE ON integration_deposit_ledger
BEGIN
    SELECT RAISE(ABORT, 'integration deposit ledger is immutable');
END;

CREATE TABLE refund_intents (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    deposit_id TEXT NOT NULL,
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    currency TEXT NOT NULL CHECK (length(currency) = 3),
    idempotency_key TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('requested', 'approved', 'dispatching', 'unknown_outcome', 'completed', 'failed', 'manual_resolution_required')),
    external_operation_id TEXT,
    reason TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, deposit_id, idempotency_key),
    FOREIGN KEY (tenant_id, deposit_id) REFERENCES integration_deposits(tenant_id, id),
    FOREIGN KEY (tenant_id, external_operation_id) REFERENCES external_operations(tenant_id, id)
);

-- The dedicated refund planner uses refund:<refund-id> as its stable identity.
-- Only that path is atomically linked here; generic fixture operations may use
-- operation_type=refund without claiming RefundIntent authority.
CREATE TRIGGER integration_refund_operation_atomic_link
AFTER INSERT ON external_operations
WHEN NEW.operation_type = 'refund' AND NEW.idempotency_key LIKE 'refund:%'
BEGIN
    UPDATE refund_intents
       SET state = 'approved',
           external_operation_id = NEW.id,
           updated_at = NEW.created_at
     WHERE tenant_id = NEW.tenant_id
       AND id = substr(NEW.idempotency_key, 8)
       AND state = 'requested';
    SELECT CASE WHEN changes() <> 1
        THEN RAISE(ABORT, 'refund operation has no matching requested refund intent')
    END;
END;

CREATE INDEX idx_provider_instances_tenant ON provider_instances(tenant_id, lifecycle, health);
CREATE INDEX idx_provider_bindings_capability ON provider_bindings(tenant_id, capability_id, enabled);
CREATE INDEX idx_external_operations_state ON external_operations(tenant_id, state, next_retry_at);
CREATE INDEX idx_external_attempts_operation ON external_operation_attempts(tenant_id, operation_id, attempt_number);
CREATE INDEX idx_webhook_inbox_pending ON webhook_inbox(tenant_id, status, received_at);
CREATE INDEX idx_refund_intents_state ON refund_intents(tenant_id, state, updated_at);
