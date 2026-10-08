-- R4-P8: procurement identity follows the tenant-scoped device serial namespace.
CREATE TABLE asset_purchases_new_074 (
    id TEXT PRIMARY KEY,
    device_serial_no TEXT NOT NULL,
    purchase_price REAL NOT NULL DEFAULT 0,
    purchase_date TEXT NOT NULL DEFAULT '',
    vendor TEXT NOT NULL DEFAULT '',
    invoice_no TEXT NOT NULL DEFAULT '',
    replacement_value REAL NOT NULL DEFAULT 0,
    notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT '',
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    UNIQUE(tenant_id, device_serial_no)
);

INSERT INTO asset_purchases_new_074
    (id,device_serial_no,purchase_price,purchase_date,vendor,invoice_no,
     replacement_value,notes,created_at,tenant_id)
SELECT id,device_serial_no,purchase_price,purchase_date,vendor,invoice_no,
       replacement_value,notes,created_at,tenant_id
FROM asset_purchases;

DROP TABLE asset_purchases;
ALTER TABLE asset_purchases_new_074 RENAME TO asset_purchases;

CREATE INDEX idx_asset_purchases_device
    ON asset_purchases(device_serial_no);
CREATE INDEX idx_asset_purchases_tenant
    ON asset_purchases(tenant_id, device_serial_no);
