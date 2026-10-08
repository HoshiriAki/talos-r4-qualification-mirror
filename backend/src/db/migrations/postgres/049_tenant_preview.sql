CREATE TABLE IF NOT EXISTS tenant_preview_sessions (
    id TEXT PRIMARY KEY,
    actor_id TEXT NOT NULL,
    target_tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    status TEXT NOT NULL CHECK(status IN ('active', 'ended')),
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ,
    CHECK(expires_at > created_at)
);

CREATE INDEX IF NOT EXISTS idx_preview_sessions_actor_status
    ON tenant_preview_sessions(actor_id, status, expires_at);
CREATE INDEX IF NOT EXISTS idx_preview_sessions_tenant
    ON tenant_preview_sessions(target_tenant_id, created_at DESC);
