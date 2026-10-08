-- Migration 044: add the missing tenant scope to tax configuration.
-- Existing global tax settings belong to the legacy default tenant.

ALTER TABLE tax_config ADD COLUMN tenant_id TEXT;
UPDATE tax_config
SET tenant_id = 'default'
WHERE tenant_id IS NULL OR tenant_id = '';

CREATE INDEX IF NOT EXISTS idx_tax_config_tenant_active
ON tax_config(tenant_id, tax_type, is_active, effective_from);
