-- Migration 067: R4 TALOS Interconnect Fabric durable semantics (PostgreSQL)
-- Generic transport-neutral Event / Work / State / Outbox authority.
-- Provider protocol facts remain outside Core and external effects remain under
-- R2/R3 ExternalOperation authority.

CREATE TABLE IF NOT EXISTS interconnect_events (
    sequence BIGSERIAL PRIMARY KEY,
    message_id TEXT NOT NULL UNIQUE,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    subject TEXT NOT NULL,
    contract_ref TEXT NOT NULL,
    contract_version TEXT NOT NULL,
    schema_ref TEXT NOT NULL,
    correlation_id TEXT NOT NULL,
    causation_id TEXT,
    created_at_ms BIGINT NOT NULL CHECK (created_at_ms >= 0),
    deadline_ms BIGINT CHECK (deadline_ms IS NULL OR deadline_ms >= created_at_ms),
    ordering_key TEXT,
    idempotency_key TEXT,
    envelope JSONB NOT NULL
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
    cursor_sequence BIGINT NOT NULL DEFAULT 0 CHECK (cursor_sequence >= 0),
    updated_at_ms BIGINT NOT NULL CHECK (updated_at_ms >= 0),
    PRIMARY KEY (tenant_id, consumer_id, subject)
);

CREATE TABLE IF NOT EXISTS interconnect_work (
    work_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    subject TEXT NOT NULL,
    envelope JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'ready'
        CHECK (status IN ('ready','leased','succeeded','dead_letter')),
    claim_generation BIGINT NOT NULL DEFAULT 0 CHECK (claim_generation >= 0),
    lease_owner TEXT,
    lease_deadline_ms BIGINT,
    attempt INTEGER NOT NULL DEFAULT 0 CHECK (attempt >= 0),
    retry_budget INTEGER NOT NULL DEFAULT 0 CHECK (retry_budget BETWEEN 0 AND 100),
    available_at_ms BIGINT NOT NULL CHECK (available_at_ms >= 0),
    created_at_ms BIGINT NOT NULL CHECK (created_at_ms >= 0),
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

-- One monotonic revision authority per tenant/subject. Snapshots and deltas
-- both advance this row under transaction lock so concurrent writers cannot
-- commit revision regressions or reorder semantic revisions.
CREATE TABLE IF NOT EXISTS interconnect_state_heads (
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    subject TEXT NOT NULL,
    latest_revision BIGINT NOT NULL CHECK (latest_revision >= 0),
    updated_at_ms BIGINT NOT NULL CHECK (updated_at_ms >= 0),
    PRIMARY KEY (tenant_id, subject)
);

CREATE TABLE IF NOT EXISTS interconnect_state_snapshots (
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    subject TEXT NOT NULL,
    revision BIGINT NOT NULL CHECK (revision >= 0),
    envelope JSONB NOT NULL,
    updated_at_ms BIGINT NOT NULL CHECK (updated_at_ms >= 0),
    PRIMARY KEY (tenant_id, subject)
);

CREATE TABLE IF NOT EXISTS interconnect_state_deltas (
    cursor BIGSERIAL PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    subject TEXT NOT NULL,
    revision BIGINT NOT NULL CHECK (revision >= 0),
    envelope JSONB NOT NULL,
    created_at_ms BIGINT NOT NULL CHECK (created_at_ms >= 0),
    UNIQUE (tenant_id, subject, revision)
);

CREATE INDEX IF NOT EXISTS idx_interconnect_state_watch
    ON interconnect_state_deltas (tenant_id, subject, cursor);

-- Durable handoff for business facts. Callers that already own the same
-- PostgreSQL business transaction may insert here before commit; publication
-- into interconnect_events occurs later in a separate atomic flush transaction.
CREATE TABLE IF NOT EXISTS interconnect_outbox (
    outbox_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    subject TEXT NOT NULL,
    envelope JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','published')),
    created_at_ms BIGINT NOT NULL CHECK (created_at_ms >= 0),
    published_at_ms BIGINT,
    CHECK (
        (status = 'pending' AND published_at_ms IS NULL)
        OR
        (status = 'published' AND published_at_ms IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_interconnect_outbox_pending
    ON interconnect_outbox (status, created_at_ms, outbox_id)
    WHERE status = 'pending';