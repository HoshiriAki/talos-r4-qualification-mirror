-- 064: R3-A hardening.  R2 Integration Fabric exclusively owns external
-- operation identity and outcome/reconciliation state.  The 063 tables remain
-- renamed historical evidence only; new R3 records are immutable business
-- authorizations and links, never a second provider-outcome state machine.

ALTER TABLE rental_settlement_effect_intents
    RENAME TO r3_legacy_settlement_effect_intents;
ALTER TABLE rental_settlement_reconciliations
    RENAME TO r3_legacy_settlement_reconciliations;

CREATE TABLE rental_settlement_effect_admissions (
    id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    settlement_case_id TEXT NOT NULL,
    effect_kind TEXT NOT NULL CHECK (effect_kind IN ('deposit_deduction', 'additional_charge')),
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    currency TEXT NOT NULL CHECK (length(currency) = 3),
    deposit_id TEXT,
    external_operation_id TEXT,
    idempotency_key TEXT NOT NULL,
    business_authorization_ref TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, settlement_case_id, idempotency_key),
    CHECK (
        (effect_kind = 'deposit_deduction' AND deposit_id IS NOT NULL AND external_operation_id IS NULL)
        OR
        (effect_kind = 'additional_charge' AND deposit_id IS NULL AND external_operation_id IS NOT NULL)
    ),
    FOREIGN KEY (tenant_id, settlement_case_id) REFERENCES rental_settlement_cases(tenant_id, id),
    FOREIGN KEY (tenant_id, deposit_id) REFERENCES integration_deposits(tenant_id, id),
    FOREIGN KEY (tenant_id, external_operation_id) REFERENCES external_operations(tenant_id, id)
);

-- Historical invalid/unlinked charge rows are deliberately not promoted into
-- the active authority table.  They remain auditable in the 063 legacy table.
INSERT INTO rental_settlement_effect_admissions
    (id,tenant_id,settlement_case_id,effect_kind,amount_minor,currency,deposit_id,
     external_operation_id,idempotency_key,business_authorization_ref,created_by,created_at)
SELECT id,tenant_id,settlement_case_id,effect_kind,amount_minor,currency,deposit_id,
       external_operation_id,idempotency_key,'legacy:063:' || settlement_case_id,created_by,created_at
  FROM r3_legacy_settlement_effect_intents
 WHERE effect_kind = 'deposit_deduction'
    OR (effect_kind = 'additional_charge' AND external_operation_id IS NOT NULL);

CREATE TRIGGER r3_effect_admission_immutable
BEFORE UPDATE ON rental_settlement_effect_admissions
BEGIN
    SELECT RAISE(ABORT, 'R3 settlement effect admission is immutable; R2 owns operation state');
END;

CREATE TRIGGER r3_effect_admission_after_settlement_guard
BEFORE INSERT ON rental_settlement_effect_admissions
WHEN EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=NEW.tenant_id AND s.id=NEW.settlement_case_id AND s.status='settled'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot admit a new effect');
END;

CREATE TRIGGER r3_dispute_after_settlement_guard
BEFORE INSERT ON rental_disputes
WHEN EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=NEW.tenant_id AND s.id=NEW.settlement_case_id AND s.status='settled'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot open a dispute');
END;

CREATE TRIGGER r3_repair_after_lifecycle_close_guard
BEFORE INSERT ON rental_repair_cases
WHEN EXISTS (
    SELECT 1 FROM order_lifecycle l
    WHERE l.tenant_id=NEW.tenant_id AND l.order_id=NEW.order_id AND l.commercial_status='closed'
)
BEGIN
    SELECT RAISE(ABORT, 'closed order cannot gain a repair blocker');
END;

CREATE TRIGGER r3_overdue_after_lifecycle_close_guard
BEFORE INSERT ON overdue_records
WHEN EXISTS (
    SELECT 1 FROM order_lifecycle l
    WHERE l.tenant_id=NEW.tenant_id AND l.order_id=NEW.order_id AND l.commercial_status='closed'
)
BEGIN
    SELECT RAISE(ABORT, 'closed order cannot gain an overdue blocker');
END;

CREATE INDEX idx_r3_effect_admissions_case ON rental_settlement_effect_admissions(tenant_id, settlement_case_id, effect_kind);
CREATE INDEX idx_r3_effect_admissions_operation ON rental_settlement_effect_admissions(tenant_id, external_operation_id);
