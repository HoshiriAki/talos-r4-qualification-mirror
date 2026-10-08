-- Migration 053: R1-P1 Customer domain and typed customer associations (PostgreSQL).
-- Ambiguous legacy free-text rows are recorded as migration exceptions rather
-- than being silently merged into a Customer identity.

CREATE TABLE IF NOT EXISTS customers (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    legal_name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active'
        CHECK(status IN ('active', 'inactive', 'anonymized')),
    risk_status TEXT NOT NULL DEFAULT 'clear'
        CHECK(risk_status IN ('clear', 'review_required', 'blocked')),
    version BIGINT NOT NULL DEFAULT 1 CHECK(version >= 1),
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
    is_primary BOOLEAN NOT NULL DEFAULT FALSE,
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
    detail_json JSONB NOT NULL DEFAULT '{}'::jsonb,
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
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
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

ALTER TABLE orders ADD COLUMN IF NOT EXISTS customer_id TEXT;
ALTER TABLE blacklist ADD COLUMN IF NOT EXISTS customer_id TEXT;
ALTER TABLE violations ADD COLUMN IF NOT EXISTS customer_id TEXT;
ALTER TABLE credit_scores ADD COLUMN IF NOT EXISTS customer_id TEXT;
ALTER TABLE overdue_records ADD COLUMN IF NOT EXISTS customer_id TEXT;
ALTER TABLE contracts ADD COLUMN IF NOT EXISTS customer_id TEXT;

CREATE INDEX IF NOT EXISTS idx_orders_customer ON orders(tenant_id, customer_id);
CREATE INDEX IF NOT EXISTS idx_blacklist_customer ON blacklist(tenant_id, customer_id);
CREATE INDEX IF NOT EXISTS idx_violations_customer ON violations(tenant_id, customer_id);
CREATE INDEX IF NOT EXISTS idx_credit_scores_customer ON credit_scores(tenant_id, customer_id);
CREATE INDEX IF NOT EXISTS idx_overdue_customer ON overdue_records(tenant_id, customer_id);
CREATE INDEX IF NOT EXISTS idx_contracts_customer ON contracts(tenant_id, customer_id);

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_orders_customer_tenant') THEN
        ALTER TABLE orders ADD CONSTRAINT fk_orders_customer_tenant
            FOREIGN KEY (customer_id, tenant_id) REFERENCES customers(id, tenant_id) ON DELETE RESTRICT;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_blacklist_customer_tenant') THEN
        ALTER TABLE blacklist ADD CONSTRAINT fk_blacklist_customer_tenant
            FOREIGN KEY (customer_id, tenant_id) REFERENCES customers(id, tenant_id) ON DELETE RESTRICT;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_violations_customer_tenant') THEN
        ALTER TABLE violations ADD CONSTRAINT fk_violations_customer_tenant
            FOREIGN KEY (customer_id, tenant_id) REFERENCES customers(id, tenant_id) ON DELETE RESTRICT;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_credit_scores_customer_tenant') THEN
        ALTER TABLE credit_scores ADD CONSTRAINT fk_credit_scores_customer_tenant
            FOREIGN KEY (customer_id, tenant_id) REFERENCES customers(id, tenant_id) ON DELETE RESTRICT;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_overdue_customer_tenant') THEN
        ALTER TABLE overdue_records ADD CONSTRAINT fk_overdue_customer_tenant
            FOREIGN KEY (customer_id, tenant_id) REFERENCES customers(id, tenant_id) ON DELETE RESTRICT;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_contracts_customer_tenant') THEN
        ALTER TABLE contracts ADD CONSTRAINT fk_contracts_customer_tenant
            FOREIGN KEY (customer_id, tenant_id) REFERENCES customers(id, tenant_id) ON DELETE RESTRICT;
    END IF;
END $$;

INSERT INTO customer_migration_exceptions (
    id, tenant_id, source_table, source_row_id, reason, payload_json, created_at
)
SELECT
    'legacy:blacklist:' || tenant_id || ':' || id::text,
    tenant_id,
    'blacklist',
    id::text,
    'legacy_free_text_customer_requires_resolution',
    jsonb_build_object('customerName', customer_name, 'customerPhone', customer_phone),
    CURRENT_TIMESTAMP::text
FROM blacklist WHERE customer_id IS NULL
ON CONFLICT (tenant_id, source_table, source_row_id) DO NOTHING;

INSERT INTO customer_migration_exceptions (
    id, tenant_id, source_table, source_row_id, reason, payload_json, created_at
)
SELECT
    'legacy:violations:' || tenant_id || ':' || id::text,
    tenant_id,
    'violations',
    id::text,
    'legacy_free_text_customer_requires_resolution',
    jsonb_build_object('customerName', customer_name, 'customerPhone', customer_phone),
    CURRENT_TIMESTAMP::text
FROM violations WHERE customer_id IS NULL
ON CONFLICT (tenant_id, source_table, source_row_id) DO NOTHING;

INSERT INTO customer_migration_exceptions (
    id, tenant_id, source_table, source_row_id, reason, payload_json, created_at
)
SELECT
    'legacy:credit_scores:' || tenant_id || ':' || id::text,
    tenant_id,
    'credit_scores',
    id::text,
    'legacy_free_text_customer_requires_resolution',
    jsonb_build_object('customerName', customer_name, 'customerPhone', customer_phone),
    CURRENT_TIMESTAMP::text
FROM credit_scores WHERE customer_id IS NULL
ON CONFLICT (tenant_id, source_table, source_row_id) DO NOTHING;

INSERT INTO customer_migration_exceptions (
    id, tenant_id, source_table, source_row_id, reason, payload_json, created_at
)
SELECT
    'legacy:overdue_records:' || tenant_id || ':' || id::text,
    tenant_id,
    'overdue_records',
    id::text,
    'legacy_free_text_customer_requires_resolution',
    jsonb_build_object('customerName', customer_name, 'customerPhone', customer_phone),
    CURRENT_TIMESTAMP::text
FROM overdue_records WHERE customer_id IS NULL
ON CONFLICT (tenant_id, source_table, source_row_id) DO NOTHING;

INSERT INTO customer_migration_exceptions (
    id, tenant_id, source_table, source_row_id, reason, payload_json, created_at
)
SELECT
    'legacy:contracts:' || tenant_id || ':' || id::text,
    tenant_id,
    'contracts',
    id::text,
    'legacy_free_text_customer_requires_resolution',
    jsonb_build_object('customerName', customer_name, 'customerPhone', customer_phone),
    CURRENT_TIMESTAMP::text
FROM contracts WHERE customer_id IS NULL
ON CONFLICT (tenant_id, source_table, source_row_id) DO NOTHING;

INSERT INTO customer_migration_exceptions (
    id, tenant_id, source_table, source_row_id, reason, payload_json, created_at
)
SELECT
    'legacy:orders:' || tenant_id || ':' || id,
    tenant_id,
    'orders',
    id,
    'legacy_order_has_no_stable_customer_reference',
    jsonb_build_object('orderNo', orderno),
    CURRENT_TIMESTAMP::text
FROM orders WHERE customer_id IS NULL
ON CONFLICT (tenant_id, source_table, source_row_id) DO NOTHING;
