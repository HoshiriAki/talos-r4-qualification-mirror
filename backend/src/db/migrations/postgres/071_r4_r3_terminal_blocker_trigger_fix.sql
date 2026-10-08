-- 071: Repair R3 terminal-blocker trigger field access across heterogeneous tables.
-- The shared trigger function is attached to rows with different shapes. Direct NEW.field
-- references fail when a trigger fires for a table that does not expose that field.
-- JSON extraction keeps the guard schema-safe while preserving the same authority rules.

CREATE OR REPLACE FUNCTION r3_prevent_post_terminal_blocker() RETURNS trigger AS $$
DECLARE
    new_row JSONB := to_jsonb(NEW);
BEGIN
    IF TG_TABLE_NAME = 'rental_settlement_effect_admissions' THEN
        IF EXISTS (
            SELECT 1
              FROM rental_settlement_cases s
             WHERE s.tenant_id = NEW.tenant_id
               AND s.id = new_row ->> 'settlement_case_id'
               AND s.status = 'settled'
        ) THEN
            RAISE EXCEPTION 'terminal settlement cannot admit a new effect';
        END IF;
    ELSIF TG_TABLE_NAME = 'rental_disputes' THEN
        IF EXISTS (
            SELECT 1
              FROM rental_settlement_cases s
             WHERE s.tenant_id = NEW.tenant_id
               AND s.id = new_row ->> 'settlement_case_id'
               AND s.status = 'settled'
        ) THEN
            RAISE EXCEPTION 'terminal settlement cannot open a dispute';
        END IF;
    ELSIF TG_TABLE_NAME IN ('rental_repair_cases', 'overdue_records') THEN
        IF EXISTS (
            SELECT 1
              FROM order_lifecycle l
             WHERE l.tenant_id = NEW.tenant_id
               AND l.order_id = new_row ->> 'order_id'
               AND l.commercial_status = 'closed'
        ) THEN
            RAISE EXCEPTION 'closed order cannot gain a terminal blocker';
        END IF;
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
