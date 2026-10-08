-- 065: PostgreSQL semantic mirror of the R3 terminal settlement seal.

CREATE OR REPLACE FUNCTION r3_prevent_terminal_settlement_fact_mutation() RETURNS trigger AS $$
BEGIN
    IF TG_TABLE_NAME='rental_settlement_cases' THEN
        IF TG_OP='UPDATE' AND OLD.status='settled' THEN
            RAISE EXCEPTION 'terminal R3 settlement case is sealed';
        END IF;
    ELSIF TG_TABLE_NAME='rental_damage_findings' THEN
        IF EXISTS (
            SELECT 1 FROM rental_settlement_cases s
            WHERE s.tenant_id=NEW.tenant_id AND s.order_id=NEW.order_id AND s.status='settled'
        ) THEN RAISE EXCEPTION 'terminal settlement cannot add or change a damage finding'; END IF;
        IF TG_OP='UPDATE' AND EXISTS (
            SELECT 1 FROM rental_settlement_cases s
            WHERE s.tenant_id=OLD.tenant_id AND s.order_id=OLD.order_id AND s.status='settled'
        ) THEN RAISE EXCEPTION 'terminal settlement cannot add or change a damage finding'; END IF;
    ELSIF TG_TABLE_NAME='rental_liability_decisions' THEN
        IF EXISTS (
            SELECT 1 FROM rental_damage_findings f
            JOIN rental_settlement_cases s ON s.tenant_id=f.tenant_id AND s.order_id=f.order_id
            WHERE f.tenant_id=NEW.tenant_id AND f.id=NEW.finding_id AND s.status='settled'
        ) THEN RAISE EXCEPTION 'terminal settlement cannot add or change liability'; END IF;
        IF TG_OP='UPDATE' AND EXISTS (
            SELECT 1 FROM rental_damage_findings f
            JOIN rental_settlement_cases s ON s.tenant_id=f.tenant_id AND s.order_id=f.order_id
            WHERE f.tenant_id=OLD.tenant_id AND f.id=OLD.finding_id AND s.status='settled'
        ) THEN RAISE EXCEPTION 'terminal settlement cannot add or change liability'; END IF;
    ELSIF TG_TABLE_NAME='rental_repair_cases' THEN
        IF EXISTS (
            SELECT 1 FROM rental_settlement_cases s
            WHERE s.tenant_id=NEW.tenant_id AND s.order_id=NEW.order_id AND s.status='settled'
        ) OR EXISTS (
            SELECT 1 FROM rental_damage_findings f
            JOIN rental_settlement_cases s ON s.tenant_id=f.tenant_id AND s.order_id=f.order_id
            WHERE f.tenant_id=NEW.tenant_id AND f.id=NEW.finding_id AND s.status='settled'
        ) OR EXISTS (
            SELECT 1 FROM order_lifecycle l
            WHERE l.tenant_id=NEW.tenant_id AND l.order_id=NEW.order_id AND l.commercial_status='closed'
        ) THEN RAISE EXCEPTION 'terminal settlement or closed order cannot add or change repair'; END IF;
        IF TG_OP='UPDATE' AND (
            EXISTS (SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.order_id=OLD.order_id AND s.status='settled')
            OR EXISTS (
                SELECT 1 FROM rental_damage_findings f
                JOIN rental_settlement_cases s ON s.tenant_id=f.tenant_id AND s.order_id=f.order_id
                WHERE f.tenant_id=OLD.tenant_id AND f.id=OLD.finding_id AND s.status='settled'
            ) OR EXISTS (
                SELECT 1 FROM order_lifecycle l
                WHERE l.tenant_id=OLD.tenant_id AND l.order_id=OLD.order_id AND l.commercial_status='closed'
            )
        ) THEN RAISE EXCEPTION 'terminal settlement or closed order cannot add or change repair'; END IF;
    ELSIF TG_TABLE_NAME IN ('rental_disputes','rental_settlement_lines') THEN
        IF EXISTS (
            SELECT 1 FROM rental_settlement_cases s
            WHERE s.tenant_id=NEW.tenant_id AND s.id=NEW.settlement_case_id AND s.status='settled'
        ) THEN RAISE EXCEPTION 'terminal settlement cannot add or change a settlement fact'; END IF;
        IF TG_OP='UPDATE' AND EXISTS (
            SELECT 1 FROM rental_settlement_cases s
            WHERE s.tenant_id=OLD.tenant_id AND s.id=OLD.settlement_case_id AND s.status='settled'
        ) THEN RAISE EXCEPTION 'terminal settlement cannot add or change a settlement fact'; END IF;
    ELSIF TG_TABLE_NAME='overdue_records'
      AND NEW.status IN ('active','escalated_d1','escalated_d3','escalated_d7') THEN
        IF EXISTS (
            SELECT 1 FROM rental_settlement_cases s
            WHERE s.tenant_id=NEW.tenant_id AND s.order_id=NEW.order_id AND s.status='settled'
        ) OR EXISTS (
            SELECT 1 FROM order_lifecycle l
            WHERE l.tenant_id=NEW.tenant_id AND l.order_id=NEW.order_id AND l.commercial_status='closed'
        ) THEN RAISE EXCEPTION 'terminal settlement or closed order cannot activate overdue'; END IF;
        IF TG_OP='UPDATE' AND (
            EXISTS (SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.order_id=OLD.order_id AND s.status='settled')
            OR EXISTS (SELECT 1 FROM order_lifecycle l WHERE l.tenant_id=OLD.tenant_id AND l.order_id=OLD.order_id AND l.commercial_status='closed')
        ) THEN RAISE EXCEPTION 'terminal settlement or closed order cannot activate overdue'; END IF;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION r3_prevent_terminal_settlement_fact_deletion() RETURNS trigger AS $$
BEGIN
    IF TG_TABLE_NAME='rental_settlement_cases' AND OLD.status='settled' THEN
        RAISE EXCEPTION 'terminal R3 settlement case is sealed';
    END IF;
    IF TG_TABLE_NAME='rental_damage_findings' AND EXISTS (
        SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.order_id=OLD.order_id AND s.status='settled'
    ) THEN RAISE EXCEPTION 'terminal settlement cannot delete a damage finding'; END IF;
    IF TG_TABLE_NAME='rental_liability_decisions' AND EXISTS (
        SELECT 1 FROM rental_damage_findings f JOIN rental_settlement_cases s ON s.tenant_id=f.tenant_id AND s.order_id=f.order_id
        WHERE f.tenant_id=OLD.tenant_id AND f.id=OLD.finding_id AND s.status='settled'
    ) THEN RAISE EXCEPTION 'terminal settlement cannot delete liability'; END IF;
    IF TG_TABLE_NAME='rental_repair_cases' AND (
        EXISTS (SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.order_id=OLD.order_id AND s.status='settled')
        OR EXISTS (SELECT 1 FROM order_lifecycle l WHERE l.tenant_id=OLD.tenant_id AND l.order_id=OLD.order_id AND l.commercial_status='closed')
    ) THEN RAISE EXCEPTION 'terminal settlement or closed order cannot delete repair'; END IF;
    IF TG_TABLE_NAME IN ('rental_disputes','rental_settlement_lines','rental_settlement_effect_admissions') AND EXISTS (
        SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.id=OLD.settlement_case_id AND s.status='settled'
    ) THEN RAISE EXCEPTION 'terminal settlement cannot delete a settlement fact'; END IF;
    IF TG_TABLE_NAME='overdue_records' AND OLD.status IN ('active','escalated_d1','escalated_d3','escalated_d7') AND EXISTS (
        SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.order_id=OLD.order_id AND s.status='settled'
    ) THEN RAISE EXCEPTION 'terminal settlement cannot delete active overdue'; END IF;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER r3_terminal_settlement_case_seal
BEFORE UPDATE ON rental_settlement_cases
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_mutation();
CREATE TRIGGER r3_terminal_settlement_case_delete_seal
BEFORE DELETE ON rental_settlement_cases
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_deletion();
CREATE TRIGGER r3_damage_finding_after_terminal_settlement_guard
BEFORE INSERT OR UPDATE ON rental_damage_findings
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_mutation();
CREATE TRIGGER r3_damage_finding_delete_after_terminal_settlement_guard
BEFORE DELETE ON rental_damage_findings
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_deletion();
CREATE TRIGGER r3_liability_after_terminal_settlement_guard
BEFORE INSERT OR UPDATE ON rental_liability_decisions
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_mutation();
CREATE TRIGGER r3_liability_delete_after_terminal_settlement_guard
BEFORE DELETE ON rental_liability_decisions
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_deletion();
CREATE TRIGGER r3_repair_after_terminal_settlement_guard
BEFORE INSERT OR UPDATE ON rental_repair_cases
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_mutation();
CREATE TRIGGER r3_repair_delete_after_terminal_settlement_guard
BEFORE DELETE ON rental_repair_cases
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_deletion();
CREATE TRIGGER r3_dispute_after_terminal_settlement_seal
BEFORE INSERT OR UPDATE ON rental_disputes
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_mutation();
CREATE TRIGGER r3_dispute_delete_after_terminal_settlement_seal
BEFORE DELETE ON rental_disputes
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_deletion();
CREATE TRIGGER r3_settlement_line_after_terminal_settlement_guard
BEFORE INSERT OR UPDATE ON rental_settlement_lines
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_mutation();
CREATE TRIGGER r3_settlement_line_delete_after_terminal_settlement_guard
BEFORE DELETE ON rental_settlement_lines
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_deletion();
CREATE TRIGGER r3_effect_admission_delete_after_terminal_settlement_guard
BEFORE DELETE ON rental_settlement_effect_admissions
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_deletion();
CREATE TRIGGER r3_overdue_active_after_terminal_settlement_guard
BEFORE INSERT OR UPDATE ON overdue_records
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_mutation();
CREATE TRIGGER r3_overdue_active_delete_after_terminal_settlement_guard
BEFORE DELETE ON overdue_records
FOR EACH ROW EXECUTE FUNCTION r3_prevent_terminal_settlement_fact_deletion();
