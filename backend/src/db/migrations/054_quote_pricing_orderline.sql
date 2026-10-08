-- Migration 054: R1-P2 Quote / Pricing V2 / OrderLine.
-- Money is stored only in integer minor units. Quote price snapshots are immutable
-- once confirmed and Order creation is bound to one confirmed Quote.

CREATE TABLE IF NOT EXISTS accessory_catalog (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    sku TEXT NOT NULL,
    name TEXT NOT NULL,
    unit_price_minor INTEGER NOT NULL CHECK(unit_price_minor >= 0),
    currency TEXT NOT NULL DEFAULT 'CNY' CHECK(length(currency) = 3),
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version >= 1),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(tenant_id, sku),
    UNIQUE(id, tenant_id)
);

CREATE TABLE IF NOT EXISTS quotes (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    customer_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft'
        CHECK(status IN ('draft', 'confirmed', 'expired', 'cancelled', 'converted')),
    start_date TEXT NOT NULL,
    end_date TEXT NOT NULL,
    region TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CNY' CHECK(length(currency) = 3),
    total_minor INTEGER NOT NULL DEFAULT 0 CHECK(total_minor >= 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version >= 1),
    expires_at TEXT NOT NULL,
    confirmed_at TEXT,
    converted_order_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(id, tenant_id),
    FOREIGN KEY(customer_id, tenant_id)
        REFERENCES customers(id, tenant_id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS quote_lines (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    quote_id TEXT NOT NULL,
    line_kind TEXT NOT NULL CHECK(line_kind IN ('model', 'accessory')),
    reference_id TEXT NOT NULL,
    description TEXT NOT NULL,
    quantity INTEGER NOT NULL CHECK(quantity > 0),
    unit_price_minor INTEGER NOT NULL CHECK(unit_price_minor >= 0),
    subtotal_minor INTEGER NOT NULL CHECK(subtotal_minor >= 0),
    currency TEXT NOT NULL CHECK(length(currency) = 3),
    price_snapshot_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(quote_id, tenant_id)
        REFERENCES quotes(id, tenant_id) ON DELETE CASCADE
);

-- order_lines uses an exact composite parent key so tenant identity participates in
-- referential integrity rather than relying on a separate tenant check.
CREATE UNIQUE INDEX IF NOT EXISTS idx_orders_id_tenant_unique
    ON orders(id, tenant_id);

CREATE TABLE IF NOT EXISTS order_lines (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    order_id TEXT NOT NULL,
    source_quote_line_id TEXT NOT NULL,
    line_kind TEXT NOT NULL CHECK(line_kind IN ('model', 'accessory')),
    reference_id TEXT NOT NULL,
    description TEXT NOT NULL,
    quantity INTEGER NOT NULL CHECK(quantity > 0),
    unit_price_minor INTEGER NOT NULL CHECK(unit_price_minor >= 0),
    subtotal_minor INTEGER NOT NULL CHECK(subtotal_minor >= 0),
    currency TEXT NOT NULL CHECK(length(currency) = 3),
    price_snapshot_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(order_id, tenant_id)
        REFERENCES orders(id, tenant_id) ON DELETE RESTRICT,
    UNIQUE(tenant_id, source_quote_line_id)
);

ALTER TABLE orders ADD COLUMN source_quote_id TEXT;
ALTER TABLE orders ADD COLUMN total_minor INTEGER;
ALTER TABLE orders ADD COLUMN currency TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_orders_source_quote
    ON orders(tenant_id, source_quote_id)
    WHERE source_quote_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_quotes_customer
    ON quotes(tenant_id, customer_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_quotes_status_expiry
    ON quotes(tenant_id, status, expires_at);
CREATE INDEX IF NOT EXISTS idx_quote_lines_quote
    ON quote_lines(tenant_id, quote_id, line_kind);
CREATE INDEX IF NOT EXISTS idx_order_lines_order
    ON order_lines(tenant_id, order_id, line_kind);
CREATE INDEX IF NOT EXISTS idx_accessory_catalog_active
    ON accessory_catalog(tenant_id, active, sku);

CREATE TRIGGER IF NOT EXISTS trg_orders_source_quote_scope_insert
BEFORE INSERT ON orders
WHEN NEW.source_quote_id IS NOT NULL
     AND NOT EXISTS (
         SELECT 1 FROM quotes q
         WHERE q.id = NEW.source_quote_id AND q.tenant_id = NEW.tenant_id
     )
BEGIN
    SELECT RAISE(ABORT, 'orders.source_quote_id must reference the same tenant');
END;

CREATE TRIGGER IF NOT EXISTS trg_orders_source_quote_scope_update
BEFORE UPDATE OF source_quote_id, tenant_id ON orders
WHEN NEW.source_quote_id IS NOT NULL
     AND NOT EXISTS (
         SELECT 1 FROM quotes q
         WHERE q.id = NEW.source_quote_id AND q.tenant_id = NEW.tenant_id
     )
BEGIN
    SELECT RAISE(ABORT, 'orders.source_quote_id must reference the same tenant');
END;

CREATE TRIGGER IF NOT EXISTS trg_quote_lines_immutable_after_confirmation_insert
BEFORE INSERT ON quote_lines
WHEN EXISTS (
    SELECT 1 FROM quotes q
    WHERE q.id = NEW.quote_id AND q.tenant_id = NEW.tenant_id AND q.status <> 'draft'
)
BEGIN
    SELECT RAISE(ABORT, 'confirmed quote lines are immutable');
END;

CREATE TRIGGER IF NOT EXISTS trg_quote_lines_immutable_after_confirmation_update
BEFORE UPDATE ON quote_lines
WHEN EXISTS (
    SELECT 1 FROM quotes q
    WHERE q.id = OLD.quote_id AND q.tenant_id = OLD.tenant_id AND q.status <> 'draft'
)
BEGIN
    SELECT RAISE(ABORT, 'confirmed quote lines are immutable');
END;

CREATE TRIGGER IF NOT EXISTS trg_quote_lines_immutable_after_confirmation_delete
BEFORE DELETE ON quote_lines
WHEN EXISTS (
    SELECT 1 FROM quotes q
    WHERE q.id = OLD.quote_id AND q.tenant_id = OLD.tenant_id AND q.status <> 'draft'
)
BEGIN
    SELECT RAISE(ABORT, 'confirmed quote lines are immutable');
END;
