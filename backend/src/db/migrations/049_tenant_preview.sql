CREATE TABLE IF NOT EXISTS tenant_preview_sessions (
    id TEXT PRIMARY KEY,
    actor_id TEXT NOT NULL,
    target_tenant_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('active', 'ended')),
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    ended_at TEXT,
    CHECK(julianday(expires_at) > julianday(created_at)),
    FOREIGN KEY(target_tenant_id) REFERENCES tenants(id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_preview_sessions_actor_status
    ON tenant_preview_sessions(actor_id, status, expires_at);
CREATE INDEX IF NOT EXISTS idx_preview_sessions_tenant
    ON tenant_preview_sessions(target_tenant_id, created_at DESC);
