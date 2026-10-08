-- PostgreSQL schema parity only. The simulation runtime intentionally fails
-- closed on PostgreSQL until its storage adapter is implemented.
CREATE TABLE IF NOT EXISTS simulation_pricing_resource_revisions (
    tenant_id TEXT NOT NULL,
    resource_key TEXT NOT NULL CHECK(resource_key = 'default'),
    present SMALLINT NOT NULL CHECK(present IN (0,1)),
    resource_version BIGINT NOT NULL CHECK(resource_version > 0),
    document_json JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY(tenant_id, resource_key)
);
