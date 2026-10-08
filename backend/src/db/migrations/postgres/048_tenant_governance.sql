-- MVP1 tenant governance evidence. No FK to tenants: lifecycle deletion must not erase evidence.
CREATE TABLE IF NOT EXISTS change_intents (
    id TEXT PRIMARY KEY,
    target_tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    reason TEXT NOT NULL CHECK(char_length(reason) BETWEEN 1 AND 1000),
    intended_outcome TEXT NOT NULL CHECK(char_length(intended_outcome) BETWEEN 1 AND 2000),
    impact TEXT NOT NULL DEFAULT '' CHECK(char_length(impact) <= 4000),
    cost_minor BIGINT,
    currency TEXT CHECK(currency IS NULL OR (char_length(currency) = 3 AND currency = upper(currency))),
    source TEXT NOT NULL CHECK(source = 'platform_governance'),
    correlation_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'recorded' CHECK(status = 'recorded'),
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_change_intents_tenant_created
    ON change_intents(target_tenant_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_change_intents_actor_created
    ON change_intents(actor_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_governance_cursor
    ON audit_logs(createdAt DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_governance_tenant_cursor
    ON audit_logs(tenant_id, createdAt DESC, id DESC);

CREATE OR REPLACE FUNCTION reject_change_intent_mutation()
RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'change_intents are append-only';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_change_intents_no_update ON change_intents;
CREATE TRIGGER trg_change_intents_no_update
BEFORE UPDATE ON change_intents
FOR EACH ROW EXECUTE FUNCTION reject_change_intent_mutation();

DROP TRIGGER IF EXISTS trg_change_intents_no_delete ON change_intents;
CREATE TRIGGER trg_change_intents_no_delete
BEFORE DELETE ON change_intents
FOR EACH ROW EXECUTE FUNCTION reject_change_intent_mutation();
