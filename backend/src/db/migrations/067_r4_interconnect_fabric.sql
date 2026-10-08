-- Migration 067: R4 TALOS Interconnect Fabric schema parity (SQLite)
-- SQLite remains prototype/test compatibility. PostgreSQL 18 is authoritative
-- for the production durable driver.

CREATE TABLE IF NOT EXISTS interconnect_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT NOT NULL UNIQUE,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    subject TEXT NOT NULL,
    contract_ref TEXT NOT NULL,
    contract_version TEXT NOT NULL,
    schema_ref TEXT NOT NULL,
    correlation_id TEXT NOT NULL,
    causation_id TEXT,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    deadline_ms INTEGER CHECK (deadline_ms IS NULL OR deadline_ms >= created_at_ms),
    ordering_key TEXT,
    idempotency_key TEXT,
    envelope TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_interconnect_events_stream
    ON interconnect_events (tenant_id, subject, sequence);
CREATE INDEX IF NOT EXISTS idx_interconnect_events_ordering
    ON interconnect_events (tenant_id, subject, ordering_key, sequence)
    WHERE ordering_key IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS uq_interconnect_events_idempotency
    ON interconnect_events (tenant_id, subject, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS interconnect_event_consumers (
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    consumer_id TEXT NOT NULL,
    subject TEXT NOT NULL,
    cursor_sequence INTEGER NOT NULL DEFAULT 0 CHECK (cursor_sequence >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    PRIMARY KEY (tenant_id, consumer_id, subject)
);

CREATE TABLE IF NOT EXISTS interconnect_work (
    work_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    subject TEXT NOT NULL,
    envelope TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'ready'
        CHECK (status IN ('ready','leased','succeeded','dead_letter')),
    claim_generation INTEGER NOT NULL DEFAULT 0 CHECK (claim_generation >= 0),
    lease_owner TEXT,
    lease_deadline_ms INTEGER,
    attempt INTEGER NOT NULL DEFAULT 0 CHECK (attempt >= 0),
    retry_budget INTEGER NOT NULL DEFAULT 0 CHECK (retry_budget BETWEEN 0 AND 100),
    available_at_ms INTEGER NOT NULL CHECK (available_at_ms >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    idempotency_key TEXT,
    last_error TEXT,
    CHECK (
        (status = 'leased' AND lease_owner IS NOT NULL AND lease_deadline_ms IS NOT NULL)
        OR
        (status <> 'leased' AND lease_owner IS NULL AND lease_deadline_ms IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_interconnect_work_claim
    ON interconnect_work (tenant_id, subject, status, available_at_ms, created_at_ms, work_id);
CREATE UNIQUE INDEX IF NOT EXISTS uq_interconnect_work_idempotency
    ON interconnect_work (tenant_id, subject, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

-- Plugin-backed Work and its executable pin are one admission identity. A pin
-- cannot exist without the Work row, and provider-backed pins must include both
-- provider instance and binding revision so queued execution cannot be partially
-- retargeted after admission.
CREATE TABLE IF NOT EXISTS interconnect_plugin_work_pins (
    work_id TEXT PRIMARY KEY REFERENCES interconnect_work(work_id) ON DELETE CASCADE,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    plugin_id TEXT NOT NULL,
    plugin_version TEXT NOT NULL,
    package_digest_sha256 TEXT NOT NULL CHECK (length(package_digest_sha256) = 64),
    manifest_digest_sha256 TEXT NOT NULL CHECK (length(manifest_digest_sha256) = 64),
    capability_contract_version TEXT NOT NULL,
    provider_instance_id TEXT,
    binding_revision TEXT,
    CHECK (
        (provider_instance_id IS NULL AND binding_revision IS NULL)
        OR
        (provider_instance_id IS NOT NULL AND binding_revision IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_interconnect_plugin_work_pins_tenant
    ON interconnect_plugin_work_pins (tenant_id, plugin_id, plugin_version);

CREATE TABLE IF NOT EXISTS interconnect_state_heads (
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    subject TEXT NOT NULL,
    latest_revision INTEGER NOT NULL CHECK (latest_revision >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    PRIMARY KEY (tenant_id, subject)
);

CREATE TABLE IF NOT EXISTS interconnect_state_snapshots (
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    subject TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    envelope TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    PRIMARY KEY (tenant_id, subject)
);

CREATE TABLE IF NOT EXISTS interconnect_state_deltas (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    subject TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    envelope TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    UNIQUE (tenant_id, subject, revision)
);

CREATE INDEX IF NOT EXISTS idx_interconnect_state_watch
    ON interconnect_state_deltas (tenant_id, subject, cursor);

CREATE TABLE IF NOT EXISTS interconnect_outbox (
    outbox_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    subject TEXT NOT NULL,
    envelope TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','published')),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    published_at_ms INTEGER,
    CHECK (
        (status = 'pending' AND published_at_ms IS NULL)
        OR
        (status = 'published' AND published_at_ms IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_interconnect_outbox_pending
    ON interconnect_outbox (status, created_at_ms, outbox_id)
    WHERE status = 'pending';