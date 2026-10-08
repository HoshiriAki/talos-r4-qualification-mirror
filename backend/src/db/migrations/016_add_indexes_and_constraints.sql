-- 016_add_indexes_and_constraints: Performance indexes for hot query paths
-- order_devices join indexes
CREATE INDEX IF NOT EXISTS idx_order_devices_serial_no ON order_devices(serialNo);
CREATE INDEX IF NOT EXISTS idx_order_devices_order_id ON order_devices(orderId);

-- orders date range + sort indexes
CREATE INDEX IF NOT EXISTS idx_orders_dates ON orders(startDate, endDate);
CREATE INDEX IF NOT EXISTS idx_orders_created_at ON orders(createdAt DESC);
CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(status);

-- devices lookup indexes
CREATE INDEX IF NOT EXISTS idx_devices_rental_status ON devices(rentalStatus);

-- schema_migrations (ensures migration table exists, fixing edge case on fresh DB)
CREATE TABLE IF NOT EXISTS schema_migrations (
    id TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    appliedAt TEXT NOT NULL
);
