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

-- PostgreSQL's READ COMMITTED snapshots do not serialize the cross-table
-- machine-marker/owner check by themselves. This transaction-scoped lock uses
-- PostgreSQL 16's deterministic text hash so every mutation for one identity
-- shares one stable 64-bit advisory key. Hash collisions only add serialization;
-- they cannot permit an invalid authority combination.
CREATE OR REPLACE FUNCTION machine_identity_guard_lock(machine_identity_id TEXT) RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(machine_identity_id, 0));
END; $$;

CREATE OR REPLACE FUNCTION machine_no_session_insert_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=NEW.identity_id) THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    RETURN NEW;
END; $$;
DROP TRIGGER IF EXISTS machine_no_session_insert ON auth_sessions;
CREATE TRIGGER machine_no_session_insert BEFORE INSERT ON auth_sessions
FOR EACH ROW EXECUTE FUNCTION machine_no_session_insert_fn();

CREATE OR REPLACE FUNCTION machine_no_session_update_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=NEW.identity_id) THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    RETURN NEW;
END; $$;
DROP TRIGGER IF EXISTS machine_no_session_update ON auth_sessions;
CREATE TRIGGER machine_no_session_update BEFORE UPDATE ON auth_sessions
FOR EACH ROW EXECUTE FUNCTION machine_no_session_update_fn();

CREATE OR REPLACE FUNCTION machine_no_platform_insert_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=NEW.identity_id) THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    RETURN NEW;
END; $$;
DROP TRIGGER IF EXISTS machine_no_platform_insert ON platform_memberships;
CREATE TRIGGER machine_no_platform_insert BEFORE INSERT ON platform_memberships
FOR EACH ROW EXECUTE FUNCTION machine_no_platform_insert_fn();

CREATE OR REPLACE FUNCTION machine_no_platform_update_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=NEW.identity_id) THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    RETURN NEW;
END; $$;
DROP TRIGGER IF EXISTS machine_no_platform_update ON platform_memberships;
CREATE TRIGGER machine_no_platform_update BEFORE UPDATE ON platform_memberships
FOR EACH ROW EXECUTE FUNCTION machine_no_platform_update_fn();

CREATE OR REPLACE FUNCTION machine_no_owner_membership_insert_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.role='owner' THEN
        PERFORM machine_identity_guard_lock(NEW.identity_id);
        IF EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=NEW.identity_id) THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    END IF;
    RETURN NEW;
END; $$;
DROP TRIGGER IF EXISTS machine_no_owner_membership_insert ON tenant_memberships;
CREATE TRIGGER machine_no_owner_membership_insert BEFORE INSERT ON tenant_memberships
FOR EACH ROW EXECUTE FUNCTION machine_no_owner_membership_insert_fn();

CREATE OR REPLACE FUNCTION machine_no_owner_membership_update_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.role='owner' THEN
        PERFORM machine_identity_guard_lock(NEW.identity_id);
        IF EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=NEW.identity_id) THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    END IF;
    RETURN NEW;
END; $$;
DROP TRIGGER IF EXISTS machine_no_owner_membership_update ON tenant_memberships;
CREATE TRIGGER machine_no_owner_membership_update BEFORE UPDATE ON tenant_memberships
FOR EACH ROW EXECUTE FUNCTION machine_no_owner_membership_update_fn();

CREATE OR REPLACE FUNCTION machine_marker_validate_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM machine_identity_guard_lock(NEW.identity_id);
    IF EXISTS (SELECT 1 FROM auth_sessions WHERE identity_id=NEW.identity_id) OR EXISTS (SELECT 1 FROM platform_memberships WHERE identity_id=NEW.identity_id) OR EXISTS (SELECT 1 FROM tenant_memberships WHERE identity_id=NEW.identity_id AND role='owner') OR NOT EXISTS (SELECT 1 FROM identities WHERE id=NEW.identity_id AND password_hash='!non-interactive') THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    RETURN NEW;
END; $$;
DROP TRIGGER IF EXISTS machine_marker_validate ON machine_identities;
CREATE TRIGGER machine_marker_validate BEFORE INSERT ON machine_identities
FOR EACH ROW EXECUTE FUNCTION machine_marker_validate_fn();

CREATE OR REPLACE FUNCTION machine_marker_immutable_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF 1=1 THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    RETURN NEW;
END; $$;
DROP TRIGGER IF EXISTS machine_marker_immutable ON machine_identities;
CREATE TRIGGER machine_marker_immutable BEFORE UPDATE ON machine_identities
FOR EACH ROW EXECUTE FUNCTION machine_marker_immutable_fn();

CREATE OR REPLACE FUNCTION machine_marker_no_delete_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF 1=1 THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    RETURN OLD;
END; $$;
DROP TRIGGER IF EXISTS machine_marker_no_delete ON machine_identities;
CREATE TRIGGER machine_marker_no_delete BEFORE DELETE ON machine_identities
FOR EACH ROW EXECUTE FUNCTION machine_marker_no_delete_fn();

CREATE OR REPLACE FUNCTION machine_password_locked_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM machine_identities WHERE identity_id=OLD.id) AND (NEW.password_hash <> '!non-interactive' OR NEW.id <> OLD.id) THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    RETURN NEW;
END; $$;
DROP TRIGGER IF EXISTS machine_password_locked ON identities;
CREATE TRIGGER machine_password_locked BEFORE UPDATE ON identities
FOR EACH ROW EXECUTE FUNCTION machine_password_locked_fn();

CREATE OR REPLACE FUNCTION api_client_seal_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.identity_id<>OLD.identity_id OR NEW.membership_id<>OLD.membership_id OR NEW.tenant_id<>OLD.tenant_id OR (OLD.status='revoked' AND NEW.status<>'revoked') THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    RETURN NEW;
END; $$;
DROP TRIGGER IF EXISTS api_client_seal ON api_clients;
CREATE TRIGGER api_client_seal BEFORE UPDATE ON api_clients
FOR EACH ROW EXECUTE FUNCTION api_client_seal_fn();

CREATE OR REPLACE FUNCTION api_credential_seal_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.client_id<>OLD.client_id OR NEW.key_hash<>OLD.key_hash OR NEW.key_prefix<>OLD.key_prefix OR (OLD.status='revoked' AND NEW.status<>'revoked') THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    RETURN NEW;
END; $$;
DROP TRIGGER IF EXISTS api_credential_seal ON api_credentials;
CREATE TRIGGER api_credential_seal BEFORE UPDATE ON api_credentials
FOR EACH ROW EXECUTE FUNCTION api_credential_seal_fn();

CREATE OR REPLACE FUNCTION api_client_no_delete_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF 1=1 THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    RETURN OLD;
END; $$;
DROP TRIGGER IF EXISTS api_client_no_delete ON api_clients;
CREATE TRIGGER api_client_no_delete BEFORE DELETE ON api_clients
FOR EACH ROW EXECUTE FUNCTION api_client_no_delete_fn();

CREATE OR REPLACE FUNCTION api_credential_no_delete_fn() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF 1=1 THEN RAISE EXCEPTION 'MACHINE_AUTHORITY_GUARD'; END IF;
    RETURN OLD;
END; $$;
DROP TRIGGER IF EXISTS api_credential_no_delete ON api_credentials;
CREATE TRIGGER api_credential_no_delete BEFORE DELETE ON api_credentials
FOR EACH ROW EXECUTE FUNCTION api_credential_no_delete_fn();
