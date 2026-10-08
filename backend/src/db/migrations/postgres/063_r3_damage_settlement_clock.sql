-- 063: PostgreSQL semantic mirror of R3 damage, repair, settlement, and clock authority.

CREATE TABLE tenant_business_time_zones (
    tenant_id TEXT PRIMARY KEY REFERENCES tenants(id),
    time_zone_id TEXT NOT NULL,
    configured_by TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

INSERT INTO tenant_business_time_zones (tenant_id, time_zone_id, configured_by, created_at, updated_at)
SELECT id, 'Asia/Shanghai', 'migration:063:profile-default',
       to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
       to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
FROM tenants
ON CONFLICT (tenant_id) DO NOTHING;

ALTER TABLE rental_returns ADD COLUMN business_date TEXT;
ALTER TABLE rental_inspections ADD COLUMN condition_code TEXT NOT NULL DEFAULT 'unknown'
    CHECK (condition_code IN ('unknown', 'good', 'damaged', 'missing', 'normal_wear'));
ALTER TABLE rental_inspections ADD COLUMN missing BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE rental_inspections ADD COLUMN normal_wear BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE rental_inspections ADD COLUMN evidence_refs_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE rental_inspections ADD COLUMN business_date TEXT;
ALTER TABLE rental_inspections ADD COLUMN inspected_by TEXT;

CREATE TABLE rental_damage_findings (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, inspection_id TEXT NOT NULL, order_id TEXT NOT NULL,
    device_serial_no TEXT NOT NULL, finding_code TEXT NOT NULL,
    condition_code TEXT NOT NULL CHECK (condition_code IN ('damaged', 'missing', 'normal_wear')),
    description TEXT NOT NULL, evidence_refs_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('open', 'withdrawn', 'resolved')),
    command_idempotency_key TEXT NOT NULL, found_by TEXT NOT NULL, occurred_at TEXT NOT NULL,
    business_date TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (tenant_id, inspection_id, command_idempotency_key),
    FOREIGN KEY (tenant_id, inspection_id) REFERENCES rental_inspections(tenant_id, id)
);

CREATE TABLE rental_liability_decisions (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, finding_id TEXT NOT NULL,
    decision TEXT NOT NULL CHECK (decision IN ('liable', 'not_liable', 'partial', 'manual_review')),
    apportioned_amount_minor BIGINT NOT NULL CHECK (apportioned_amount_minor >= 0),
    currency TEXT NOT NULL CHECK (char_length(currency) = 3), rationale TEXT NOT NULL,
    evidence_refs_json TEXT NOT NULL, status TEXT NOT NULL CHECK (status IN ('open', 'decided')),
    version BIGINT NOT NULL DEFAULT 1 CHECK (version > 0), command_idempotency_key TEXT NOT NULL,
    decided_by TEXT NOT NULL, decided_at TEXT NOT NULL, business_date TEXT NOT NULL,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (tenant_id, finding_id), UNIQUE (tenant_id, command_idempotency_key),
    FOREIGN KEY (tenant_id, finding_id) REFERENCES rental_damage_findings(tenant_id, id)
);

CREATE TABLE rental_repair_cases (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, finding_id TEXT NOT NULL, order_id TEXT NOT NULL,
    device_serial_no TEXT NOT NULL,
    decision TEXT NOT NULL CHECK (decision IN ('repair_required', 'no_action', 'retired', 'lost')),
    status TEXT NOT NULL CHECK (status IN ('repair_required', 'under_repair', 'repaired', 'retired', 'lost', 'no_action')),
    version BIGINT NOT NULL DEFAULT 1 CHECK (version > 0), command_idempotency_key TEXT NOT NULL,
    decided_by TEXT NOT NULL, decided_at TEXT NOT NULL, completed_at TEXT, business_date TEXT NOT NULL,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (tenant_id, finding_id), UNIQUE (tenant_id, command_idempotency_key),
    FOREIGN KEY (tenant_id, finding_id) REFERENCES rental_damage_findings(tenant_id, id)
);

CREATE TABLE rental_settlement_cases (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, order_id TEXT NOT NULL,
    currency TEXT NOT NULL CHECK (char_length(currency) = 3),
    total_amount_minor BIGINT NOT NULL CHECK (total_amount_minor >= 0),
    deposit_deducted_minor BIGINT NOT NULL DEFAULT 0 CHECK (deposit_deducted_minor >= 0),
    status TEXT NOT NULL CHECK (status IN ('proposed', 'accepted', 'disputed', 'under_review', 'resolved', 'settled')),
    facts_hash TEXT NOT NULL, command_idempotency_key TEXT NOT NULL, proposed_by TEXT NOT NULL,
    proposed_at TEXT NOT NULL, accounting_business_date TEXT NOT NULL, accepted_at TEXT, settled_at TEXT,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (tenant_id, order_id), UNIQUE (tenant_id, command_idempotency_key)
);

CREATE TABLE rental_settlement_lines (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, settlement_case_id TEXT NOT NULL, finding_id TEXT NOT NULL,
    liability_decision_id TEXT NOT NULL,
    line_kind TEXT NOT NULL CHECK (line_kind IN ('damage', 'missing_device', 'repair', 'overdue_adjustment')),
    amount_minor BIGINT NOT NULL CHECK (amount_minor >= 0), currency TEXT NOT NULL CHECK (char_length(currency) = 3),
    description TEXT NOT NULL, created_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (tenant_id, settlement_case_id, finding_id),
    FOREIGN KEY (tenant_id, settlement_case_id) REFERENCES rental_settlement_cases(tenant_id, id),
    FOREIGN KEY (tenant_id, finding_id) REFERENCES rental_damage_findings(tenant_id, id),
    FOREIGN KEY (tenant_id, liability_decision_id) REFERENCES rental_liability_decisions(tenant_id, id)
);

CREATE TABLE rental_disputes (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, settlement_case_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('open', 'under_review', 'resolved', 'withdrawn')),
    reason TEXT NOT NULL, evidence_refs_json TEXT NOT NULL, command_idempotency_key TEXT NOT NULL,
    opened_by TEXT NOT NULL, opened_at TEXT NOT NULL, resolved_by TEXT, resolved_at TEXT,
    resolution_note TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (tenant_id, settlement_case_id, command_idempotency_key),
    FOREIGN KEY (tenant_id, settlement_case_id) REFERENCES rental_settlement_cases(tenant_id, id)
);

CREATE TABLE rental_settlement_effect_intents (
    id TEXT NOT NULL, tenant_id TEXT NOT NULL, settlement_case_id TEXT NOT NULL,
    effect_kind TEXT NOT NULL CHECK (effect_kind IN ('deposit_deduction', 'additional_charge')),
    amount_minor BIGINT NOT NULL CHECK (amount_minor > 0), currency TEXT NOT NULL CHECK (char_length(currency) = 3),
    deposit_id TEXT, external_operation_id TEXT, idempotency_key TEXT NOT NULL, request_hash TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('admitted', 'succeeded', 'rejected', 'unknown_outcome', 'manual_resolution_required')),
    created_by TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id), UNIQUE (tenant_id, settlement_case_id, idempotency_key),
    FOREIGN KEY (tenant_id, settlement_case_id) REFERENCES rental_settlement_cases(tenant_id, id),
    FOREIGN KEY (tenant_id, deposit_id) REFERENCES integration_deposits(tenant_id, id),
    FOREIGN KEY (tenant_id, external_operation_id) REFERENCES external_operations(tenant_id, id)
);

CREATE TABLE rental_settlement_reconciliations (
    id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, effect_intent_id TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('effect_confirmed', 'effect_absent', 'manual_required')),
    evidence_ref TEXT NOT NULL, resolved_by TEXT NOT NULL, created_at TEXT NOT NULL, resolved_at TEXT,
    FOREIGN KEY (tenant_id, effect_intent_id) REFERENCES rental_settlement_effect_intents(tenant_id, id)
);

CREATE OR REPLACE FUNCTION reject_r3_effect_intent_mutation() RETURNS trigger AS $$
BEGIN
    IF NEW.tenant_id <> OLD.tenant_id OR NEW.settlement_case_id <> OLD.settlement_case_id
       OR NEW.effect_kind <> OLD.effect_kind OR NEW.amount_minor <> OLD.amount_minor
       OR NEW.currency <> OLD.currency OR NEW.deposit_id IS DISTINCT FROM OLD.deposit_id
       OR NEW.external_operation_id IS DISTINCT FROM OLD.external_operation_id
       OR NEW.idempotency_key <> OLD.idempotency_key OR NEW.request_hash <> OLD.request_hash THEN
        RAISE EXCEPTION 'rental settlement effect intent is immutable';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER rental_settlement_effect_intent_immutable
BEFORE UPDATE ON rental_settlement_effect_intents
FOR EACH ROW EXECUTE FUNCTION reject_r3_effect_intent_mutation();

CREATE INDEX idx_r3_damage_findings_order ON rental_damage_findings(tenant_id, order_id, status);
CREATE INDEX idx_r3_liability_open ON rental_liability_decisions(tenant_id, status, finding_id);
CREATE INDEX idx_r3_repair_order ON rental_repair_cases(tenant_id, order_id, status);
CREATE INDEX idx_r3_settlement_status ON rental_settlement_cases(tenant_id, status, order_id);
CREATE INDEX idx_r3_disputes_active ON rental_disputes(tenant_id, settlement_case_id, status);
CREATE INDEX idx_r3_effect_intents_state ON rental_settlement_effect_intents(tenant_id, state, settlement_case_id);
