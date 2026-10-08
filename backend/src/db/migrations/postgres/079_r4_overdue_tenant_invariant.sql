-- R4-P8: harden tenant-local Overdue configuration and escalation identity.

UPDATE overdue_records
SET tenant_id='default'
WHERE tenant_id IS NULL OR tenant_id='';

UPDATE overdue_fee_config
SET tenant_id='default'
WHERE tenant_id IS NULL OR tenant_id='';

ALTER TABLE overdue_notification_log
    ADD COLUMN IF NOT EXISTS tenant_id TEXT;

UPDATE overdue_notification_log AS notification
SET tenant_id = records.tenant_id
FROM overdue_records AS records
WHERE notification.overdue_id = records.id
  AND (notification.tenant_id IS NULL OR notification.tenant_id='');

UPDATE overdue_notification_log
SET tenant_id='default'
WHERE tenant_id IS NULL OR tenant_id='';

DELETE FROM overdue_fee_config AS stale
USING overdue_fee_config AS keep
WHERE stale.tenant_id = keep.tenant_id
  AND stale.id > keep.id;

DELETE FROM overdue_notification_log AS stale
USING overdue_notification_log AS keep
WHERE stale.tenant_id = keep.tenant_id
  AND stale.overdue_id = keep.overdue_id
  AND stale.escalation_level = keep.escalation_level
  AND stale.id > keep.id;

ALTER TABLE overdue_records
    ALTER COLUMN tenant_id SET NOT NULL;

ALTER TABLE overdue_fee_config
    ALTER COLUMN tenant_id SET NOT NULL;

ALTER TABLE overdue_notification_log
    ALTER COLUMN tenant_id SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_overdue_fee_config_tenant_unique
    ON overdue_fee_config(tenant_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_overdue_notification_tenant_level_unique
    ON overdue_notification_log(tenant_id, overdue_id, escalation_level);
