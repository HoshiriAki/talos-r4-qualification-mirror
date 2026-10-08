-- Migration 032: Inventory Reservations + Rules
-- Multi-warehouse inventory reservation system
--
-- inventory_reservations: pre-hold devices for specific customers/orders
-- reservation_rules: configurable reservation policies (max days ahead, per-customer limits, auto-release)

CREATE TABLE IF NOT EXISTS inventory_reservations (
    id BIGSERIAL PRIMARY KEY,
    device_serial_no TEXT NOT NULL,
    warehouse_id TEXT REFERENCES warehouses(id),
    order_id TEXT REFERENCES orders(id),
    customer_name TEXT NOT NULL,
    customer_phone TEXT NOT NULL,
    start_date TEXT NOT NULL,
    end_date TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'reserved' CHECK(status IN ('reserved', 'confirmed', 'released', 'expired')),
    notes TEXT,
    reserved_by TEXT NOT NULL REFERENCES identities(id),
    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    updated_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00')
);

CREATE TABLE IF NOT EXISTS reservation_rules (
    id BIGSERIAL PRIMARY KEY,
    rule_name TEXT NOT NULL,
    max_days_ahead INTEGER NOT NULL DEFAULT 90,
    max_concurrent_per_customer INTEGER NOT NULL DEFAULT 2,
    auto_release_minutes INTEGER NOT NULL DEFAULT 30,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00'),
    updated_at TEXT NOT NULL DEFAULT (to_char(timezone('Asia/Shanghai', now()), 'YYYY-MM-DD"T"HH24:MI:SS') || '+08:00')
);

CREATE INDEX IF NOT EXISTS idx_reservation_device ON inventory_reservations(device_serial_no);
CREATE INDEX IF NOT EXISTS idx_reservation_order ON inventory_reservations(order_id);
CREATE INDEX IF NOT EXISTS idx_reservation_dates ON inventory_reservations(start_date, end_date);
CREATE INDEX IF NOT EXISTS idx_reservation_status ON inventory_reservations(status);

-- Seed default rule
INSERT INTO reservation_rules (id, rule_name) VALUES (1, 'default')
ON CONFLICT (id) DO NOTHING;
