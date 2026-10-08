CREATE TABLE tenants (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK (status IN ('active', 'suspended', 'inactive', 'deleted')),
    plan TEXT NOT NULL,
    settings JSONB,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE identities (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    email TEXT UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT NOT NULL,
    phone TEXT,
    totp_secret_ciphertext TEXT,
    totp_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    status TEXT NOT NULL CHECK (status IN ('active', 'disabled', 'locked')),
    last_login_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE tenant_memberships (
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

CREATE TABLE platform_memberships (
    id TEXT PRIMARY KEY,
    identity_id TEXT NOT NULL UNIQUE REFERENCES identities(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('active', 'suspended', 'revoked')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (id, identity_id)
);

CREATE TABLE platform_role_grants (
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

CREATE TABLE auth_sessions (
    id TEXT PRIMARY KEY,
    token_hash TEXT NOT NULL UNIQUE,
    identity_id TEXT NOT NULL REFERENCES identities(id) ON DELETE CASCADE,
    auth_strength TEXT NOT NULL DEFAULT 'password' CHECK (auth_strength IN ('password', 'mfa')),
    created_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    revoked_at TEXT
);

CREATE TABLE audit_events (
    id TEXT PRIMARY KEY,
    actor_identity_id TEXT REFERENCES identities(id) ON DELETE SET NULL,
    authority_kind TEXT NOT NULL CHECK (authority_kind IN ('tenant', 'platform', 'system')),
    tenant_id TEXT REFERENCES tenants(id) ON DELETE RESTRICT,
    tenant_membership_id TEXT,
    platform_membership_id TEXT,
    roles_snapshot JSONB NOT NULL DEFAULT '[]'::jsonb,
    capabilities_snapshot JSONB NOT NULL DEFAULT '[]'::jsonb,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    correlation_id TEXT NOT NULL,
    detail_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    occurred_at TEXT NOT NULL,
    FOREIGN KEY (tenant_membership_id, actor_identity_id, tenant_id)
        REFERENCES tenant_memberships(id, identity_id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY (platform_membership_id, actor_identity_id)
        REFERENCES platform_memberships(id, identity_id) ON DELETE RESTRICT,
    CHECK (
        (authority_kind = 'tenant' AND actor_identity_id IS NOT NULL AND tenant_id IS NOT NULL AND tenant_membership_id IS NOT NULL AND platform_membership_id IS NULL)
        OR (authority_kind = 'platform' AND actor_identity_id IS NOT NULL AND tenant_membership_id IS NULL AND platform_membership_id IS NOT NULL)
        OR (authority_kind = 'system' AND actor_identity_id IS NULL
            AND tenant_membership_id IS NULL AND platform_membership_id IS NULL)
    )
);

CREATE INDEX idx_tenant_memberships_tenant ON tenant_memberships(tenant_id, status);
CREATE INDEX idx_tenant_memberships_identity ON tenant_memberships(identity_id, status);
CREATE UNIQUE INDEX uq_tenant_active_owner
    ON tenant_memberships(tenant_id) WHERE role = 'owner' AND status = 'active';
CREATE INDEX idx_platform_role_grants_membership ON platform_role_grants(platform_membership_id);
CREATE INDEX idx_auth_sessions_identity ON auth_sessions(identity_id, expires_at);
CREATE INDEX idx_audit_events_actor ON audit_events(actor_identity_id, occurred_at DESC);
CREATE INDEX idx_audit_events_tenant ON audit_events(tenant_id, occurred_at DESC);
CREATE INDEX idx_audit_events_correlation ON audit_events(correlation_id);
