-- R4-P8: harden tenant-local Overdue configuration and escalation identity.
--
UPDATE overdue_records
SET tenant_id='default'
WHERE tenant_id IS NULL OR tenant_id='';

UPDATE overdue_fee_config
SET tenant_id='default'
WHERE tenant_id IS NULL OR tenant_id='';

UPDATE overdue_notification_log
SET tenant_id='default'
WHERE tenant_id IS NULL OR tenant_id='';

-- Existing 040/045 migrations backfilled tenant ownership. This extension makes
-- the invariants fail closed for future writes and removes duplicate legacy rows
-- deterministically before creating tenant-local unique indexes.

DELETE FROM overdue_fee_config
WHERE id NOT IN (
    SELECT MIN(id)
    FROM overdue_fee_config
    GROUP BY tenant_id
);

DELETE FROM overdue_notification_log
WHERE id NOT IN (
    SELECT MIN(id)
    FROM overdue_notification_log
    GROUP BY tenant_id, overdue_id, escalation_level
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_overdue_fee_config_tenant_unique
    ON overdue_fee_config(tenant_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_overdue_notification_tenant_level_unique
    ON overdue_notification_log(tenant_id, overdue_id, escalation_level);

CREATE TRIGGER IF NOT EXISTS overdue_records_tenant_not_null_insert
BEFORE INSERT ON overdue_records
WHEN NEW.tenant_id IS NULL OR NEW.tenant_id = ''
BEGIN
    SELECT RAISE(ABORT, 'overdue_records tenant scope is required');
END;

CREATE TRIGGER IF NOT EXISTS overdue_records_tenant_not_null_update
BEFORE UPDATE OF tenant_id ON overdue_records
WHEN NEW.tenant_id IS NULL OR NEW.tenant_id = ''
BEGIN
    SELECT RAISE(ABORT, 'overdue_records tenant scope is required');
END;

CREATE TRIGGER IF NOT EXISTS overdue_fee_config_tenant_not_null_insert
BEFORE INSERT ON overdue_fee_config
WHEN NEW.tenant_id IS NULL OR NEW.tenant_id = ''
BEGIN
    SELECT RAISE(ABORT, 'overdue_fee_config tenant scope is required');
END;

CREATE TRIGGER IF NOT EXISTS overdue_fee_config_tenant_not_null_update
BEFORE UPDATE OF tenant_id ON overdue_fee_config
WHEN NEW.tenant_id IS NULL OR NEW.tenant_id = ''
BEGIN
    SELECT RAISE(ABORT, 'overdue_fee_config tenant scope is required');
END;
