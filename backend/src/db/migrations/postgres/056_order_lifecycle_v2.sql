-- Migration 056: R1-P5 multi-dimensional Order Lifecycle V2 (PostgreSQL parity).

CREATE TABLE IF NOT EXISTS order_lifecycle (
    order_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    commercial_status TEXT NOT NULL DEFAULT 'draft'
        CHECK(commercial_status IN ('draft', 'submitted', 'confirmed', 'completed', 'closed', 'cancelled')),
    contract_status TEXT NOT NULL DEFAULT 'not_required'
        CHECK(contract_status IN ('not_required', 'draft', 'generated', 'sent', 'signed', 'verified')),
    financial_status TEXT NOT NULL DEFAULT 'unbilled'
        CHECK(financial_status IN ('unbilled', 'awaiting_payment', 'partially_paid', 'paid', 'settlement_pending', 'settled')),
    fulfilment_status TEXT NOT NULL DEFAULT 'unplanned'
        CHECK(fulfilment_status IN ('unplanned', 'reserved', 'allocated', 'ready_to_ship', 'shipped', 'in_use', 'return_pending', 'returned', 'inspected')),
    risk_status TEXT NOT NULL DEFAULT 'clear'
        CHECK(risk_status IN ('clear', 'review_required', 'overdue', 'damage_review', 'repairing', 'disputed', 'resolved')),
    version BIGINT NOT NULL DEFAULT 1 CHECK(version >= 1),
    updated_at TEXT NOT NULL,
    PRIMARY KEY(order_id, tenant_id),
    FOREIGN KEY(order_id, tenant_id) REFERENCES orders(id, tenant_id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS order_lifecycle_history (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    order_id TEXT NOT NULL,
    dimension TEXT NOT NULL
        CHECK(dimension IN ('commercial', 'contract', 'financial', 'fulfilment', 'risk')),
    from_status TEXT NOT NULL DEFAULT '',
    to_status TEXT NOT NULL,
    guard_name TEXT NOT NULL,
    actor_user_id TEXT NOT NULL DEFAULT '',
    reason TEXT NOT NULL DEFAULT '',
    expected_version BIGINT NOT NULL,
    resulting_version BIGINT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(order_id, tenant_id) REFERENCES order_lifecycle(order_id, tenant_id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS lifecycle_migration_exceptions (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    order_id TEXT NOT NULL,
    legacy_status TEXT NOT NULL,
    reason TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK(status IN ('pending', 'resolved', 'ignored')),
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    UNIQUE(tenant_id, order_id),
    FOREIGN KEY(order_id, tenant_id) REFERENCES orders(id, tenant_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_order_lifecycle_tenant_commercial
    ON order_lifecycle(tenant_id, commercial_status, updated_at);
CREATE INDEX IF NOT EXISTS idx_order_lifecycle_tenant_fulfilment
    ON order_lifecycle(tenant_id, fulfilment_status, updated_at);
CREATE INDEX IF NOT EXISTS idx_order_lifecycle_tenant_risk
    ON order_lifecycle(tenant_id, risk_status, updated_at);
CREATE INDEX IF NOT EXISTS idx_order_lifecycle_history_order
    ON order_lifecycle_history(tenant_id, order_id, resulting_version, created_at);
CREATE INDEX IF NOT EXISTS idx_lifecycle_migration_pending
    ON lifecycle_migration_exceptions(tenant_id, status, created_at);

INSERT INTO order_lifecycle (
    order_id, tenant_id, commercial_status, contract_status, financial_status,
    fulfilment_status, risk_status, version, updated_at
)
SELECT
    id,
    tenant_id,
    CASE status
        WHEN 'draft' THEN 'draft'
        WHEN 'confirmed' THEN 'confirmed'
        WHEN 'paid' THEN 'confirmed'
        WHEN 'shipped' THEN 'confirmed'
        WHEN 'in_use' THEN 'confirmed'
        WHEN 'returned' THEN 'confirmed'
        WHEN 'inspected' THEN 'confirmed'
        WHEN 'repairing' THEN 'confirmed'
        WHEN 'completed' THEN 'completed'
        WHEN 'closed' THEN 'closed'
        WHEN 'cancelled' THEN 'cancelled'
        ELSE 'draft'
    END,
    'not_required',
    CASE status
        WHEN 'paid' THEN 'paid'
        WHEN 'shipped' THEN 'paid'
        WHEN 'in_use' THEN 'paid'
        WHEN 'returned' THEN 'paid'
        WHEN 'inspected' THEN 'paid'
        WHEN 'repairing' THEN 'paid'
        WHEN 'completed' THEN 'settlement_pending'
        WHEN 'closed' THEN 'settled'
        ELSE 'unbilled'
    END,
    CASE status
        WHEN 'shipped' THEN 'shipped'
        WHEN 'in_use' THEN 'in_use'
        WHEN 'returned' THEN 'returned'
        WHEN 'inspected' THEN 'inspected'
        WHEN 'repairing' THEN 'inspected'
        WHEN 'completed' THEN 'inspected'
        WHEN 'closed' THEN 'inspected'
        ELSE 'unplanned'
    END,
    CASE status WHEN 'repairing' THEN 'repairing' ELSE 'clear' END,
    1,
    COALESCE(NULLIF(createdat, ''), CURRENT_TIMESTAMP::text)
FROM orders
WHERE tenant_id IS NOT NULL AND tenant_id <> ''
ON CONFLICT (order_id, tenant_id) DO NOTHING;

INSERT INTO lifecycle_migration_exceptions
    (id, tenant_id, order_id, legacy_status, reason, created_at)
SELECT
    'lifecycle-056:' || id,
    tenant_id,
    id,
    COALESCE(status, ''),
    'legacy single-axis order status cannot reconstruct independent contract/financial/fulfilment/risk history; minimum implied lifecycle seeded for explicit review',
    CURRENT_TIMESTAMP::text
FROM orders
WHERE tenant_id IS NOT NULL
  AND tenant_id <> ''
  AND COALESCE(status, '') NOT IN ('', 'draft')
ON CONFLICT (tenant_id, order_id) DO NOTHING;

CREATE OR REPLACE FUNCTION ensure_order_lifecycle_v2()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.tenant_id IS NOT NULL AND NEW.tenant_id <> '' THEN
        INSERT INTO order_lifecycle (
            order_id, tenant_id, commercial_status, contract_status, financial_status,
            fulfilment_status, risk_status, version, updated_at
        ) VALUES (
            NEW.id, NEW.tenant_id, 'draft', 'not_required', 'unbilled',
            'unplanned', 'clear', 1, CURRENT_TIMESTAMP::text
        ) ON CONFLICT (order_id, tenant_id) DO NOTHING;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_orders_create_lifecycle_v2 ON orders;
CREATE TRIGGER trg_orders_create_lifecycle_v2
AFTER INSERT ON orders
FOR EACH ROW EXECUTE FUNCTION ensure_order_lifecycle_v2();
