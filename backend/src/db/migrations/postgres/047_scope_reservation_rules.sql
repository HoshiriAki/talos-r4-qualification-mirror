-- MVP0: scope optional reservation tables when the feature schema exists.
DO $$
BEGIN
    IF to_regclass('public.reservation_rules') IS NOT NULL THEN
        ALTER TABLE reservation_rules ADD COLUMN IF NOT EXISTS tenant_id TEXT;
        UPDATE reservation_rules SET tenant_id = 'default' WHERE tenant_id IS NULL;
        ALTER TABLE reservation_rules ALTER COLUMN tenant_id SET NOT NULL;
        CREATE UNIQUE INDEX IF NOT EXISTS idx_reservation_rules_tenant
            ON reservation_rules(tenant_id);
    END IF;
    IF to_regclass('public.inventory_reservations') IS NOT NULL THEN
        ALTER TABLE inventory_reservations ADD COLUMN IF NOT EXISTS tenant_id TEXT;
        UPDATE inventory_reservations SET tenant_id = 'default' WHERE tenant_id IS NULL;
        ALTER TABLE inventory_reservations ALTER COLUMN tenant_id SET NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_reservations_tenant
            ON inventory_reservations(tenant_id, device_serial_no);
    END IF;
END $$;
