-- Pre-Alpha clean-database invariant: core business rows always have a tenant.
ALTER TABLE orders ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE devices ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE warehouses ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE device_models ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE contracts ALTER COLUMN tenant_id SET NOT NULL;
