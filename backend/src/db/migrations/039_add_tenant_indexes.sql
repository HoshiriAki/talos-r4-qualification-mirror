-- Migration 039: Add tenant isolation indexes for query performance
--
-- Composite indexes on (tenant_id, primary_key) ensure efficient tenant-scoped queries.
-- These indexes are critical for multi-tenant query performance.
--
-- NOTE: Talos uses camelCase column names (startDate, serialNo, rentalStatus, etc.)

-- Orders indexes
CREATE INDEX IF NOT EXISTS idx_orders_tenant ON orders(tenant_id, id);
CREATE INDEX IF NOT EXISTS idx_orders_tenant_status ON orders(tenant_id, status);
CREATE INDEX IF NOT EXISTS idx_orders_tenant_date ON orders(tenant_id, startDate);

-- Devices indexes
CREATE INDEX IF NOT EXISTS idx_devices_tenant ON devices(tenant_id, serialNo);
CREATE INDEX IF NOT EXISTS idx_devices_tenant_status ON devices(tenant_id, rentalStatus);

-- Warehouses indexes
CREATE INDEX IF NOT EXISTS idx_warehouses_tenant ON warehouses(tenant_id, id);

-- Audit logs indexes
CREATE INDEX IF NOT EXISTS idx_audit_logs_tenant ON audit_logs(tenant_id, createdAt);

-- Order devices indexes
CREATE INDEX IF NOT EXISTS idx_order_devices_tenant ON order_devices(tenant_id, orderId);
