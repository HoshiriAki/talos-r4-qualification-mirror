-- 060: durable execution evidence for the Stage 2 Integration Fabric.
--
-- This migration extends, rather than mutates, the 059 authority. The event
-- records deliberately contain classifications and identifiers only: neither
-- request payloads nor resolved Provider secrets belong in durable audit data.

CREATE TABLE external_operation_runtime_events (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    attempt_id TEXT,
    event_type TEXT NOT NULL CHECK (event_type IN (
        'claimed', 'retry_scheduled', 'unknown_outcome',
        'recovered_after_restart', 'circuit_opened', 'circuit_closed',
        'binding_revision_mismatch', 'rate_limited', 'concurrency_limited',
        'reconciled'
    )),
    classification TEXT,
    occurred_at TEXT NOT NULL,
    FOREIGN KEY (tenant_id, operation_id) REFERENCES external_operations(tenant_id, id)
);

CREATE TABLE webhook_processing_attempts (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    inbox_id TEXT NOT NULL,
    attempt_number INTEGER NOT NULL CHECK (attempt_number > 0),
    state TEXT NOT NULL CHECK (state IN ('processing', 'processed', 'retryable_failure', 'dead_letter')),
    canonical_event_type TEXT,
    classification TEXT,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    UNIQUE (tenant_id, inbox_id, attempt_number),
    FOREIGN KEY (inbox_id) REFERENCES webhook_inbox(id)
);

CREATE TABLE webhook_replay_audit (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    inbox_id TEXT NOT NULL,
    actor_ref TEXT NOT NULL,
    reason TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    FOREIGN KEY (inbox_id) REFERENCES webhook_inbox(id)
);

CREATE TABLE integration_deposit_reconciliations (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    deposit_id TEXT NOT NULL,
    refund_id TEXT,
    external_operation_id TEXT,
    outcome TEXT NOT NULL CHECK (outcome IN ('pending', 'matched', 'unknown', 'manual_required')),
    evidence_ref TEXT NOT NULL,
    resolved_by TEXT,
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    FOREIGN KEY (tenant_id, deposit_id) REFERENCES integration_deposits(tenant_id, id),
    FOREIGN KEY (tenant_id, refund_id) REFERENCES refund_intents(tenant_id, id),
    FOREIGN KEY (tenant_id, external_operation_id) REFERENCES external_operations(tenant_id, id)
);

-- Provider revision/availability changes invalidate the admission snapshot of
-- already queued work. Such work must not silently resume if an operator later
-- re-enables the Provider or advances its revision: it requires an explicit new
-- authorization or reconciliation decision.
CREATE TRIGGER integration_instance_invalidates_queued_operations
AFTER UPDATE OF config_revision, lifecycle, health, readiness ON provider_instances
WHEN NEW.config_revision <> OLD.config_revision
  OR NEW.lifecycle <> 'active'
  OR NEW.health <> 'ready'
  OR NEW.readiness <> 'fixture'
BEGIN
    UPDATE external_operations
       SET state = 'manual_resolution_required',
           classification = CASE
               WHEN NEW.config_revision <> OLD.config_revision
                   THEN 'provider_instance_revision_changed'
               ELSE 'provider_instance_availability_changed'
           END,
           next_retry_at = NULL,
           updated_at = NEW.updated_at
     WHERE tenant_id = NEW.tenant_id
       AND provider_instance_id = NEW.id
       AND state IN ('ready', 'retryable_failure');
END;

CREATE TRIGGER integration_binding_invalidates_queued_operations
AFTER UPDATE OF config_revision, enabled ON provider_bindings
WHEN NEW.config_revision <> OLD.config_revision OR NEW.enabled = 0
BEGIN
    UPDATE external_operations
       SET state = 'manual_resolution_required',
           classification = CASE
               WHEN NEW.config_revision <> OLD.config_revision
                   THEN 'provider_binding_revision_changed'
               ELSE 'provider_binding_disabled'
           END,
           next_retry_at = NULL,
           updated_at = NEW.updated_at
     WHERE tenant_id = NEW.tenant_id
       AND binding_id = NEW.id
       AND state IN ('ready', 'retryable_failure');
END;

CREATE TRIGGER integration_manifest_invalidates_queued_operations
AFTER UPDATE OF readiness ON provider_manifests
WHEN NEW.readiness <> 'fixture'
BEGIN
    UPDATE external_operations
       SET state = 'manual_resolution_required',
           classification = 'provider_manifest_readiness_changed',
           next_retry_at = NULL,
           updated_at = NEW.created_at
     WHERE state IN ('ready', 'retryable_failure')
       AND EXISTS (
          SELECT 1 FROM provider_instances i
           WHERE i.tenant_id = external_operations.tenant_id
             AND i.id = external_operations.provider_instance_id
             AND i.provider_id = NEW.provider_id
             AND i.manifest_version = NEW.version
       );
END;

-- Circuit state is transient runtime health, not an authority/configuration
-- change. Open circuits therefore defer work only until their persisted expiry.
CREATE TRIGGER integration_circuit_open_defers_operations_insert
AFTER INSERT ON integration_circuit_state
WHEN NEW.state = 'open' AND NEW.opened_until IS NOT NULL
BEGIN
    UPDATE external_operations
       SET next_retry_at = NEW.opened_until
     WHERE tenant_id = NEW.tenant_id
       AND binding_id = NEW.binding_id
       AND state IN ('ready', 'retryable_failure')
       AND (next_retry_at IS NULL OR next_retry_at < NEW.opened_until);
END;
CREATE TRIGGER integration_circuit_open_defers_operations_update
AFTER UPDATE OF state, opened_until ON integration_circuit_state
WHEN NEW.state = 'open' AND NEW.opened_until IS NOT NULL
BEGIN
    UPDATE external_operations
       SET next_retry_at = NEW.opened_until
     WHERE tenant_id = NEW.tenant_id
       AND binding_id = NEW.binding_id
       AND state IN ('ready', 'retryable_failure')
       AND (next_retry_at IS NULL OR next_retry_at < NEW.opened_until);
END;

-- A half-open circuit admits exactly one probe. Normal concurrency policy may
-- allow more than one in-flight request, but half-open recovery must not fan
-- out until the probe has proved the Provider healthy again.
CREATE TRIGGER integration_half_open_single_probe
BEFORE UPDATE OF state ON external_operations
WHEN NEW.state = 'dispatching'
 AND OLD.state <> 'dispatching'
 AND EXISTS (
    SELECT 1 FROM integration_circuit_state c
     WHERE c.tenant_id = NEW.tenant_id
       AND c.binding_id = NEW.binding_id
       AND c.state = 'half_open'
 )
 AND EXISTS (
    SELECT 1 FROM external_operations other
     WHERE other.tenant_id = NEW.tenant_id
       AND other.binding_id = NEW.binding_id
       AND other.id <> NEW.id
       AND other.state = 'dispatching'
 )
BEGIN
    SELECT RAISE(ABORT, 'half-open circuit already has a probe in flight');
END;

CREATE INDEX idx_external_operation_runtime_events_operation
    ON external_operation_runtime_events(tenant_id, operation_id, occurred_at);
CREATE INDEX idx_webhook_processing_attempts_inbox
    ON webhook_processing_attempts(tenant_id, inbox_id, attempt_number);
CREATE INDEX idx_deposit_reconciliations_pending
    ON integration_deposit_reconciliations(tenant_id, outcome, created_at);
