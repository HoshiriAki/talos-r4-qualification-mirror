-- 059: tenant-scoped Provider Integration Fabric.  Provider secret values are
-- never stored in these authority tables; only secret refs are persisted.

CREATE TABLE provider_manifests (
    provider_id TEXT NOT NULL, version TEXT NOT NULL,
    capabilities_json TEXT NOT NULL, config_schema_json TEXT NOT NULL, secret_schema_json TEXT NOT NULL,
    api_versions_json TEXT NOT NULL, webhook_types_json TEXT NOT NULL, simulation_capabilities_json TEXT NOT NULL,
    readiness TEXT NOT NULL CHECK (readiness IN ('stub', 'fixture', 'sandbox', 'production')),
    compatibility_json TEXT NOT NULL, created_at TEXT NOT NULL,
    PRIMARY KEY (provider_id, version)
);
CREATE TABLE provider_instances (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, provider_id TEXT NOT NULL, manifest_version TEXT NOT NULL,
    config_revision TEXT NOT NULL, config_json TEXT NOT NULL, secret_refs_json TEXT NOT NULL,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('draft', 'active', 'suspended', 'retired')),
    readiness TEXT NOT NULL CHECK (readiness IN ('stub', 'fixture', 'sandbox', 'production')),
    health TEXT NOT NULL CHECK (health IN ('unknown', 'ready', 'degraded', 'unhealthy')),
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (provider_id, manifest_version) REFERENCES provider_manifests(provider_id, version)
);

CREATE OR REPLACE FUNCTION integration_guard_provider_instance_update() RETURNS trigger AS $$
BEGIN
    IF NEW.provider_id IS DISTINCT FROM OLD.provider_id
       OR NEW.manifest_version IS DISTINCT FROM OLD.manifest_version THEN
        RAISE EXCEPTION 'provider instance identity is immutable';
    END IF;
    IF NEW.config_revision IS NOT DISTINCT FROM OLD.config_revision
       AND (
          NEW.config_json IS DISTINCT FROM OLD.config_json
          OR NEW.secret_refs_json IS DISTINCT FROM OLD.secret_refs_json
          OR NEW.readiness IS DISTINCT FROM OLD.readiness
       ) THEN
        RAISE EXCEPTION 'provider instance configuration change requires a new revision';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER provider_instance_revision_guard
BEFORE UPDATE ON provider_instances
FOR EACH ROW EXECUTE FUNCTION integration_guard_provider_instance_update();

CREATE TABLE provider_bindings (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, provider_instance_id TEXT NOT NULL,
    capability_id TEXT NOT NULL, config_revision TEXT NOT NULL,
    enabled BOOLEAN NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (tenant_id, capability_id),
    FOREIGN KEY (tenant_id, provider_instance_id) REFERENCES provider_instances(tenant_id, id)
);

CREATE OR REPLACE FUNCTION integration_guard_provider_binding_update() RETURNS trigger AS $$
BEGIN
    IF NEW.provider_instance_id IS DISTINCT FROM OLD.provider_instance_id
       OR NEW.capability_id IS DISTINCT FROM OLD.capability_id THEN
        RAISE EXCEPTION 'provider binding identity is immutable';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER provider_binding_identity_immutable
BEFORE UPDATE ON provider_bindings
FOR EACH ROW EXECUTE FUNCTION integration_guard_provider_binding_update();

CREATE TABLE provider_binding_history (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, binding_id TEXT NOT NULL, revision TEXT NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('created', 'updated', 'disabled', 'enabled', 'retired')),
    occurred_at TEXT NOT NULL, actor_ref TEXT NOT NULL,
    UNIQUE (tenant_id, binding_id, revision, action)
);
CREATE TABLE external_operations (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, provider_instance_id TEXT NOT NULL, binding_id TEXT NOT NULL,
    binding_revision TEXT NOT NULL, capability_id TEXT NOT NULL, operation_type TEXT NOT NULL,
    idempotency_key TEXT NOT NULL, request_hash TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('planned', 'ready', 'dispatching', 'succeeded', 'rejected', 'retryable_failure', 'non_retryable_failure', 'unknown_outcome', 'reconciling', 'resolved', 'manual_resolution_required')),
    attempt_count BIGINT NOT NULL DEFAULT 0 CHECK (attempt_count >= 0), next_retry_at TEXT,
    result_ref TEXT, classification TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (tenant_id, binding_id, idempotency_key),
    FOREIGN KEY (tenant_id, binding_id) REFERENCES provider_bindings(tenant_id, id)
);

CREATE OR REPLACE FUNCTION integration_guard_external_operation_intent() RETURNS trigger AS $$
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
       OR NEW.provider_instance_id IS DISTINCT FROM OLD.provider_instance_id
       OR NEW.binding_id IS DISTINCT FROM OLD.binding_id
       OR NEW.binding_revision IS DISTINCT FROM OLD.binding_revision
       OR NEW.capability_id IS DISTINCT FROM OLD.capability_id
       OR NEW.operation_type IS DISTINCT FROM OLD.operation_type
       OR NEW.idempotency_key IS DISTINCT FROM OLD.idempotency_key
       OR NEW.request_hash IS DISTINCT FROM OLD.request_hash THEN
        RAISE EXCEPTION 'external operation intent is immutable';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER external_operation_intent_immutable
BEFORE UPDATE ON external_operations
FOR EACH ROW EXECUTE FUNCTION integration_guard_external_operation_intent();

CREATE TABLE external_operation_attempts (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, operation_id TEXT NOT NULL,
    attempt_number BIGINT NOT NULL CHECK (attempt_number > 0), state TEXT NOT NULL,
    classification TEXT, request_hash TEXT NOT NULL, provider_result_ref TEXT,
    started_at TEXT NOT NULL, completed_at TEXT,
    UNIQUE (tenant_id, operation_id, attempt_number),
    FOREIGN KEY (tenant_id, operation_id) REFERENCES external_operations(tenant_id, id)
);
CREATE TABLE external_operation_reconciliations (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, operation_id TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('pending', 'effect_confirmed', 'effect_absent', 'manual_required')),
    evidence_ref TEXT, resolved_by TEXT, created_at TEXT NOT NULL, resolved_at TEXT,
    FOREIGN KEY (tenant_id, operation_id) REFERENCES external_operations(tenant_id, id)
);
CREATE TABLE integration_circuit_state (
    tenant_id TEXT NOT NULL, binding_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('closed', 'open', 'half_open')),
    failure_count BIGINT NOT NULL DEFAULT 0 CHECK (failure_count >= 0), opened_until TEXT, updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, binding_id),
    FOREIGN KEY (tenant_id, binding_id) REFERENCES provider_bindings(tenant_id, id)
);
CREATE TABLE webhook_endpoints (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, binding_id TEXT NOT NULL, provider_id TEXT NOT NULL,
    token_hash TEXT NOT NULL, enabled BOOLEAN NOT NULL, created_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (token_hash),
    FOREIGN KEY (tenant_id, binding_id) REFERENCES provider_bindings(tenant_id, id)
);
CREATE TABLE webhook_inbox (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, endpoint_id TEXT NOT NULL, provider_event_id TEXT NOT NULL,
    payload_hash TEXT NOT NULL, headers_json TEXT NOT NULL, raw_payload BYTEA NOT NULL,
    received_at TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('verified', 'processing', 'processed', 'rejected', 'dead_letter')),
    canonical_event_type TEXT, error_classification TEXT,
    UNIQUE (tenant_id, endpoint_id, provider_event_id),
    FOREIGN KEY (tenant_id, endpoint_id) REFERENCES webhook_endpoints(tenant_id, id)
);

CREATE OR REPLACE FUNCTION integration_webhook_identity_guard() RETURNS trigger AS $$
DECLARE
    existing_hash TEXT;
    existing_status TEXT;
BEGIN
    SELECT payload_hash, status
      INTO existing_hash, existing_status
      FROM webhook_inbox
     WHERE tenant_id = NEW.tenant_id
       AND endpoint_id = NEW.endpoint_id
       AND provider_event_id = NEW.provider_event_id;

    IF FOUND THEN
        IF existing_hash IS DISTINCT FROM NEW.payload_hash THEN
            RAISE EXCEPTION 'webhook event id reused with different payload';
        END IF;
        IF NEW.status = 'verified' AND existing_status = 'rejected' THEN
            UPDATE webhook_inbox
               SET headers_json = NEW.headers_json,
                   raw_payload = NEW.raw_payload,
                   received_at = NEW.received_at,
                   status = 'verified',
                   canonical_event_type = NULL,
                   error_classification = NULL
             WHERE tenant_id = NEW.tenant_id
               AND endpoint_id = NEW.endpoint_id
               AND provider_event_id = NEW.provider_event_id;
            RETURN NULL;
        END IF;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER webhook_event_identity_guard
BEFORE INSERT ON webhook_inbox
FOR EACH ROW EXECUTE FUNCTION integration_webhook_identity_guard();

CREATE TABLE webhook_dead_letters (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, inbox_id TEXT NOT NULL, reason TEXT NOT NULL,
    replay_count BIGINT NOT NULL DEFAULT 0 CHECK (replay_count >= 0), created_at TEXT NOT NULL, replayed_at TEXT,
    FOREIGN KEY (inbox_id) REFERENCES webhook_inbox(id)
);
CREATE TABLE integration_deposits (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL,
    authority_kind TEXT NOT NULL CHECK (authority_kind IN ('order', 'settlement')), authority_id TEXT NOT NULL,
    expected_amount_minor BIGINT NOT NULL CHECK (expected_amount_minor >= 0),
    currency TEXT NOT NULL CHECK (char_length(currency) = 3),
    state TEXT NOT NULL CHECK (state IN ('expected', 'recorded', 'held', 'partially_deducted', 'released', 'manual_resolution_required')),
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (tenant_id, authority_kind, authority_id)
);
CREATE TABLE integration_deposit_ledger (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, deposit_id TEXT NOT NULL,
    entry_type TEXT NOT NULL CHECK (entry_type IN ('expected', 'received', 'held', 'deducted', 'released', 'refund_completed', 'manual_adjustment')),
    amount_minor BIGINT NOT NULL, currency TEXT NOT NULL CHECK (char_length(currency) = 3),
    external_operation_id TEXT, audit_ref TEXT NOT NULL, created_at TEXT NOT NULL,
    FOREIGN KEY (tenant_id, deposit_id) REFERENCES integration_deposits(tenant_id, id),
    FOREIGN KEY (tenant_id, external_operation_id) REFERENCES external_operations(tenant_id, id)
);

CREATE OR REPLACE FUNCTION integration_guard_deposit_ledger_append() RETURNS trigger AS $$
BEGIN
    IF NEW.entry_type = 'received'
       AND NOT EXISTS (
          SELECT 1 FROM integration_deposits d
           WHERE d.tenant_id = NEW.tenant_id
             AND d.id = NEW.deposit_id
             AND d.state IN ('expected', 'recorded')
       ) THEN
        RAISE EXCEPTION 'deposit receipt is not allowed in the current state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER integration_deposit_received_state_guard
BEFORE INSERT ON integration_deposit_ledger
FOR EACH ROW EXECUTE FUNCTION integration_guard_deposit_ledger_append();

CREATE OR REPLACE FUNCTION integration_prevent_ledger_mutation() RETURNS trigger AS $$
BEGIN RAISE EXCEPTION 'integration deposit ledger is immutable'; END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER integration_deposit_ledger_immutable_update BEFORE UPDATE ON integration_deposit_ledger
FOR EACH ROW EXECUTE FUNCTION integration_prevent_ledger_mutation();
CREATE TRIGGER integration_deposit_ledger_immutable_delete BEFORE DELETE ON integration_deposit_ledger
FOR EACH ROW EXECUTE FUNCTION integration_prevent_ledger_mutation();
CREATE TABLE refund_intents (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, deposit_id TEXT NOT NULL,
    amount_minor BIGINT NOT NULL CHECK (amount_minor > 0), currency TEXT NOT NULL CHECK (char_length(currency) = 3),
    idempotency_key TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('requested', 'approved', 'dispatching', 'unknown_outcome', 'completed', 'failed', 'manual_resolution_required')),
    external_operation_id TEXT, reason TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (tenant_id, deposit_id, idempotency_key),
    FOREIGN KEY (tenant_id, deposit_id) REFERENCES integration_deposits(tenant_id, id),
    FOREIGN KEY (tenant_id, external_operation_id) REFERENCES external_operations(tenant_id, id)
);

CREATE OR REPLACE FUNCTION integration_link_refund_operation() RETURNS trigger AS $$
DECLARE
    affected BIGINT;
BEGIN
    IF NEW.operation_type <> 'refund' OR NEW.idempotency_key NOT LIKE 'refund:%' THEN
        RETURN NEW;
    END IF;
    UPDATE refund_intents
       SET state = 'approved',
           external_operation_id = NEW.id,
           updated_at = NEW.created_at
     WHERE tenant_id = NEW.tenant_id
       AND id = substring(NEW.idempotency_key FROM 8)
       AND state = 'requested';
    GET DIAGNOSTICS affected = ROW_COUNT;
    IF affected <> 1 THEN
        RAISE EXCEPTION 'refund operation has no matching requested refund intent';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER integration_refund_operation_atomic_link
AFTER INSERT ON external_operations
FOR EACH ROW EXECUTE FUNCTION integration_link_refund_operation();

CREATE INDEX idx_provider_instances_tenant ON provider_instances(tenant_id, lifecycle, health);
CREATE INDEX idx_provider_bindings_capability ON provider_bindings(tenant_id, capability_id, enabled);
CREATE INDEX idx_external_operations_state ON external_operations(tenant_id, state, next_retry_at);
CREATE INDEX idx_external_attempts_operation ON external_operation_attempts(tenant_id, operation_id, attempt_number);
CREATE INDEX idx_webhook_inbox_pending ON webhook_inbox(tenant_id, status, received_at);
CREATE INDEX idx_refund_intents_state ON refund_intents(tenant_id, state, updated_at);
