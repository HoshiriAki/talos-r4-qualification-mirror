-- Pre-Alpha authority foundation. The identity subject and session start in 001.

CREATE TABLE IF NOT EXISTS identities (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    email TEXT UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT NOT NULL,
    phone TEXT,
    totp_secret_ciphertext TEXT,
    totp_enabled INTEGER NOT NULL DEFAULT 0 CHECK (totp_enabled IN (0, 1)),
    status TEXT NOT NULL CHECK (status IN ('active', 'disabled', 'locked')),
    last_login_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tenant_memberships (
    id TEXT PRIMARY KEY,
    identity_id TEXT NOT NULL REFERENCES identities(id) ON DELETE CASCADE,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('staff', 'admin', 'owner')),
    status TEXT NOT NULL CHECK (status IN ('active', 'suspended', 'revoked')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (identity_id, tenant_id),
    UNIQUE (id, identity_id, tenant_id)
);

CREATE TABLE IF NOT EXISTS platform_memberships (
    id TEXT PRIMARY KEY,
    identity_id TEXT NOT NULL UNIQUE REFERENCES identities(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('active', 'suspended', 'revoked')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (id, identity_id)
);

CREATE TABLE IF NOT EXISTS platform_role_grants (
    id TEXT PRIMARY KEY,
    platform_membership_id TEXT NOT NULL REFERENCES platform_memberships(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN (
        'platform_owner', 'platform_admin', 'platform_operator',
        'support_engineer', 'business_operator', 'security_auditor'
    )),
    granted_by_identity_id TEXT REFERENCES identities(id) ON DELETE SET NULL,
    granted_at TEXT NOT NULL,
    UNIQUE (platform_membership_id, role)
);

CREATE TABLE IF NOT EXISTS auth_sessions (
    id TEXT PRIMARY KEY,
    token_hash TEXT NOT NULL UNIQUE,
    identity_id TEXT NOT NULL REFERENCES identities(id) ON DELETE CASCADE,
    auth_strength TEXT NOT NULL DEFAULT 'password' CHECK (auth_strength IN ('password', 'mfa')),
    created_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    revoked_at TEXT
);

CREATE TABLE IF NOT EXISTS audit_events (
    id TEXT PRIMARY KEY,
    actor_identity_id TEXT REFERENCES identities(id) ON DELETE SET NULL,
    authority_kind TEXT NOT NULL CHECK (authority_kind IN ('tenant', 'platform', 'system')),
    tenant_id TEXT REFERENCES tenants(id) ON DELETE RESTRICT,
    tenant_membership_id TEXT,
    platform_membership_id TEXT,
    roles_snapshot TEXT NOT NULL DEFAULT '[]',
    capabilities_snapshot TEXT NOT NULL DEFAULT '[]',
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    correlation_id TEXT NOT NULL,
    detail_json TEXT NOT NULL DEFAULT '{}',
    occurred_at TEXT NOT NULL,
    FOREIGN KEY (tenant_membership_id, actor_identity_id, tenant_id)
        REFERENCES tenant_memberships(id, identity_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (platform_membership_id, actor_identity_id)
        REFERENCES platform_memberships(id, identity_id) ON DELETE RESTRICT,
    CHECK (
        (authority_kind = 'tenant' AND actor_identity_id IS NOT NULL AND tenant_id IS NOT NULL
            AND tenant_membership_id IS NOT NULL AND platform_membership_id IS NULL)
        OR (authority_kind = 'platform' AND actor_identity_id IS NOT NULL
            AND tenant_membership_id IS NULL AND platform_membership_id IS NOT NULL)
        OR (authority_kind = 'system' AND actor_identity_id IS NULL
            AND tenant_membership_id IS NULL AND platform_membership_id IS NULL)
    )
);

CREATE TABLE IF NOT EXISTS tenant_workspace_sessions (
    id TEXT PRIMARY KEY,
    actor_identity_id TEXT NOT NULL REFERENCES identities(id) ON DELETE RESTRICT,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    mode TEXT NOT NULL CHECK (mode IN ('preview', 'diagnostics', 'simulation')),
    preview_session_id TEXT REFERENCES tenant_preview_sessions(id) ON DELETE RESTRICT,
    simulation_id TEXT,
    capabilities_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL CHECK (status IN ('active', 'ended')),
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    ended_at TEXT
);

-- One production audit model. The legacy column contract remains a write-through
-- view so feature crates can be migrated independently without splitting data.
DROP TABLE IF EXISTS audit_logs;
CREATE VIEW audit_logs AS
SELECT
    id,
    COALESCE(actor_identity_id, 'system') AS actorIdentityId,
    COALESCE(actor_identity_id, 'system') AS actorUsername,
    action AS actionType,
    resource_type AS entityType,
    COALESCE(resource_id, '') AS entityId,
    COALESCE(resource_id, '') AS entityLabel,
    detail_json AS detailJson,
    '' AS ip,
    '' AS userAgent,
    occurred_at AS createdAt,
    tenant_id
FROM audit_events;

CREATE TRIGGER audit_logs_insert
INSTEAD OF INSERT ON audit_logs
BEGIN
    INSERT INTO audit_events (
        id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
        platform_membership_id, roles_snapshot, capabilities_snapshot, action,
        resource_type, resource_id, correlation_id, detail_json, occurred_at
    )
    SELECT
        NEW.id,
        NULL,
        'system',
        NEW.tenant_id,
        NULL,
        NULL,
        '[]',
        '[]',
        NEW.actionType,
        NEW.entityType,
        NULLIF(NEW.entityId, ''),
        NEW.id,
        COALESCE(NEW.detailJson, '{}'),
        NEW.createdAt
    FROM (SELECT 1);
END;

CREATE INDEX IF NOT EXISTS idx_tenant_memberships_tenant ON tenant_memberships(tenant_id, status);
CREATE INDEX IF NOT EXISTS idx_tenant_memberships_identity ON tenant_memberships(identity_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS uq_tenant_active_owner
    ON tenant_memberships(tenant_id) WHERE role = 'owner' AND status = 'active';
CREATE INDEX IF NOT EXISTS idx_platform_role_grants_membership ON platform_role_grants(platform_membership_id);
CREATE INDEX IF NOT EXISTS idx_auth_sessions_identity ON auth_sessions(identity_id, expires_at);
CREATE INDEX IF NOT EXISTS idx_audit_events_actor ON audit_events(actor_identity_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_events_tenant ON audit_events(tenant_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_events_correlation ON audit_events(correlation_id);
CREATE INDEX IF NOT EXISTS idx_workspace_actor_status
    ON tenant_workspace_sessions(actor_identity_id, status, expires_at);
CREATE INDEX IF NOT EXISTS idx_workspace_tenant
    ON tenant_workspace_sessions(tenant_id, created_at DESC);
