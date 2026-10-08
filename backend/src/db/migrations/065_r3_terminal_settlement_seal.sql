-- 065: A terminal R3 settlement seals the business facts that made its exact
-- accounting and close decision true.  A future approved amend/reopen flow is
-- the only permitted way to change these facts after settlement.

CREATE TRIGGER r3_terminal_settlement_case_seal
BEFORE UPDATE ON rental_settlement_cases
WHEN OLD.status='settled'
BEGIN
    SELECT RAISE(ABORT, 'terminal R3 settlement case is sealed');
END;

CREATE TRIGGER r3_terminal_settlement_case_delete_seal
BEFORE DELETE ON rental_settlement_cases
WHEN OLD.status='settled'
BEGIN
    SELECT RAISE(ABORT, 'terminal R3 settlement case is sealed');
END;

CREATE TRIGGER r3_damage_finding_insert_after_terminal_settlement_guard
BEFORE INSERT ON rental_damage_findings
WHEN EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=NEW.tenant_id AND s.order_id=NEW.order_id AND s.status='settled'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot add or change a damage finding');
END;

CREATE TRIGGER r3_damage_finding_update_after_terminal_settlement_guard
BEFORE UPDATE ON rental_damage_findings
WHEN EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=NEW.tenant_id AND s.order_id=NEW.order_id AND s.status='settled'
)
OR EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=OLD.tenant_id AND s.order_id=OLD.order_id AND s.status='settled'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot add or change a damage finding');
END;

CREATE TRIGGER r3_damage_finding_delete_after_terminal_settlement_guard
BEFORE DELETE ON rental_damage_findings
WHEN EXISTS (SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.order_id=OLD.order_id AND s.status='settled')
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot delete a damage finding');
END;

CREATE TRIGGER r3_liability_insert_after_terminal_settlement_guard
BEFORE INSERT ON rental_liability_decisions
WHEN EXISTS (
    SELECT 1 FROM rental_damage_findings f
    JOIN rental_settlement_cases s ON s.tenant_id=f.tenant_id AND s.order_id=f.order_id
    WHERE f.tenant_id=NEW.tenant_id AND f.id=NEW.finding_id AND s.status='settled'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot add or change liability');
END;

CREATE TRIGGER r3_liability_update_after_terminal_settlement_guard
BEFORE UPDATE ON rental_liability_decisions
WHEN EXISTS (
    SELECT 1 FROM rental_damage_findings f
    JOIN rental_settlement_cases s ON s.tenant_id=f.tenant_id AND s.order_id=f.order_id
    WHERE f.tenant_id=NEW.tenant_id AND f.id=NEW.finding_id AND s.status='settled'
)
OR EXISTS (
    SELECT 1 FROM rental_damage_findings f
    JOIN rental_settlement_cases s ON s.tenant_id=f.tenant_id AND s.order_id=f.order_id
    WHERE f.tenant_id=OLD.tenant_id AND f.id=OLD.finding_id AND s.status='settled'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot add or change liability');
END;

CREATE TRIGGER r3_liability_delete_after_terminal_settlement_guard
BEFORE DELETE ON rental_liability_decisions
WHEN EXISTS (
    SELECT 1 FROM rental_damage_findings f JOIN rental_settlement_cases s ON s.tenant_id=f.tenant_id AND s.order_id=f.order_id
    WHERE f.tenant_id=OLD.tenant_id AND f.id=OLD.finding_id AND s.status='settled'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot delete liability');
END;

CREATE TRIGGER r3_repair_insert_after_terminal_settlement_guard
BEFORE INSERT ON rental_repair_cases
WHEN EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=NEW.tenant_id AND s.order_id=NEW.order_id AND s.status='settled'
)
OR EXISTS (
    SELECT 1 FROM rental_damage_findings f
    JOIN rental_settlement_cases s ON s.tenant_id=f.tenant_id AND s.order_id=f.order_id
    WHERE f.tenant_id=NEW.tenant_id AND f.id=NEW.finding_id AND s.status='settled'
)
OR EXISTS (
    SELECT 1 FROM order_lifecycle l
    WHERE l.tenant_id=NEW.tenant_id AND l.order_id=NEW.order_id AND l.commercial_status='closed'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement or closed order cannot add or change repair');
END;

CREATE TRIGGER r3_repair_update_after_terminal_settlement_guard
BEFORE UPDATE ON rental_repair_cases
WHEN EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=NEW.tenant_id AND s.order_id=NEW.order_id AND s.status='settled'
)
OR EXISTS (
    SELECT 1 FROM rental_damage_findings f
    JOIN rental_settlement_cases s ON s.tenant_id=f.tenant_id AND s.order_id=f.order_id
    WHERE f.tenant_id=NEW.tenant_id AND f.id=NEW.finding_id AND s.status='settled'
)
OR EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=OLD.tenant_id AND s.order_id=OLD.order_id AND s.status='settled'
)
OR EXISTS (
    SELECT 1 FROM rental_damage_findings f
    JOIN rental_settlement_cases s ON s.tenant_id=f.tenant_id AND s.order_id=f.order_id
    WHERE f.tenant_id=OLD.tenant_id AND f.id=OLD.finding_id AND s.status='settled'
)
OR EXISTS (
    SELECT 1 FROM order_lifecycle l
    WHERE l.tenant_id=NEW.tenant_id AND l.order_id=NEW.order_id AND l.commercial_status='closed'
)
OR EXISTS (
    SELECT 1 FROM order_lifecycle l
    WHERE l.tenant_id=OLD.tenant_id AND l.order_id=OLD.order_id AND l.commercial_status='closed'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement or closed order cannot add or change repair');
END;

CREATE TRIGGER r3_repair_delete_after_terminal_settlement_guard
BEFORE DELETE ON rental_repair_cases
WHEN EXISTS (SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.order_id=OLD.order_id AND s.status='settled')
OR EXISTS (SELECT 1 FROM order_lifecycle l WHERE l.tenant_id=OLD.tenant_id AND l.order_id=OLD.order_id AND l.commercial_status='closed')
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement or closed order cannot delete repair');
END;

CREATE TRIGGER r3_dispute_insert_after_terminal_settlement_seal
BEFORE INSERT ON rental_disputes
WHEN EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=NEW.tenant_id AND s.id=NEW.settlement_case_id AND s.status='settled'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot add or change a dispute');
END;

CREATE TRIGGER r3_dispute_update_after_terminal_settlement_seal
BEFORE UPDATE ON rental_disputes
WHEN EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=NEW.tenant_id AND s.id=NEW.settlement_case_id AND s.status='settled'
)
OR EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=OLD.tenant_id AND s.id=OLD.settlement_case_id AND s.status='settled'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot add or change a dispute');
END;

CREATE TRIGGER r3_dispute_delete_after_terminal_settlement_seal
BEFORE DELETE ON rental_disputes
WHEN EXISTS (SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.id=OLD.settlement_case_id AND s.status='settled')
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot delete a dispute');
END;

CREATE TRIGGER r3_settlement_line_insert_after_terminal_settlement_guard
BEFORE INSERT ON rental_settlement_lines
WHEN EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=NEW.tenant_id AND s.id=NEW.settlement_case_id AND s.status='settled'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot add or change a settlement line');
END;

CREATE TRIGGER r3_settlement_line_update_after_terminal_settlement_guard
BEFORE UPDATE ON rental_settlement_lines
WHEN EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=NEW.tenant_id AND s.id=NEW.settlement_case_id AND s.status='settled'
)
OR EXISTS (
    SELECT 1 FROM rental_settlement_cases s
    WHERE s.tenant_id=OLD.tenant_id AND s.id=OLD.settlement_case_id AND s.status='settled'
)
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot add or change a settlement line');
END;

CREATE TRIGGER r3_settlement_line_delete_after_terminal_settlement_guard
BEFORE DELETE ON rental_settlement_lines
WHEN EXISTS (SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.id=OLD.settlement_case_id AND s.status='settled')
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot delete a settlement line');
END;

CREATE TRIGGER r3_effect_admission_delete_after_terminal_settlement_guard
BEFORE DELETE ON rental_settlement_effect_admissions
WHEN EXISTS (SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.id=OLD.settlement_case_id AND s.status='settled')
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot delete an effect admission');
END;

CREATE TRIGGER r3_overdue_active_after_terminal_settlement_guard
BEFORE INSERT ON overdue_records
WHEN NEW.status IN ('active','escalated_d1','escalated_d3','escalated_d7')
 AND (
    EXISTS (SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=NEW.tenant_id AND s.order_id=NEW.order_id AND s.status='settled')
    OR EXISTS (SELECT 1 FROM order_lifecycle l WHERE l.tenant_id=NEW.tenant_id AND l.order_id=NEW.order_id AND l.commercial_status='closed')
 )
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement or closed order cannot gain active overdue');
END;

CREATE TRIGGER r3_overdue_active_update_after_terminal_settlement_guard
BEFORE UPDATE OF status, tenant_id, order_id ON overdue_records
WHEN NEW.status IN ('active','escalated_d1','escalated_d3','escalated_d7')
 AND (
    EXISTS (SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=NEW.tenant_id AND s.order_id=NEW.order_id AND s.status='settled')
    OR EXISTS (SELECT 1 FROM order_lifecycle l WHERE l.tenant_id=NEW.tenant_id AND l.order_id=NEW.order_id AND l.commercial_status='closed')
    OR EXISTS (SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.order_id=OLD.order_id AND s.status='settled')
    OR EXISTS (SELECT 1 FROM order_lifecycle l WHERE l.tenant_id=OLD.tenant_id AND l.order_id=OLD.order_id AND l.commercial_status='closed')
 )
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement or closed order cannot activate overdue');
END;

CREATE TRIGGER r3_overdue_active_delete_after_terminal_settlement_guard
BEFORE DELETE ON overdue_records
WHEN OLD.status IN ('active','escalated_d1','escalated_d3','escalated_d7')
 AND EXISTS (SELECT 1 FROM rental_settlement_cases s WHERE s.tenant_id=OLD.tenant_id AND s.order_id=OLD.order_id AND s.status='settled')
BEGIN
    SELECT RAISE(ABORT, 'terminal settlement cannot delete active overdue');
END;
