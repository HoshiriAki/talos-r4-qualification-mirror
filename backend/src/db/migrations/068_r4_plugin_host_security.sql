-- R4-P6 Plugin Permission & Isolation durable package/install truth.

CREATE TABLE IF NOT EXISTS plugin_packages (
    plugin_id TEXT NOT NULL,
    publisher_id TEXT NOT NULL,
    plugin_version TEXT NOT NULL,
    package_digest_sha256 TEXT NOT NULL
        CHECK (length(package_digest_sha256) = 64 AND package_digest_sha256 NOT GLOB '*[^0-9a-f]*'),
    manifest_digest_sha256 TEXT NOT NULL
        CHECK (length(manifest_digest_sha256) = 64 AND manifest_digest_sha256 NOT GLOB '*[^0-9a-f]*'),
    capability_contract_version TEXT NOT NULL,
    compatibility_range TEXT NOT NULL,
    declared_capabilities_json TEXT NOT NULL,
    permission_request_json TEXT NOT NULL,
    verification_evidence_json TEXT NOT NULL,
    lifecycle_state TEXT NOT NULL
        CHECK (lifecycle_state IN ('staged','active','suspended','revoked')),
    revoked_reason_code TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (plugin_id, plugin_version, package_digest_sha256),
    UNIQUE (plugin_id, plugin_version, package_digest_sha256, manifest_digest_sha256),
    CHECK (
        (lifecycle_state = 'revoked' AND revoked_reason_code IS NOT NULL)
        OR (lifecycle_state <> 'revoked' AND revoked_reason_code IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_plugin_packages_publisher
    ON plugin_packages (publisher_id, plugin_id, plugin_version);
CREATE INDEX IF NOT EXISTS idx_plugin_packages_lifecycle
    ON plugin_packages (lifecycle_state, plugin_id);

CREATE TABLE IF NOT EXISTS plugin_installations (
    installation_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    plugin_id TEXT NOT NULL,
    plugin_version TEXT NOT NULL,
    package_digest_sha256 TEXT NOT NULL,
    manifest_digest_sha256 TEXT NOT NULL,
    isolation_profile TEXT NOT NULL
        CHECK (isolation_profile IN ('first_party_native','verified_sandboxed','local_unverified','single_file_web_app','remote_worker')),
    installation_grant_json TEXT NOT NULL,
    tenant_policy_json TEXT NOT NULL,
    grant_revision INTEGER NOT NULL DEFAULT 1 CHECK (grant_revision > 0),
    tenant_policy_revision INTEGER NOT NULL DEFAULT 1 CHECK (tenant_policy_revision > 0),
    lifecycle_state TEXT NOT NULL
        CHECK (lifecycle_state IN ('active','suspended','revoked')),
    revoked_reason_code TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (
        tenant_id,
        plugin_id,
        plugin_version,
        package_digest_sha256,
        manifest_digest_sha256
    ),
    FOREIGN KEY (plugin_id, plugin_version, package_digest_sha256, manifest_digest_sha256)
        REFERENCES plugin_packages (
            plugin_id, plugin_version, package_digest_sha256, manifest_digest_sha256
        ),
    CHECK (
        (lifecycle_state = 'revoked' AND revoked_reason_code IS NOT NULL)
        OR (lifecycle_state <> 'revoked' AND revoked_reason_code IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_plugin_installations_tenant_lifecycle
    ON plugin_installations (tenant_id, lifecycle_state, plugin_id);
CREATE INDEX IF NOT EXISTS idx_plugin_installations_package
    ON plugin_installations (plugin_id, plugin_version, package_digest_sha256);

-- Durable record of the exact upgrade decision that authorized a candidate
-- installation. It records migration policy but never acts as a mutable route
-- that could silently retarget queued P3 executable pins.
CREATE TABLE IF NOT EXISTS plugin_upgrade_transitions (
    candidate_installation_id TEXT PRIMARY KEY
        REFERENCES plugin_installations(installation_id) ON DELETE RESTRICT,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    previous_executable_json TEXT NOT NULL,
    candidate_identity_json TEXT NOT NULL,
    permission_diff_json TEXT NOT NULL,
    approved_additions_json TEXT NOT NULL,
    migration_strategy TEXT NOT NULL
        CHECK (migration_strategy = 'preserve_exact_pins'),
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_plugin_upgrade_transitions_tenant
    ON plugin_upgrade_transitions (tenant_id, created_at, candidate_installation_id);

CREATE TABLE IF NOT EXISTS plugin_storage_owners (
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    plugin_id TEXT NOT NULL,
    max_entry_bytes INTEGER NOT NULL CHECK (max_entry_bytes > 0),
    max_total_bytes INTEGER NOT NULL CHECK (max_total_bytes >= max_entry_bytes),
    max_entries INTEGER NOT NULL CHECK (max_entries > 0),
    quota_revision INTEGER NOT NULL DEFAULT 1 CHECK (quota_revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, plugin_id)
);

CREATE TABLE IF NOT EXISTS plugin_storage_entries (
    tenant_id TEXT NOT NULL,
    plugin_id TEXT NOT NULL,
    namespace TEXT NOT NULL CHECK (length(namespace) BETWEEN 1 AND 192),
    entry_key TEXT NOT NULL CHECK (length(entry_key) BETWEEN 1 AND 192),
    value_bytes BLOB NOT NULL,
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, plugin_id, namespace, entry_key),
    FOREIGN KEY (tenant_id, plugin_id)
        REFERENCES plugin_storage_owners (tenant_id, plugin_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_plugin_storage_owner
    ON plugin_storage_entries (tenant_id, plugin_id, namespace);
