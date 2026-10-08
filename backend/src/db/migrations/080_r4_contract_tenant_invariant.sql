-- R4-P8: normalize Contract identity and tenant-local referential integrity.
-- The executor disables foreign_keys around this table rebuild and runs
-- PRAGMA foreign_key_check before commit.

CREATE TABLE contract_templates_new_080 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    content_json TEXT NOT NULL DEFAULT '{}',
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    UNIQUE(id, tenant_id)
);
INSERT INTO contract_templates_new_080
    (id,name,content_json,is_active,created_at,updated_at,tenant_id)
SELECT id,name,content_json,is_active,created_at,created_at,
       COALESCE(NULLIF(tenant_id,''),'default')
FROM contract_templates;

CREATE TABLE contracts_new_080 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id TEXT NOT NULL,
    template_id INTEGER,
    customer_name TEXT NOT NULL,
    customer_phone TEXT NOT NULL,
    device_value REAL NOT NULL DEFAULT 0.0,
    content_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'draft'
        CHECK(status IN ('draft','generated','signed','verified','expired','voided')),
    signed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    UNIQUE(id, tenant_id),
    FOREIGN KEY(order_id, tenant_id)
        REFERENCES orders(id, tenant_id) ON DELETE RESTRICT,
    FOREIGN KEY(template_id, tenant_id)
        REFERENCES contract_templates_new_080(id, tenant_id) ON DELETE RESTRICT
);
INSERT INTO contracts_new_080
    (id,order_id,template_id,customer_name,customer_phone,device_value,
     content_json,status,signed_at,created_at,updated_at,tenant_id)
SELECT id,CAST(order_id AS TEXT),template_id,customer_name,customer_phone,device_value,
       content_json,status,signed_at,created_at,updated_at,
       COALESCE(NULLIF(tenant_id,''),'default')
FROM contracts;

CREATE TABLE e_signatures_new_080 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    contract_id INTEGER NOT NULL,
    signer_name TEXT NOT NULL,
    signer_phone TEXT NOT NULL,
    signature_data TEXT NOT NULL,
    signed_at TEXT NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    FOREIGN KEY(contract_id, tenant_id)
        REFERENCES contracts_new_080(id, tenant_id) ON DELETE CASCADE
);
INSERT INTO e_signatures_new_080
    (id,contract_id,signer_name,signer_phone,signature_data,signed_at,tenant_id)
SELECT id,contract_id,signer_name,signer_phone,signature_data,signed_at,
       COALESCE(NULLIF(tenant_id,''),'default')
FROM e_signatures;

DROP TABLE e_signatures;
DROP TABLE contracts;
DROP TABLE contract_templates;

ALTER TABLE contract_templates_new_080 RENAME TO contract_templates;
ALTER TABLE contracts_new_080 RENAME TO contracts;
ALTER TABLE e_signatures_new_080 RENAME TO e_signatures;

CREATE INDEX idx_contract_templates_tenant
    ON contract_templates(tenant_id, name);
CREATE INDEX idx_contracts_order ON contracts(order_id);
CREATE INDEX idx_contracts_status ON contracts(status);
CREATE INDEX idx_contracts_phone ON contracts(customer_phone);
CREATE INDEX idx_contracts_tenant ON contracts(tenant_id, order_id);
CREATE INDEX idx_esign_contract ON e_signatures(contract_id);
CREATE INDEX idx_e_signatures_tenant ON e_signatures(tenant_id, contract_id);
