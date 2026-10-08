-- R4-P8: catalog names are tenant-scoped, not process-global.
ALTER TABLE device_models DROP CONSTRAINT IF EXISTS device_models_name_key;
ALTER TABLE warehouses DROP CONSTRAINT IF EXISTS warehouses_name_key;

CREATE UNIQUE INDEX IF NOT EXISTS idx_device_models_tenant_name_unique
    ON device_models(tenant_id, name);
CREATE UNIQUE INDEX IF NOT EXISTS idx_warehouses_tenant_name_unique
    ON warehouses(tenant_id, name);
