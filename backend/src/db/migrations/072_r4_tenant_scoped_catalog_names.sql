-- R4-P8: move catalog-name uniqueness into the tenant namespace.
-- SQLite generalized ALTER TABLE safety rule:
-- create new table -> copy -> drop old -> rename new into canonical name.
-- The executor disables foreign_keys before opening the migration transaction,
-- runs PRAGMA foreign_key_check before commit, then restores the prior setting.

CREATE TABLE device_models_new_072 (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    prefix TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    createdAt TEXT NOT NULL,
    updatedAt TEXT NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    UNIQUE(tenant_id, name)
);
INSERT INTO device_models_new_072
    (id,name,category,prefix,enabled,createdAt,updatedAt,tenant_id)
SELECT id,name,category,prefix,enabled,createdAt,updatedAt,tenant_id
FROM device_models;
DROP TABLE device_models;
ALTER TABLE device_models_new_072 RENAME TO device_models;
CREATE UNIQUE INDEX idx_device_models_id_tenant_unique
    ON device_models(id, tenant_id);

CREATE TABLE warehouses_new_072 (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    type TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    createdAt TEXT NOT NULL,
    updatedAt TEXT NOT NULL,
    address TEXT DEFAULT '',
    contactName TEXT DEFAULT '',
    contactPhone TEXT DEFAULT '',
    notes TEXT DEFAULT '',
    capacity INTEGER DEFAULT 0,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    UNIQUE(tenant_id, name)
);
INSERT INTO warehouses_new_072
    (id,name,type,enabled,createdAt,updatedAt,address,contactName,contactPhone,notes,capacity,tenant_id)
SELECT id,name,type,enabled,createdAt,updatedAt,address,contactName,contactPhone,notes,capacity,tenant_id
FROM warehouses;
DROP TABLE warehouses;
ALTER TABLE warehouses_new_072 RENAME TO warehouses;
CREATE INDEX idx_warehouses_tenant
    ON warehouses(tenant_id, id);
