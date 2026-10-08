-- MVP0: tenant-scope pricing storage and replace legacy global uniqueness.
ALTER TABLE pricing_configs ADD COLUMN IF NOT EXISTS tenant_id TEXT NOT NULL DEFAULT 'default';
CREATE UNIQUE INDEX IF NOT EXISTS idx_pricing_configs_tenant ON pricing_configs(tenant_id);

ALTER TABLE model_base_prices ADD COLUMN IF NOT EXISTS tenant_id TEXT NOT NULL DEFAULT 'default';
DO $$
DECLARE pk_name TEXT;
BEGIN
    SELECT c.conname INTO pk_name
    FROM pg_constraint c JOIN pg_class t ON t.oid = c.conrelid
    WHERE t.relname = 'model_base_prices' AND c.contype = 'p' LIMIT 1;
    IF pk_name IS NOT NULL THEN
        EXECUTE format('ALTER TABLE model_base_prices DROP CONSTRAINT %I', pk_name);
    END IF;
END $$;
ALTER TABLE model_base_prices
    ADD CONSTRAINT model_base_prices_pkey PRIMARY KEY (tenant_id, modelId);

ALTER TABLE dynamic_daily_prices ADD COLUMN IF NOT EXISTS tenant_id TEXT NOT NULL DEFAULT 'default';
DO $$
DECLARE constraint_name TEXT;
BEGIN
    SELECT c.conname INTO constraint_name
    FROM pg_constraint c JOIN pg_class t ON t.oid = c.conrelid
    WHERE t.relname = 'dynamic_daily_prices' AND c.contype = 'u'
      AND pg_get_constraintdef(c.oid) = 'UNIQUE (datekey)'
    LIMIT 1;
    IF constraint_name IS NOT NULL THEN
        EXECUTE format('ALTER TABLE dynamic_daily_prices DROP CONSTRAINT %I', constraint_name);
    END IF;
END $$;
CREATE UNIQUE INDEX IF NOT EXISTS idx_dynamic_daily_prices_tenant_date
    ON dynamic_daily_prices(tenant_id, dateKey);
