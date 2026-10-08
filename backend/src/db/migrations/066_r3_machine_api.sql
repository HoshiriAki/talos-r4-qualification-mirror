-- Project/Profile machine authority; FUTURE_CANONICAL_MIGRATION.
-- No new Canonical Identity kind. Existing identity/membership remain authority.
CREATE TABLE IF NOT EXISTS machine_identities (
    identity_id TEXT PRIMARY KEY REFERENCES identities(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS api_clients (
    id TEXT PRIMARY KEY,
    identity_id TEXT NOT NULL REFERENCES machine_identities(identity_id) ON DELETE RESTRICT,
    membership_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active','disabled','revoked')),
    api_version TEXT NOT NULL CHECK (api_version = 'v1'),
    rate_limit_rpm INTEGER NOT NULL CHECK (rate_limit_rpm BETWEEN 1 AND 600),
    created_by TEXT NOT NULL REFERENCES identities(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (membership_id,identity_id,tenant_id)
        REFERENCES tenant_memberships(id,identity_id,tenant_id) ON DELETE RESTRICT
);
CREATE TABLE IF NOT EXISTS api_credentials (
    id TEXT PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES api_clients(id) ON DELETE RESTRICT,
    key_prefix TEXT NOT NULL UNIQUE,
    key_hash TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active','disabled','revoked')),
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS api_scope_grants (
    client_id TEXT NOT NULL REFERENCES api_clients(id) ON DELETE RESTRICT,
    module TEXT NOT NULL,
    command TEXT NOT NULL,
    PRIMARY KEY (client_id,module,command)
);
-- One bounded, restart-safe rolling fixed-minute window per client.
-- Last-used is derived from this usage fact, not a per-request credential write.
CREATE TABLE IF NOT EXISTS api_usage_windows (
    client_id TEXT PRIMARY KEY REFERENCES api_clients(id) ON DELETE RESTRICT,
    window_start BIGINT NOT NULL,
    used INTEGER NOT NULL CHECK (used >= 0),
    last_used_at TEXT NOT NULL
);

CREATE TRIGGER IF NOT EXISTS machine_no_session_insert BEFORE INSERT ON auth_sessions
WHEN EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=NEW.identity_id)
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

CREATE TRIGGER IF NOT EXISTS machine_no_session_update BEFORE UPDATE ON auth_sessions
WHEN EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=NEW.identity_id)
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

CREATE TRIGGER IF NOT EXISTS machine_no_platform_insert BEFORE INSERT ON platform_memberships
WHEN EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=NEW.identity_id)
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

CREATE TRIGGER IF NOT EXISTS machine_no_platform_update BEFORE UPDATE ON platform_memberships
WHEN EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=NEW.identity_id)
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

-- A project machine can hold a real tenant membership for command execution,
-- but never the interactive tenant-owner governance role.  Check NEW on both
-- mutations so a direct SQL retarget cannot bypass the provision-time role
-- validation.
CREATE TRIGGER IF NOT EXISTS machine_no_owner_membership_insert BEFORE INSERT ON tenant_memberships
WHEN NEW.role='owner' AND EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=NEW.identity_id)
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

CREATE TRIGGER IF NOT EXISTS machine_no_owner_membership_update BEFORE UPDATE ON tenant_memberships
WHEN NEW.role='owner' AND EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=NEW.identity_id)
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

CREATE TRIGGER IF NOT EXISTS machine_marker_validate BEFORE INSERT ON machine_identities
WHEN EXISTS (SELECT 1 FROM auth_sessions WHERE identity_id=NEW.identity_id) OR EXISTS (SELECT 1 FROM platform_memberships WHERE identity_id=NEW.identity_id) OR EXISTS (SELECT 1 FROM tenant_memberships WHERE identity_id=NEW.identity_id AND role='owner') OR NOT EXISTS (SELECT 1 FROM identities WHERE id=NEW.identity_id AND password_hash='!non-interactive')
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

CREATE TRIGGER IF NOT EXISTS machine_marker_immutable BEFORE UPDATE ON machine_identities
WHEN 1=1
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

CREATE TRIGGER IF NOT EXISTS machine_marker_no_delete BEFORE DELETE ON machine_identities
WHEN 1=1
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

CREATE TRIGGER IF NOT EXISTS machine_password_locked BEFORE UPDATE ON identities
WHEN EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=OLD.id) AND (NEW.password_hash <> '!non-interactive' OR NEW.id <> OLD.id)
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

CREATE TRIGGER IF NOT EXISTS api_client_seal BEFORE UPDATE ON api_clients
WHEN NEW.identity_id<>OLD.identity_id OR NEW.membership_id<>OLD.membership_id OR NEW.tenant_id<>OLD.tenant_id OR (OLD.status='revoked' AND NEW.status<>'revoked')
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

CREATE TRIGGER IF NOT EXISTS api_credential_seal BEFORE UPDATE ON api_credentials
WHEN NEW.client_id<>OLD.client_id OR NEW.key_hash<>OLD.key_hash OR NEW.key_prefix<>OLD.key_prefix OR (OLD.status='revoked' AND NEW.status<>'revoked')
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

CREATE TRIGGER IF NOT EXISTS api_client_no_delete BEFORE DELETE ON api_clients
WHEN 1=1
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;

CREATE TRIGGER IF NOT EXISTS api_credential_no_delete BEFORE DELETE ON api_credentials
WHEN 1=1
BEGIN SELECT RAISE(ABORT, 'MACHINE_AUTHORITY_GUARD'); END;
