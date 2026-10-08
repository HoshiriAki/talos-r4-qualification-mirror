CREATE TABLE workflow_instances (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, source_kind TEXT NOT NULL, source_id TEXT NOT NULL,
    definition_id TEXT NOT NULL, definition_version BIGINT NOT NULL CHECK (definition_version > 0), definition_hash TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active','blocked','manual_review','completed','cancelled')),
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    UNIQUE (tenant_id, source_kind, source_id, definition_id)
);
CREATE TABLE workflow_steps (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, workflow_instance_id TEXT NOT NULL REFERENCES workflow_instances(id),
    step_key TEXT NOT NULL, sequence_no BIGINT NOT NULL CHECK (sequence_no >= 0),
    state TEXT NOT NULL CHECK (state IN ('pending','running','succeeded','retry_scheduled','blocked','compensating','manual_review')),
    attempt_count BIGINT NOT NULL DEFAULT 0 CHECK (attempt_count >= 0), next_eligible_at TEXT,
    lease_owner TEXT, lease_expires_at TEXT, last_error TEXT, idempotency_key TEXT NOT NULL,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    UNIQUE (tenant_id, workflow_instance_id, step_key), UNIQUE (tenant_id, idempotency_key)
);
CREATE TABLE workflow_blockers (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, workflow_instance_id TEXT NOT NULL REFERENCES workflow_instances(id),
    step_key TEXT NOT NULL, blocker_code TEXT NOT NULL, detail TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('open','resolved')), created_at TEXT NOT NULL, resolved_at TEXT,
    UNIQUE (tenant_id, workflow_instance_id, step_key, blocker_code, status)
);
CREATE TABLE manual_decision_tasks (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, workflow_instance_id TEXT NOT NULL REFERENCES workflow_instances(id),
    step_key TEXT NOT NULL, decision_code TEXT NOT NULL, detail TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('open','resolved','cancelled')), decision TEXT, created_at TEXT NOT NULL, resolved_at TEXT,
    UNIQUE (tenant_id, workflow_instance_id, step_key, decision_code, status)
);
CREATE TABLE domain_outbox (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, source_kind TEXT NOT NULL, source_id TEXT NOT NULL,
    message_type TEXT NOT NULL, idempotency_key TEXT NOT NULL, payload_json TEXT NOT NULL,
    payload_version BIGINT NOT NULL CHECK (payload_version > 0), state TEXT NOT NULL CHECK (state IN ('pending','processing','delivered','failed')),
    attempt_count BIGINT NOT NULL DEFAULT 0 CHECK (attempt_count >= 0), available_at TEXT NOT NULL, created_at TEXT NOT NULL,
    delivered_at TEXT, last_error TEXT, UNIQUE (tenant_id, idempotency_key)
);
CREATE TABLE domain_inbox (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, message_id TEXT NOT NULL, message_type TEXT NOT NULL,
    payload_version BIGINT NOT NULL CHECK (payload_version > 0), received_at TEXT NOT NULL, processed_at TEXT,
    state TEXT NOT NULL CHECK (state IN ('received','processed','failed')), last_error TEXT, UNIQUE (tenant_id, message_id)
);
CREATE INDEX idx_workflow_steps_due ON workflow_steps (tenant_id, state, next_eligible_at, lease_expires_at, sequence_no);
CREATE INDEX idx_workflow_instances_source ON workflow_instances (tenant_id, source_kind, source_id);
CREATE INDEX idx_workflow_blockers_open ON workflow_blockers (tenant_id, status, workflow_instance_id);
CREATE INDEX idx_manual_decision_tasks_open ON manual_decision_tasks (tenant_id, status, workflow_instance_id);
CREATE INDEX idx_domain_outbox_due ON domain_outbox (tenant_id, state, available_at, created_at);
CREATE INDEX idx_domain_inbox_state ON domain_inbox (tenant_id, state, received_at);
