-- R4-P8: repair the historical PostgreSQL Reservation rule tenant invariant.
-- Migration 047 incorrectly qualified reservation_rules through the public schema,
-- so search_path-isolated production qualification schemas could miss tenant_id.

ALTER TABLE reservation_rules
    ADD COLUMN IF NOT EXISTS tenant_id TEXT;

UPDATE reservation_rules
SET tenant_id = 'default'
WHERE tenant_id IS NULL OR btrim(tenant_id) = '';

ALTER TABLE reservation_rules
    ALTER COLUMN tenant_id SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_reservation_rules_tenant
    ON reservation_rules(tenant_id);
