-- R4-P8: device serial identity belongs to a tenant namespace.
-- Legacy single-column serial foreign keys are replaced by tenant-scoped
-- composite references before the original global serial uniqueness is removed.

ALTER TABLE order_devices
    DROP CONSTRAINT IF EXISTS order_devices_serialno_fkey;
ALTER TABLE damage_reports
    DROP CONSTRAINT IF EXISTS damage_reports_device_serial_no_fkey;
ALTER TABLE repair_orders
    DROP CONSTRAINT IF EXISTS repair_orders_device_serial_no_fkey;

ALTER TABLE devices
    DROP CONSTRAINT IF EXISTS devices_serialno_key;

CREATE UNIQUE INDEX IF NOT EXISTS idx_devices_serial_tenant_unique
    ON devices(serialNo, tenant_id);

ALTER TABLE order_devices ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE damage_reports ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE repair_orders ALTER COLUMN tenant_id SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'fk_order_devices_device_tenant'
    ) THEN
        ALTER TABLE order_devices
            ADD CONSTRAINT fk_order_devices_device_tenant
            FOREIGN KEY (serialNo, tenant_id)
            REFERENCES devices(serialNo, tenant_id) ON DELETE CASCADE;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'fk_damage_reports_device_tenant'
    ) THEN
        ALTER TABLE damage_reports
            ADD CONSTRAINT fk_damage_reports_device_tenant
            FOREIGN KEY (device_serial_no, tenant_id)
            REFERENCES devices(serialNo, tenant_id) ON DELETE CASCADE;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'fk_repair_orders_device_tenant'
    ) THEN
        ALTER TABLE repair_orders
            ADD CONSTRAINT fk_repair_orders_device_tenant
            FOREIGN KEY (device_serial_no, tenant_id)
            REFERENCES devices(serialNo, tenant_id) ON DELETE CASCADE;
    END IF;
END $$;
