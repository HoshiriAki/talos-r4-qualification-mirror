-- Migration 044: add tenant scope when the optional finance schema is present.
-- PostgreSQL deployments without tax_config remain supported.

DO $$
BEGIN
    IF to_regclass('public.tax_config') IS NOT NULL THEN
        ALTER TABLE tax_config ADD COLUMN IF NOT EXISTS tenant_id TEXT;
        UPDATE tax_config
        SET tenant_id = 'default'
        WHERE tenant_id IS NULL OR tenant_id = '';
        EXECUTE 'CREATE INDEX IF NOT EXISTS idx_tax_config_tenant_active '
             || 'ON tax_config(tenant_id, tax_type, is_active, effective_from)';
    END IF;
END $$;
