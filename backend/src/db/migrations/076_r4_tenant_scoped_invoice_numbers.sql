-- R4-P8: invoice numbering is tenant-local, not a global tenant-leaking sequence.
CREATE TABLE invoices_new_076 (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    invoice_no TEXT NOT NULL,
    type TEXT NOT NULL CHECK(type IN ('普通发票', '专用发票')),
    amount REAL NOT NULL,
    tax_rate REAL NOT NULL DEFAULT 0.13,
    tax_amount REAL NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('issued', 'voided', 'red_flushed')),
    tenant_id TEXT NOT NULL,
    issued_at TEXT NOT NULL,
    voided_at TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(tenant_id, invoice_no)
);

INSERT INTO invoices_new_076
    (id,order_id,invoice_no,type,amount,tax_rate,tax_amount,status,tenant_id,
     issued_at,voided_at,created_at)
SELECT id,order_id,invoice_no,type,amount,tax_rate,tax_amount,status,tenant_id,
       issued_at,voided_at,created_at
FROM invoices;

DROP TABLE invoices;
ALTER TABLE invoices_new_076 RENAME TO invoices;

CREATE INDEX idx_invoices_tenant_order
    ON invoices(tenant_id, order_id, created_at);
CREATE INDEX idx_invoices_tenant_issued
    ON invoices(tenant_id, issued_at);
