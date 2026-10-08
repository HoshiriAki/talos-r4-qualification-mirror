-- R4-P8: depreciation log rows are tenant-owned durable records.
ALTER TABLE depreciation_log
    ALTER COLUMN tenant_id SET NOT NULL;

ALTER TABLE depreciation_log
    ADD CONSTRAINT fk_depreciation_log_tenant
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE RESTRICT;

CREATE INDEX IF NOT EXISTS idx_depreciation_log_tenant_period
    ON depreciation_log(tenant_id, period);
