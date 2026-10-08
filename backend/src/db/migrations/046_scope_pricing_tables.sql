-- MVP0: tenant-scope pricing storage and replace the legacy global date key.
ALTER TABLE pricing_configs ADD COLUMN tenant_id TEXT NOT NULL DEFAULT 'default';
CREATE UNIQUE INDEX IF NOT EXISTS idx_pricing_configs_tenant ON pricing_configs(tenant_id);

ALTER TABLE model_base_prices RENAME TO model_base_prices_legacy;
CREATE TABLE model_base_prices (
    modelId TEXT NOT NULL,
    weekdayPrice REAL NOT NULL,
    weekendPrice REAL NOT NULL,
    updatedBy TEXT DEFAULT '',
    createdAt TEXT NOT NULL,
    updatedAt TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    PRIMARY KEY (tenant_id, modelId),
    FOREIGN KEY(modelId) REFERENCES device_models(id) ON DELETE CASCADE
);
INSERT INTO model_base_prices
    (modelId, weekdayPrice, weekendPrice, updatedBy, createdAt, updatedAt, tenant_id)
SELECT modelId, weekdayPrice, weekendPrice, updatedBy, createdAt, updatedAt, 'default'
FROM model_base_prices_legacy;
DROP TABLE model_base_prices_legacy;

ALTER TABLE dynamic_daily_prices RENAME TO dynamic_daily_prices_legacy;
CREATE TABLE dynamic_daily_prices (
    id TEXT PRIMARY KEY,
    dateKey TEXT NOT NULL,
    price REAL NOT NULL,
    updatedBy TEXT DEFAULT '',
    createdAt TEXT NOT NULL,
    updatedAt TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    UNIQUE(tenant_id, dateKey)
);
INSERT INTO dynamic_daily_prices
    (id, dateKey, price, updatedBy, createdAt, updatedAt, tenant_id)
SELECT id, dateKey, price, updatedBy, createdAt, updatedAt, COALESCE(tenant_id, 'default')
FROM dynamic_daily_prices_legacy;
DROP TABLE dynamic_daily_prices_legacy;
CREATE INDEX IF NOT EXISTS idx_dynamic_daily_prices_date_key ON dynamic_daily_prices(dateKey);
CREATE INDEX IF NOT EXISTS idx_daily_prices_tenant ON dynamic_daily_prices(tenant_id, dateKey);
