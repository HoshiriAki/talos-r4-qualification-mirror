-- R4-P8: normalize Optical SOP tenant-local referential integrity.
UPDATE inspection_checklists
SET tenant_id='default'
WHERE tenant_id IS NULL OR tenant_id='';

ALTER TABLE inspection_checklists
    ADD COLUMN IF NOT EXISTS completed_at TEXT;
ALTER TABLE inspection_checklists
    ALTER COLUMN tenant_id SET NOT NULL;

ALTER TABLE inspection_checklists
    DROP CONSTRAINT IF EXISTS inspection_checklists_order_id_fkey;
ALTER TABLE inspection_checklists
    DROP CONSTRAINT IF EXISTS inspection_checklists_damage_report_id_fkey;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'inspection_checklists_id_tenant_key'
    ) THEN
        ALTER TABLE inspection_checklists
            ADD CONSTRAINT inspection_checklists_id_tenant_key
            UNIQUE (id, tenant_id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'inspection_checklists_tenant_order_device_key'
    ) THEN
        ALTER TABLE inspection_checklists
            ADD CONSTRAINT inspection_checklists_tenant_order_device_key
            UNIQUE (tenant_id, order_id, device_serial_no);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'inspection_checklists_order_tenant_fkey'
    ) THEN
        ALTER TABLE inspection_checklists
            ADD CONSTRAINT inspection_checklists_order_tenant_fkey
            FOREIGN KEY (order_id, tenant_id)
            REFERENCES orders(id, tenant_id) ON DELETE RESTRICT;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'inspection_checklists_device_tenant_fkey'
    ) THEN
        ALTER TABLE inspection_checklists
            ADD CONSTRAINT inspection_checklists_device_tenant_fkey
            FOREIGN KEY (device_serial_no, tenant_id)
            REFERENCES devices(serialNo, tenant_id) ON DELETE RESTRICT;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'inspection_checklists_damage_report_fkey'
    ) THEN
        ALTER TABLE inspection_checklists
            ADD CONSTRAINT inspection_checklists_damage_report_fkey
            FOREIGN KEY (damage_report_id)
            REFERENCES damage_reports(id) ON DELETE SET NULL;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'inspection_checklists_tenant_fkey'
    ) THEN
        ALTER TABLE inspection_checklists
            ADD CONSTRAINT inspection_checklists_tenant_fkey
            FOREIGN KEY (tenant_id)
            REFERENCES tenants(id) ON DELETE RESTRICT;
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_inspection_checklists_tenant
    ON inspection_checklists(tenant_id, order_id);
CREATE INDEX IF NOT EXISTS idx_inspection_checklists_grade
    ON inspection_checklists(tenant_id, overall_grade);
