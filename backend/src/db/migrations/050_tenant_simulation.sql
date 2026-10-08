-- MVP3 isolated simulation overlay. Evidence intentionally has no tenant FK so
-- tenant lifecycle operations can never cascade away retained audit evidence.
CREATE TABLE IF NOT EXISTS simulation_reference_configs (
    tenant_id TEXT NOT NULL,
    key TEXT NOT NULL CHECK(length(key) BETWEEN 1 AND 64),
    value_json TEXT NOT NULL,
    resource_version INTEGER NOT NULL CHECK(resource_version > 0),
    updated_at TEXT NOT NULL,
    PRIMARY KEY(tenant_id, key)
);
CREATE TABLE IF NOT EXISTS simulation_reference_resource_revisions (
    tenant_id TEXT NOT NULL, key TEXT NOT NULL,
    resource_version INTEGER NOT NULL CHECK(resource_version > 0),
    present INTEGER NOT NULL CHECK(present IN (0,1)), updated_at TEXT NOT NULL,
    PRIMARY KEY(tenant_id, key)
);
CREATE TABLE IF NOT EXISTS simulation_tenant_revisions (
    tenant_id TEXT PRIMARY KEY, revision INTEGER NOT NULL CHECK(revision >= 0), updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS simulation_sessions (
    id TEXT NOT NULL, actor_id TEXT NOT NULL, target_tenant_id TEXT NOT NULL,
    namespace_id TEXT NOT NULL UNIQUE, base_revision INTEGER NOT NULL,
    generation INTEGER NOT NULL DEFAULT 1 CHECK(generation >= 1),
    idempotency_key_hash TEXT NOT NULL, create_request_digest TEXT NOT NULL,
    scenario_name TEXT NOT NULL CHECK(length(scenario_name) BETWEEN 1 AND 80),
    change_intent TEXT NOT NULL CHECK(length(change_intent) BETWEEN 1 AND 500),
    status TEXT NOT NULL CHECK(status IN ('provisioning','active','expired','discarded','failed')),
    created_at TEXT NOT NULL, expires_at TEXT NOT NULL, provisioning_lease_until TEXT,
    discarded_at TEXT, failure_code TEXT,
    PRIMARY KEY(id, target_tenant_id), UNIQUE(actor_id, idempotency_key_hash),
    CHECK(julianday(expires_at) > julianday(created_at))
);
CREATE INDEX IF NOT EXISTS idx_simulation_sessions_actor_status ON simulation_sessions(actor_id,status,expires_at);
CREATE INDEX IF NOT EXISTS idx_simulation_sessions_tenant ON simulation_sessions(target_tenant_id,created_at DESC);
CREATE TABLE IF NOT EXISTS simulation_revision_evidence (
    simulation_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
    resource_kind TEXT NOT NULL, resource_key TEXT NOT NULL,
    base_presence INTEGER NOT NULL CHECK(base_presence IN (0,1)),
    captured_resource_version INTEGER NOT NULL,
    PRIMARY KEY(simulation_id,tenant_id,resource_kind,resource_key),
    FOREIGN KEY(simulation_id,tenant_id) REFERENCES simulation_sessions(id,target_tenant_id) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS simulation_base_documents (
    simulation_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
    resource_kind TEXT NOT NULL, resource_key TEXT NOT NULL,
    base_resource_version INTEGER NOT NULL, document_json TEXT NOT NULL,
    payload_policy TEXT NOT NULL DEFAULT 'REFERENCE_ONLY', captured_at TEXT NOT NULL,
    PRIMARY KEY(simulation_id,tenant_id,resource_kind,resource_key),
    FOREIGN KEY(simulation_id,tenant_id,resource_kind,resource_key)
      REFERENCES simulation_revision_evidence(simulation_id,tenant_id,resource_kind,resource_key) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS simulation_overlay_entries (
    simulation_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
    resource_kind TEXT NOT NULL, resource_key TEXT NOT NULL,
    operation TEXT NOT NULL CHECK(operation IN ('value','tombstone')),
    document_json TEXT, base_resource_version INTEGER NOT NULL,
    overlay_version INTEGER NOT NULL CHECK(overlay_version > 0),
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY(simulation_id,tenant_id,resource_kind,resource_key),
    FOREIGN KEY(simulation_id,tenant_id,resource_kind,resource_key)
      REFERENCES simulation_revision_evidence(simulation_id,tenant_id,resource_kind,resource_key) ON DELETE RESTRICT,
    CHECK((operation = 'value' AND document_json IS NOT NULL) OR (operation = 'tombstone' AND document_json IS NULL))
);
CREATE TABLE IF NOT EXISTS simulation_usage (
    simulation_id TEXT NOT NULL, tenant_id TEXT NOT NULL, overlay_rows INTEGER NOT NULL DEFAULT 0,
    document_bytes INTEGER NOT NULL DEFAULT 0, effect_count INTEGER NOT NULL DEFAULT 0,
    evaluation_count INTEGER NOT NULL DEFAULT 0, evidence_bytes INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(simulation_id,tenant_id),
    FOREIGN KEY(simulation_id,tenant_id) REFERENCES simulation_sessions(id,target_tenant_id) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS simulation_command_results (
    simulation_id TEXT NOT NULL, tenant_id TEXT NOT NULL, actor_id TEXT NOT NULL, command TEXT NOT NULL,
    command_idempotency_key_hash TEXT NOT NULL, request_digest TEXT NOT NULL,
    response_status INTEGER NOT NULL, response_json TEXT NOT NULL, created_at TEXT NOT NULL,
    PRIMARY KEY(simulation_id,actor_id,command,command_idempotency_key_hash),
    FOREIGN KEY(simulation_id,tenant_id) REFERENCES simulation_sessions(id,target_tenant_id) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS simulation_effect_records (
    id TEXT PRIMARY KEY, simulation_id TEXT NOT NULL, tenant_id TEXT NOT NULL, actor_id TEXT NOT NULL,
    command TEXT NOT NULL, command_idempotency_key_hash TEXT NOT NULL, request_digest TEXT NOT NULL,
    effect_ordinal INTEGER NOT NULL, effect_kind TEXT NOT NULL, payload_policy TEXT NOT NULL,
    payload_preview TEXT NOT NULL, deterministic_result TEXT NOT NULL, correlation_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(simulation_id,actor_id,command,command_idempotency_key_hash,effect_ordinal),
    FOREIGN KEY(simulation_id,tenant_id) REFERENCES simulation_sessions(id,target_tenant_id) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS simulation_diff_evidence (
    simulation_id TEXT NOT NULL, tenant_id TEXT NOT NULL, evaluation_id TEXT NOT NULL,
    evaluated_revision INTEGER NOT NULL, generation INTEGER NOT NULL, overlay_digest TEXT NOT NULL,
    item_count INTEGER NOT NULL, total_bytes INTEGER NOT NULL, digest TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('ready','pending','failed')), created_at TEXT NOT NULL,
    PRIMARY KEY(simulation_id,tenant_id,evaluation_id),
    UNIQUE(simulation_id,generation,evaluated_revision,overlay_digest),
    FOREIGN KEY(simulation_id,tenant_id) REFERENCES simulation_sessions(id,target_tenant_id) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS simulation_diff_evidence_items (
    simulation_id TEXT NOT NULL, tenant_id TEXT NOT NULL, evaluation_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL, canonical_item_json TEXT NOT NULL, item_bytes INTEGER NOT NULL,
    PRIMARY KEY(simulation_id,tenant_id,evaluation_id,ordinal),
    FOREIGN KEY(simulation_id,tenant_id,evaluation_id) REFERENCES simulation_diff_evidence(simulation_id,tenant_id,evaluation_id) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS simulation_terminal_inputs (
    simulation_id TEXT NOT NULL, tenant_id TEXT NOT NULL, generation INTEGER NOT NULL,
    evaluated_revision INTEGER NOT NULL, overlay_digest TEXT NOT NULL, item_count INTEGER NOT NULL,
    input_bytes INTEGER NOT NULL, created_at TEXT NOT NULL,
    PRIMARY KEY(simulation_id,tenant_id,generation),
    FOREIGN KEY(simulation_id,tenant_id) REFERENCES simulation_sessions(id,target_tenant_id) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS simulation_terminal_input_items (
    simulation_id TEXT NOT NULL, tenant_id TEXT NOT NULL, generation INTEGER NOT NULL,
    ordinal INTEGER NOT NULL, canonical_input_json TEXT NOT NULL, item_bytes INTEGER NOT NULL,
    PRIMARY KEY(simulation_id,tenant_id,generation,ordinal),
    FOREIGN KEY(simulation_id,tenant_id,generation) REFERENCES simulation_terminal_inputs(simulation_id,tenant_id,generation) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS simulation_cleanup_jobs (
    simulation_id TEXT NOT NULL, tenant_id TEXT NOT NULL, job_kind TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','running','complete','failed')), attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL, last_error_code TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY(simulation_id,tenant_id,job_kind),
    FOREIGN KEY(simulation_id,tenant_id) REFERENCES simulation_sessions(id,target_tenant_id) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS simulation_rate_buckets (
    actor_id TEXT NOT NULL, simulation_id TEXT NOT NULL, operation_class TEXT NOT NULL,
    window_start TEXT NOT NULL, request_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(actor_id,simulation_id,operation_class,window_start)
);
