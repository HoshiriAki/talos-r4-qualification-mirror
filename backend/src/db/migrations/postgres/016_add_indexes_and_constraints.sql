-- 016: Performance indexes + data integrity constraints (PostgreSQL)
-- orderNo already has a UNIQUE constraint from migration 001;
-- this unique index provides defense-in-depth against TOCTOU race conditions.

CREATE UNIQUE INDEX IF NOT EXISTS idx_orders_orderno_unique ON orders(orderNo);

-- Missing query performance indexes
CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(status);
CREATE INDEX IF NOT EXISTS idx_orders_province ON orders(province);
CREATE INDEX IF NOT EXISTS idx_orders_created_at ON orders(createdAt);
CREATE INDEX IF NOT EXISTS idx_devices_rental_status ON devices(rentalStatus);
CREATE INDEX IF NOT EXISTS idx_order_devices_serial ON order_devices(serialNo);
