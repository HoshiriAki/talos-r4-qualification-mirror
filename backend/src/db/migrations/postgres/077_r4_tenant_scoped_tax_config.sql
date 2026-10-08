-- R4-P8: repair PostgreSQL tax_config tenant ownership for non-public search_path schemas.
-- Historical migration 044 looked only in public.tax_config and could silently skip
-- schemas used by PG18 qualification and schema-isolated deployments.

ALTER TABLE tax_config
    ADD COLUMN IF NOT EXISTS tenant_id TEXT;

UPDATE tax_config
SET tenant_id = 'default'
WHERE tenant_id IS NULL OR tenant_id = '';

ALTER TABLE tax_config
    ALTER COLUMN tenant_id SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_tax_config_tenant_active
    ON tax_config(tenant_id, tax_type, is_active, effective_from);
