-- R4-P8: procurement identity follows the tenant-scoped device serial namespace.
ALTER TABLE asset_purchases
    DROP CONSTRAINT IF EXISTS asset_purchases_device_serial_no_key;

ALTER TABLE asset_purchases
    ALTER COLUMN tenant_id SET NOT NULL;

ALTER TABLE asset_purchases
    ADD CONSTRAINT fk_asset_purchases_tenant
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE RESTRICT;

CREATE UNIQUE INDEX idx_asset_purchases_tenant_serial_unique
    ON asset_purchases(tenant_id, device_serial_no);
