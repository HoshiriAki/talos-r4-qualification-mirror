-- 060: durable execution evidence for the Stage 2 Integration Fabric (PG).
-- No payload or resolved-secret column is introduced by this migration.

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
    attempt_number BIGINT NOT NULL CHECK (attempt_number > 0),
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
-- queued external effects. Re-enabling or advancing a Provider never silently
-- resurrects old authorization; an operator must explicitly reconcile or
-- create new work.
CREATE OR REPLACE FUNCTION integration_invalidate_instance_operations() RETURNS trigger AS $$
BEGIN
    IF NEW.config_revision IS DISTINCT FROM OLD.config_revision
       OR NEW.lifecycle <> 'active'
       OR NEW.health <> 'ready'
       OR NEW.readiness <> 'fixture' THEN
        UPDATE external_operations
           SET state = 'manual_resolution_required',
               classification = CASE
                   WHEN NEW.config_revision IS DISTINCT FROM OLD.config_revision
                       THEN 'provider_instance_revision_changed'
                   ELSE 'provider_instance_availability_changed'
               END,
               next_retry_at = NULL,
               updated_at = NEW.updated_at
         WHERE tenant_id = NEW.tenant_id
           AND provider_instance_id = NEW.id
           AND state IN ('ready', 'retryable_failure');
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER integration_instance_invalidates_queued_operations
AFTER UPDATE OF config_revision, lifecycle, health, readiness ON provider_instances
FOR EACH ROW EXECUTE FUNCTION integration_invalidate_instance_operations();

CREATE OR REPLACE FUNCTION integration_invalidate_binding_operations() RETURNS trigger AS $$
BEGIN
    IF NEW.config_revision IS DISTINCT FROM OLD.config_revision OR NOT NEW.enabled THEN
        UPDATE external_operations
           SET state = 'manual_resolution_required',
               classification = CASE
                   WHEN NEW.config_revision IS DISTINCT FROM OLD.config_revision
                       THEN 'provider_binding_revision_changed'
                   ELSE 'provider_binding_disabled'
               END,
               next_retry_at = NULL,
               updated_at = NEW.updated_at
         WHERE tenant_id = NEW.tenant_id
           AND binding_id = NEW.id
           AND state IN ('ready', 'retryable_failure');
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER integration_binding_invalidates_queued_operations
AFTER UPDATE OF config_revision, enabled ON provider_bindings
FOR EACH ROW EXECUTE FUNCTION integration_invalidate_binding_operations();

CREATE OR REPLACE FUNCTION integration_invalidate_manifest_operations() RETURNS trigger AS $$
BEGIN
    IF NEW.readiness <> 'fixture' THEN
        UPDATE external_operations o
           SET state = 'manual_resolution_required',
               classification = 'provider_manifest_readiness_changed',
               next_retry_at = NULL,
               updated_at = NEW.created_at
         WHERE o.state IN ('ready', 'retryable_failure')
           AND EXISTS (
              SELECT 1 FROM provider_instances i
               WHERE i.tenant_id = o.tenant_id
                 AND i.id = o.provider_instance_id
                 AND i.provider_id = NEW.provider_id
                 AND i.manifest_version = NEW.version
           );
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER integration_manifest_invalidates_queued_operations
AFTER UPDATE OF readiness ON provider_manifests
FOR EACH ROW EXECUTE FUNCTION integration_invalidate_manifest_operations();

-- Circuit health is transient and may recover without a new authority decision.
CREATE OR REPLACE FUNCTION integration_project_circuit_open() RETURNS trigger AS $$
BEGIN
    IF NEW.state = 'open' AND NEW.opened_until IS NOT NULL THEN
        UPDATE external_operations
           SET next_retry_at = NEW.opened_until
         WHERE tenant_id = NEW.tenant_id
           AND binding_id = NEW.binding_id
           AND state IN ('ready', 'retryable_failure')
           AND (next_retry_at IS NULL OR next_retry_at < NEW.opened_until);
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER integration_circuit_open_projection_insert
AFTER INSERT ON integration_circuit_state
FOR EACH ROW EXECUTE FUNCTION integration_project_circuit_open();
CREATE TRIGGER integration_circuit_open_projection_update
AFTER UPDATE OF state, opened_until ON integration_circuit_state
FOR EACH ROW EXECUTE FUNCTION integration_project_circuit_open();

-- Half-open recovery is intentionally a one-probe state even when normal
-- per-binding concurrency allows multiple in-flight requests.
CREATE OR REPLACE FUNCTION integration_guard_half_open_probe() RETURNS trigger AS $$
BEGIN
    IF NEW.state = 'dispatching'
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
       ) THEN
        RAISE EXCEPTION 'half-open circuit already has a probe in flight';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER integration_half_open_single_probe
BEFORE UPDATE OF state ON external_operations
FOR EACH ROW EXECUTE FUNCTION integration_guard_half_open_probe();

CREATE INDEX idx_external_operation_runtime_events_operation
    ON external_operation_runtime_events(tenant_id, operation_id, occurred_at);
CREATE INDEX idx_webhook_processing_attempts_inbox
    ON webhook_processing_attempts(tenant_id, inbox_id, attempt_number);
CREATE INDEX idx_deposit_reconciliations_pending
    ON integration_deposit_reconciliations(tenant_id, outcome, created_at);