-- R4-P8: enforce the Credit authority's tenant-local customer identity invariant.
-- Historical migration 045 qualified public.credit_scores explicitly and can be skipped
-- by schema-isolated deployments that rely on search_path.

ALTER TABLE credit_scores
    ADD COLUMN IF NOT EXISTS tenant_id TEXT;

UPDATE credit_scores
SET tenant_id = 'default'
WHERE tenant_id IS NULL OR tenant_id = '';

ALTER TABLE credit_scores
    ALTER COLUMN tenant_id SET NOT NULL;

DROP INDEX IF EXISTS idx_credit_scores_phone;

CREATE UNIQUE INDEX IF NOT EXISTS idx_credit_scores_tenant_phone
    ON credit_scores(tenant_id, customer_phone);
