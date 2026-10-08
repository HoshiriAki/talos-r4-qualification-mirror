-- 064: PostgreSQL semantic mirror.  R2 owns external operation identity and
-- outcome/reconciliation state; R3 stores only immutable business admissions.

ALTER TABLE rental_settlement_effect_intents RENAME TO r3_legacy_settlement_effect_intents;
ALTER TABLE rental_settlement_reconciliations RENAME TO r3_legacy_settlement_reconciliations;

CREATE TABLE rental_settlement_effect_admissions (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, settlement_case_id TEXT NOT NULL,
    effect_kind TEXT NOT NULL CHECK (effect_kind IN ('deposit_deduction', 'additional_charge')),
    amount_minor BIGINT NOT NULL CHECK (amount_minor > 0),
    currency TEXT NOT NULL CHECK (char_length(currency) = 3),
    deposit_id TEXT, external_operation_id TEXT, idempotency_key TEXT NOT NULL,
    business_authorization_ref TEXT NOT NULL, created_by TEXT NOT NULL, created_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (tenant_id, settlement_case_id, idempotency_key),
    CHECK ((effect_kind='deposit_deduction' AND deposit_id IS NOT NULL AND external_operation_id IS NULL)
        OR (effect_kind='additional_charge' AND deposit_id IS NULL AND external_operation_id IS NOT NULL)),
    FOREIGN KEY (tenant_id, settlement_case_id) REFERENCES rental_settlement_cases(tenant_id, id),
    FOREIGN KEY (tenant_id, deposit_id) REFERENCES integration_deposits(tenant_id, id),
    FOREIGN KEY (tenant_id, external_operation_id) REFERENCES external_operations(tenant_id, id)
);

INSERT INTO rental_settlement_effect_admissions
    (id,tenant_id,settlement_case_id,effect_kind,amount_minor,currency,deposit_id,
     external_operation_id,idempotency_key,business_authorization_ref,created_by,created_at)
SELECT id,tenant_id,settlement_case_id,effect_kind,amount_minor,currency,deposit_id,
       external_operation_id,idempotency_key,'legacy:063:' || settlement_case_id,created_by,created_at
  FROM r3_legacy_settlement_effect_intents
 WHERE effect_kind='deposit_deduction'
    OR (effect_kind='additional_charge' AND external_operation_id IS NOT NULL);

CREATE OR REPLACE FUNCTION reject_r3_effect_admission_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'R3 settlement effect admission is immutable; R2 owns operation state';
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER r3_effect_admission_immutable
BEFORE UPDATE ON rental_settlement_effect_admissions
FOR EACH ROW EXECUTE FUNCTION reject_r3_effect_admission_mutation();

CREATE OR REPLACE FUNCTION r3_prevent_post_terminal_blocker() RETURNS trigger AS $$
BEGIN
    IF TG_TABLE_NAME='rental_settlement_effect_admissions' AND EXISTS (
        SELECT 1 FROM rental_settlement_cases s
        WHERE s.tenant_id=NEW.tenant_id AND s.id=NEW.settlement_case_id AND s.status='settled'
    ) THEN RAISE EXCEPTION 'terminal settlement cannot admit a new effect'; END IF;
    IF TG_TABLE_NAME='rental_disputes' AND EXISTS (
        SELECT 1 FROM rental_settlement_cases s
        WHERE s.tenant_id=NEW.tenant_id AND s.id=NEW.settlement_case_id AND s.status='settled'
    ) THEN RAISE EXCEPTION 'terminal settlement cannot open a dispute'; END IF;
    IF TG_TABLE_NAME IN ('rental_repair_cases','overdue_records') AND EXISTS (
        SELECT 1 FROM order_lifecycle l
        WHERE l.tenant_id=NEW.tenant_id AND l.order_id=NEW.order_id AND l.commercial_status='closed'
    ) THEN RAISE EXCEPTION 'closed order cannot gain a terminal blocker'; END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER r3_effect_admission_after_settlement_guard BEFORE INSERT ON rental_settlement_effect_admissions
FOR EACH ROW EXECUTE FUNCTION r3_prevent_post_terminal_blocker();
CREATE TRIGGER r3_dispute_after_settlement_guard BEFORE INSERT ON rental_disputes
FOR EACH ROW EXECUTE FUNCTION r3_prevent_post_terminal_blocker();
CREATE TRIGGER r3_repair_after_lifecycle_close_guard BEFORE INSERT ON rental_repair_cases
FOR EACH ROW EXECUTE FUNCTION r3_prevent_post_terminal_blocker();
CREATE TRIGGER r3_overdue_after_lifecycle_close_guard BEFORE INSERT ON overdue_records
FOR EACH ROW EXECUTE FUNCTION r3_prevent_post_terminal_blocker();

CREATE INDEX idx_r3_effect_admissions_case ON rental_settlement_effect_admissions(tenant_id, settlement_case_id, effect_kind);
CREATE INDEX idx_r3_effect_admissions_operation ON rental_settlement_effect_admissions(tenant_id, external_operation_id);
