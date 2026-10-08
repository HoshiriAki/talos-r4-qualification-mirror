-- R4-P8: normalize Contract tenant-local referential integrity.
ALTER TABLE contract_templates
    ADD COLUMN IF NOT EXISTS updated_at TEXT;
UPDATE contract_templates
SET updated_at=created_at
WHERE updated_at IS NULL OR updated_at='';

UPDATE contract_templates
SET tenant_id='default'
WHERE tenant_id IS NULL OR tenant_id='';
UPDATE contracts
SET tenant_id='default'
WHERE tenant_id IS NULL OR tenant_id='';
UPDATE e_signatures
SET tenant_id='default'
WHERE tenant_id IS NULL OR tenant_id='';

ALTER TABLE contract_templates ALTER COLUMN updated_at SET NOT NULL;
ALTER TABLE contract_templates ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE contracts ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE e_signatures ALTER COLUMN tenant_id SET NOT NULL;

SELECT setval(
    pg_get_serial_sequence('contract_templates', 'id'),
    GREATEST((SELECT COALESCE(MAX(id), 0) FROM contract_templates), 1),
    true
);

ALTER TABLE contract_templates
    ADD CONSTRAINT contract_templates_id_tenant_key UNIQUE (id, tenant_id);
ALTER TABLE contracts
    ADD CONSTRAINT contracts_id_tenant_key UNIQUE (id, tenant_id);

ALTER TABLE contracts DROP CONSTRAINT IF EXISTS contracts_order_id_fkey;
ALTER TABLE contracts DROP CONSTRAINT IF EXISTS contracts_template_id_fkey;
ALTER TABLE e_signatures DROP CONSTRAINT IF EXISTS e_signatures_contract_id_fkey;

ALTER TABLE contracts
    ADD CONSTRAINT contracts_order_tenant_fkey
    FOREIGN KEY (order_id, tenant_id)
    REFERENCES orders(id, tenant_id) ON DELETE RESTRICT;
ALTER TABLE contracts
    ADD CONSTRAINT contracts_template_tenant_fkey
    FOREIGN KEY (template_id, tenant_id)
    REFERENCES contract_templates(id, tenant_id) ON DELETE RESTRICT;
ALTER TABLE e_signatures
    ADD CONSTRAINT e_signatures_contract_tenant_fkey
    FOREIGN KEY (contract_id, tenant_id)
    REFERENCES contracts(id, tenant_id) ON DELETE CASCADE;

ALTER TABLE contract_templates
    ADD CONSTRAINT fk_contract_templates_tenant
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE RESTRICT;
ALTER TABLE e_signatures
    ADD CONSTRAINT fk_e_signatures_tenant
    FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE RESTRICT;

CREATE INDEX IF NOT EXISTS idx_contract_templates_tenant
    ON contract_templates(tenant_id, name);
CREATE INDEX IF NOT EXISTS idx_contracts_tenant
    ON contracts(tenant_id, order_id);
CREATE INDEX IF NOT EXISTS idx_e_signatures_tenant
    ON e_signatures(tenant_id, contract_id);
