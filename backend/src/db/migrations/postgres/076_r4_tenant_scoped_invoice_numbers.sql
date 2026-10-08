-- R4-P8: invoice numbering is tenant-local, not a global tenant-leaking sequence.
ALTER TABLE invoices
    DROP CONSTRAINT IF EXISTS invoices_invoice_no_key;

CREATE UNIQUE INDEX IF NOT EXISTS idx_invoices_tenant_invoice_no_unique
    ON invoices(tenant_id, invoice_no);

CREATE INDEX IF NOT EXISTS idx_invoices_tenant_order
    ON invoices(tenant_id, order_id, created_at);
CREATE INDEX IF NOT EXISTS idx_invoices_tenant_issued
    ON invoices(tenant_id, issued_at);
