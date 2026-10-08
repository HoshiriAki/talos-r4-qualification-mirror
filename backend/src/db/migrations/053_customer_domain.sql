-- Migration 053: R1-P1 Customer domain and typed customer associations.
-- Legacy free-text customer fields are not auto-merged. Ambiguous rows are captured
-- as explicit migration exceptions for later governed resolution.

CREATE TABLE IF NOT EXISTS customers (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    legal_name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active'
        CHECK(status IN ('active', 'inactive', 'anonymized')),
    risk_status TEXT NOT NULL DEFAULT 'clear'
        CHECK(risk_status IN ('clear', 'review_required', 'blocked')),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version >= 1),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(id, tenant_id)
);

CREATE TABLE IF NOT EXISTS customer_contacts (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    customer_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('phone', 'email', 'other')),
    raw_value TEXT NOT NULL,
    normalized_value TEXT NOT NULL,
    is_primary INTEGER NOT NULL DEFAULT 0 CHECK(is_primary IN (0, 1)),
    classification TEXT NOT NULL DEFAULT 'pii',
    purpose TEXT NOT NULL DEFAULT 'rental_contact',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(customer_id, tenant_id)
        REFERENCES customers(id, tenant_id) ON DELETE CASCADE,
    UNIQUE(tenant_id, customer_id, kind, normalized_value)
);

CREATE TABLE IF NOT EXISTS customer_external_identities (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    customer_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    external_subject TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(customer_id, tenant_id)
        REFERENCES customers(id, tenant_id) ON DELETE CASCADE,
    UNIQUE(tenant_id, provider, external_subject)
);

CREATE TABLE IF NOT EXISTS customer_history (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    customer_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    detail_json TEXT NOT NULL DEFAULT '{}',
    actor_identity_id TEXT REFERENCES identities(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(customer_id, tenant_id)
        REFERENCES customers(id, tenant_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS customer_migration_exceptions (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    source_table TEXT NOT NULL,
    source_row_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    payload_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK(status IN ('pending', 'resolved', 'ignored')),
    resolved_customer_id TEXT,
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    UNIQUE(tenant_id, source_table, source_row_id),
    FOREIGN KEY(resolved_customer_id, tenant_id)
        REFERENCES customers(id, tenant_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_customers_tenant_name
    ON customers(tenant_id, display_name, legal_name);
CREATE INDEX IF NOT EXISTS idx_customers_tenant_status
    ON customers(tenant_id, status, risk_status);
CREATE INDEX IF NOT EXISTS idx_customer_contacts_lookup
    ON customer_contacts(tenant_id, kind, normalized_value);
CREATE INDEX IF NOT EXISTS idx_customer_contacts_customer
    ON customer_contacts(tenant_id, customer_id, is_primary DESC);
CREATE INDEX IF NOT EXISTS idx_customer_external_customer
    ON customer_external_identities(tenant_id, customer_id);
CREATE INDEX IF NOT EXISTS idx_customer_history_customer
    ON customer_history(tenant_id, customer_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_customer_migration_pending
    ON customer_migration_exceptions(tenant_id, status, source_table);

ALTER TABLE orders ADD COLUMN customer_id TEXT;
ALTER TABLE blacklist ADD COLUMN customer_id TEXT;
ALTER TABLE violations ADD COLUMN customer_id TEXT;
ALTER TABLE credit_scores ADD COLUMN customer_id TEXT;
ALTER TABLE overdue_records ADD COLUMN customer_id TEXT;
ALTER TABLE contracts ADD COLUMN customer_id TEXT;

CREATE INDEX IF NOT EXISTS idx_orders_customer ON orders(tenant_id, customer_id);
CREATE INDEX IF NOT EXISTS idx_blacklist_customer ON blacklist(tenant_id, customer_id);
CREATE INDEX IF NOT EXISTS idx_violations_customer ON violations(tenant_id, customer_id);
CREATE INDEX IF NOT EXISTS idx_credit_scores_customer ON credit_scores(tenant_id, customer_id);
CREATE INDEX IF NOT EXISTS idx_overdue_customer ON overdue_records(tenant_id, customer_id);
CREATE INDEX IF NOT EXISTS idx_contracts_customer ON contracts(tenant_id, customer_id);

CREATE TRIGGER IF NOT EXISTS trg_orders_customer_scope_insert
BEFORE INSERT ON orders
WHEN NEW.customer_id IS NOT NULL
     AND NOT EXISTS (
         SELECT 1 FROM customers c
         WHERE c.id = NEW.customer_id AND c.tenant_id = NEW.tenant_id
     )
BEGIN
    SELECT RAISE(ABORT, 'orders.customer_id must reference the same tenant');
END;

CREATE TRIGGER IF NOT EXISTS trg_orders_customer_scope_update
BEFORE UPDATE OF customer_id, tenant_id ON orders
WHEN NEW.customer_id IS NOT NULL
     AND NOT EXISTS (
         SELECT 1 FROM customers c
         WHERE c.id = NEW.customer_id AND c.tenant_id = NEW.tenant_id
     )
BEGIN
    SELECT RAISE(ABORT, 'orders.customer_id must reference the same tenant');
END;

CREATE TRIGGER IF NOT EXISTS trg_blacklist_customer_scope
BEFORE INSERT ON blacklist
WHEN NEW.customer_id IS NOT NULL
     AND NOT EXISTS (
         SELECT 1 FROM customers c
         WHERE c.id = NEW.customer_id AND c.tenant_id = NEW.tenant_id
     )
BEGIN
    SELECT RAISE(ABORT, 'blacklist.customer_id must reference the same tenant');
END;

CREATE TRIGGER IF NOT EXISTS trg_violations_customer_scope
BEFORE INSERT ON violations
WHEN NEW.customer_id IS NOT NULL
     AND NOT EXISTS (
         SELECT 1 FROM customers c
         WHERE c.id = NEW.customer_id AND c.tenant_id = NEW.tenant_id
     )
BEGIN
    SELECT RAISE(ABORT, 'violations.customer_id must reference the same tenant');
END;

CREATE TRIGGER IF NOT EXISTS trg_credit_scores_customer_scope
BEFORE INSERT ON credit_scores
WHEN NEW.customer_id IS NOT NULL
     AND NOT EXISTS (
         SELECT 1 FROM customers c
         WHERE c.id = NEW.customer_id AND c.tenant_id = NEW.tenant_id
     )
BEGIN
    SELECT RAISE(ABORT, 'credit_scores.customer_id must reference the same tenant');
END;

CREATE TRIGGER IF NOT EXISTS trg_overdue_customer_scope
BEFORE INSERT ON overdue_records
WHEN NEW.customer_id IS NOT NULL
     AND NOT EXISTS (
         SELECT 1 FROM customers c
         WHERE c.id = NEW.customer_id AND c.tenant_id = NEW.tenant_id
     )
BEGIN
    SELECT RAISE(ABORT, 'overdue_records.customer_id must reference the same tenant');
END;

CREATE TRIGGER IF NOT EXISTS trg_contracts_customer_scope
BEFORE INSERT ON contracts
WHEN NEW.customer_id IS NOT NULL
     AND NOT EXISTS (
         SELECT 1 FROM customers c
         WHERE c.id = NEW.customer_id AND c.tenant_id = NEW.tenant_id
     )
BEGIN
    SELECT RAISE(ABORT, 'contracts.customer_id must reference the same tenant');
END;

-- Existing free-text customer references are ambiguous by definition. Preserve the
-- source rows and create explicit exceptions instead of silently merging people.
INSERT OR IGNORE INTO customer_migration_exceptions (
    id, tenant_id, source_table, source_row_id, reason, payload_json, created_at
)
SELECT
    'legacy:blacklist:' || tenant_id || ':' || CAST(id AS TEXT),
    tenant_id,
    'blacklist',
    CAST(id AS TEXT),
    'legacy_free_text_customer_requires_resolution',
    json_object('customerName', customer_name, 'customerPhone', customer_phone),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM blacklist WHERE customer_id IS NULL;

INSERT OR IGNORE INTO customer_migration_exceptions (
    id, tenant_id, source_table, source_row_id, reason, payload_json, created_at
)
SELECT
    'legacy:violations:' || tenant_id || ':' || CAST(id AS TEXT),
    tenant_id,
    'violations',
    CAST(id AS TEXT),
    'legacy_free_text_customer_requires_resolution',
    json_object('customerName', customer_name, 'customerPhone', customer_phone),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM violations WHERE customer_id IS NULL;

INSERT OR IGNORE INTO customer_migration_exceptions (
    id, tenant_id, source_table, source_row_id, reason, payload_json, created_at
)
SELECT
    'legacy:credit_scores:' || tenant_id || ':' || CAST(id AS TEXT),
    tenant_id,
    'credit_scores',
    CAST(id AS TEXT),
    'legacy_free_text_customer_requires_resolution',
    json_object('customerName', customer_name, 'customerPhone', customer_phone),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM credit_scores WHERE customer_id IS NULL;

INSERT OR IGNORE INTO customer_migration_exceptions (
    id, tenant_id, source_table, source_row_id, reason, payload_json, created_at
)
SELECT
    'legacy:overdue_records:' || tenant_id || ':' || CAST(id AS TEXT),
    tenant_id,
    'overdue_records',
    CAST(id AS TEXT),
    'legacy_free_text_customer_requires_resolution',
    json_object('customerName', customer_name, 'customerPhone', customer_phone),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM overdue_records WHERE customer_id IS NULL;

INSERT OR IGNORE INTO customer_migration_exceptions (
    id, tenant_id, source_table, source_row_id, reason, payload_json, created_at
)
SELECT
    'legacy:contracts:' || tenant_id || ':' || CAST(id AS TEXT),
    tenant_id,
    'contracts',
    CAST(id AS TEXT),
    'legacy_free_text_customer_requires_resolution',
    json_object('customerName', customer_name, 'customerPhone', customer_phone),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM contracts WHERE customer_id IS NULL;

INSERT OR IGNORE INTO customer_migration_exceptions (
    id, tenant_id, source_table, source_row_id, reason, payload_json, created_at
)
SELECT
    'legacy:orders:' || tenant_id || ':' || id,
    tenant_id,
    'orders',
    id,
    'legacy_order_has_no_stable_customer_reference',
    json_object('orderNo', orderNo),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM orders WHERE customer_id IS NULL;
