-- R4-P8: depreciation log rows are tenant-owned durable records.
CREATE TRIGGER depreciation_log_tenant_id_not_null
BEFORE INSERT ON depreciation_log
FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'tenant_id cannot be NULL in depreciation_log');
END;

CREATE TRIGGER depreciation_log_tenant_id_update_not_null
BEFORE UPDATE OF tenant_id ON depreciation_log
FOR EACH ROW WHEN NEW.tenant_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'tenant_id cannot be NULL in depreciation_log');
END;

CREATE TRIGGER fk_depreciation_log_tenant_id_insert
BEFORE INSERT ON depreciation_log
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'Foreign key constraint failed: depreciation_log.tenant_id must reference tenants.id')
    WHERE NEW.tenant_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM tenants WHERE id = NEW.tenant_id);
END;

CREATE TRIGGER fk_depreciation_log_tenant_id_update
BEFORE UPDATE OF tenant_id ON depreciation_log
FOR EACH ROW
BEGIN
    SELECT RAISE(ABORT, 'Foreign key constraint failed: depreciation_log.tenant_id must reference tenants.id')
    WHERE NEW.tenant_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM tenants WHERE id = NEW.tenant_id);
END;

CREATE INDEX IF NOT EXISTS idx_depreciation_log_tenant_period
    ON depreciation_log(tenant_id, period);
