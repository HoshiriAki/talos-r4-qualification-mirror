//! R3 tenant-scoped damage, repair, settlement, and dispute authority.
//!
//! This repository deliberately owns business facts only.  It uses the
//! existing Integration Fabric tables for the immutable deposit ledger and
//! ExternalOperation linkage, and never performs HTTP/provider work.

use chrono::{SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::application::rental_time::TenantBusinessTimeZone;
use crate::integration::operation::{FinancialEffectState, OperationState};
use crate::integration::settlement_admission::{
    SettlementChargeAdmission, admit_settlement_charge_tx,
};
use crate::observability::{IntegrationEvent, MetricsSink, OperationStateClass};
use crate::repositories::lifecycle::apply_action_in_transaction;
use crate::repositories::sqlite::SqliteRepositorySession;
use crate::repositories::workflow::append_outbox_tx;
use crate::repositories::{RepositoryError, ScopedRepositories};
use crate::utils::constants::txt;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InspectionCompletionInput {
    pub inspection_id: String,
    pub expected_version: i64,
    pub condition_code: String,
    #[serde(default)]
    pub missing: bool,
    #[serde(default)]
    pub normal_wear: bool,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionCompletionProjection {
    pub inspection_id: String,
    pub order_id: String,
    pub status: String,
    pub condition_code: String,
    pub business_date: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DamageFindingInput {
    pub inspection_id: String,
    pub finding_code: String,
    pub condition_code: String,
    pub description: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DamageFindingProjection {
    pub id: String,
    pub order_id: String,
    pub inspection_id: String,
    pub device_serial_no: String,
    pub condition_code: String,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LiabilityDecisionInput {
    pub finding_id: String,
    pub decision: String,
    pub apportioned_amount_minor: i64,
    pub currency: String,
    pub rationale: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiabilityDecisionProjection {
    pub id: String,
    pub finding_id: String,
    pub decision: String,
    pub apportioned_amount_minor: i64,
    pub currency: String,
    pub status: String,
    pub version: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepairDecisionInput {
    pub finding_id: String,
    pub decision: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepairTransitionInput {
    pub repair_case_id: String,
    pub target_status: String,
    pub expected_version: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairCaseProjection {
    pub id: String,
    pub finding_id: String,
    pub device_serial_no: String,
    pub decision: String,
    pub status: String,
    pub version: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettlementLineInput {
    pub finding_id: String,
    pub line_kind: String,
    pub amount_minor: i64,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettlementProposalInput {
    pub order_id: String,
    pub currency: String,
    pub idempotency_key: String,
    #[serde(default)]
    pub lines: Vec<SettlementLineInput>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettlementCaseProjection {
    pub id: String,
    pub order_id: String,
    pub currency: String,
    pub total_amount_minor: i64,
    pub deposit_deducted_minor: i64,
    pub status: String,
    pub facts_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettlementIdInput {
    pub settlement_case_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DepositDeductionInput {
    pub settlement_case_id: String,
    pub deposit_id: String,
    pub amount_minor: i64,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdditionalChargeInput {
    pub settlement_case_id: String,
    pub amount_minor: i64,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettlementEffectAdmissionProjection {
    pub id: String,
    pub settlement_case_id: String,
    pub effect_kind: String,
    pub amount_minor: i64,
    pub currency: String,
    pub external_operation_id: Option<String>,
    /// Deposit deductions are business-final; charge state is read from the
    /// linked R2 ExternalOperation and is never written by R3.
    pub external_operation_state: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenDisputeInput {
    pub settlement_case_id: String,
    pub reason: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveDisputeInput {
    pub dispute_id: String,
    pub resolution_note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisputeProjection {
    pub id: String,
    pub settlement_case_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct R3ClosureFacts {
    pub return_received: bool,
    pub inspection_complete: bool,
    pub unresolved_damage_findings: i64,
    pub unresolved_liability_decisions: i64,
    pub unresolved_repair_cases: i64,
    pub active_disputes: i64,
    pub unresolved_effect_intents: i64,
    pub overdue_open: bool,
    pub settlement_terminal: bool,
    pub financial_balance_fully_accounted: bool,
}

impl R3ClosureFacts {
    pub fn close_ready(&self) -> bool {
        self.return_received
            && self.inspection_complete
            && self.unresolved_damage_findings == 0
            && self.unresolved_liability_decisions == 0
            && self.unresolved_repair_cases == 0
            && self.active_disputes == 0
            && self.unresolved_effect_intents == 0
            && !self.overdue_open
            && self.settlement_terminal
            && self.financial_balance_fully_accounted
    }
}

pub struct ScopedR3SettlementRepository<'a> {
    session: &'a SqliteRepositorySession,
    metrics: Arc<dyn MetricsSink>,
}

impl<'a> ScopedR3SettlementRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
            metrics: scoped.metrics(),
        }
    }

    pub fn configure_business_timezone(
        &self,
        time_zone_id: &str,
        actor: &str,
    ) -> Result<String, RepositoryError> {
        let tenant = self.tenant();
        let zone = TenantBusinessTimeZone::parse(time_zone_id).map_err(contract)?;
        let actor = required(actor, "actor")?.to_owned();
        let now = utc_now();
        self.session.write_immediate(|tx| {
            tx.execute(
                "INSERT INTO tenant_business_time_zones (tenant_id,time_zone_id,configured_by,created_at,updated_at) VALUES (?1,?2,?3,?4,?4) ON CONFLICT(tenant_id) DO UPDATE SET time_zone_id=excluded.time_zone_id, configured_by=excluded.configured_by, updated_at=excluded.updated_at",
                params![tenant, zone.id(), actor, now],
            ).map_err(sqlite)?;
            Ok(zone.id().to_owned())
        })
    }

    pub fn business_timezone(&self) -> Result<String, RepositoryError> {
        let tenant = self.tenant();
        self.session
            .read(|connection| {
                connection.query_row(
                    "SELECT time_zone_id FROM tenant_business_time_zones WHERE tenant_id=?1",
                    params![tenant],
                    |row| row.get(0),
                )
            })
            .map_err(|error| match error {
                RepositoryError::Sqlite(detail) if detail.contains("Query returned no rows") => {
                    contract("tenant business timezone is not configured")
                }
                other => other,
            })
    }

    pub fn complete_inspection(
        &self,
        input: InspectionCompletionInput,
        actor: &str,
    ) -> Result<InspectionCompletionProjection, RepositoryError> {
        validate_inspection_completion(&input)?;
        let tenant = self.tenant();
        let actor = required(actor, "actor")?.to_owned();
        let zone = self.zone()?;
        let now = Utc::now();
        let now_text = format_utc(now);
        let business_date = zone.business_date(now).to_string();
        let evidence = evidence_json(&input.evidence_refs)?;
        self.session.write_immediate(|tx| {
            let (status, order_id, version, recorded_condition, recorded_missing, recorded_wear, recorded_evidence, recorded_business_date): (String, String, i64, String, i64, i64, String, Option<String>) = tx.query_row(
                "SELECT i.status,r.order_id,i.version,i.condition_code,i.missing,i.normal_wear,i.evidence_refs_json,i.business_date FROM rental_inspections i JOIN rental_return_items ri ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id JOIN rental_returns r ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id WHERE i.tenant_id=?1 AND i.id=?2",
                params![tenant, input.inspection_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
            ).map_err(|_| contract("inspection missing or foreign to the active tenant"))?;
            let terminal = if input.condition_code == "damaged" || input.missing { "failed" } else { "passed" };
            if matches!(status.as_str(), "passed" | "failed") {
                if version == input.expected_version + 1
                    && status == terminal
                    && recorded_condition == input.condition_code
                    && recorded_missing == i64::from(input.missing)
                    && recorded_wear == i64::from(input.normal_wear)
                    && recorded_evidence == evidence
                {
                    return Ok(InspectionCompletionProjection {
                        inspection_id: input.inspection_id,
                        order_id,
                        status,
                        condition_code: input.condition_code,
                        business_date: recorded_business_date.unwrap_or(business_date),
                    });
                }
                return Err(contract("terminal inspection completion is idempotent only for identical facts and expected version"));
            }
            if version != input.expected_version {
                return Err(contract("inspection optimistic version conflict"));
            }
            if status != "in_progress" {
                return Err(contract("inspection must be in_progress before a final result is recorded"));
            }
            let changed = tx.execute(
                "UPDATE rental_inspections SET status=?1,condition_code=?2,missing=?3,normal_wear=?4,evidence_refs_json=?5,business_date=?6,inspected_by=?7,completed_at=?8,updated_at=?8,version=version+1 WHERE tenant_id=?9 AND id=?10 AND version=?11",
                params![terminal,input.condition_code,i64::from(input.missing),i64::from(input.normal_wear),evidence,business_date,actor,now_text,tenant,input.inspection_id,input.expected_version],
            ).map_err(sqlite)?;
            if changed != 1 { return Err(contract("inspection finalization lost its expected version")); }
            emit_inspection_complete_if_ready(tx, &tenant, &order_id, &now_text)?;
            Ok(InspectionCompletionProjection { inspection_id: input.inspection_id, order_id, status: terminal.into(), condition_code: input.condition_code, business_date })
        })
    }

    pub fn create_damage_finding(
        &self,
        input: DamageFindingInput,
        actor: &str,
    ) -> Result<DamageFindingProjection, RepositoryError> {
        validate_finding(&input)?;
        let tenant = self.tenant();
        let actor = required(actor, "actor")?.to_owned();
        let zone = self.zone()?;
        let now = Utc::now();
        let now_text = format_utc(now);
        let business_date = zone.business_date(now).to_string();
        let evidence = evidence_json(&input.evidence_refs)?;
        self.session.write_immediate(|tx| {
            let existing: Option<DamageFindingProjection> = tx.query_row(
                "SELECT id,order_id,inspection_id,device_serial_no,condition_code,status FROM rental_damage_findings WHERE tenant_id=?1 AND inspection_id=?2 AND command_idempotency_key=?3",
                params![tenant,input.inspection_id,input.idempotency_key], row_finding,
            ).optional().map_err(sqlite)?;
            if let Some(existing) = existing { return Ok(existing); }
            let (order_id, device_serial_no, inspection_status, condition): (String,String,String,String) = tx.query_row(
                "SELECT r.order_id,i.device_serial_no,i.status,i.condition_code FROM rental_inspections i JOIN rental_return_items ri ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id JOIN rental_returns r ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id WHERE i.tenant_id=?1 AND i.id=?2",
                params![tenant,input.inspection_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            ).map_err(|_| contract("inspection not found in the active tenant"))?;
            if !matches!(inspection_status.as_str(), "passed" | "failed") { return Err(contract("a damage finding requires terminal inspection evidence")); }
            if input.condition_code != "normal_wear" && condition != input.condition_code { return Err(contract("damage finding condition conflicts with the final inspection outcome")); }
            assert_order_settlement_unsealed(tx, &tenant, &order_id)?;
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO rental_damage_findings (id,tenant_id,inspection_id,order_id,device_serial_no,finding_code,condition_code,description,evidence_refs_json,status,command_idempotency_key,found_by,occurred_at,business_date,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'open',?10,?11,?12,?13,?12,?12)",
                params![id,tenant,input.inspection_id,order_id,device_serial_no,input.finding_code,input.condition_code,input.description,evidence,input.idempotency_key,actor,now_text,business_date],
            ).map_err(sqlite)?;
            Ok(DamageFindingProjection { id, order_id, inspection_id: input.inspection_id, device_serial_no, condition_code: input.condition_code, status: "open".into() })
        })
    }

    pub fn decide_liability(
        &self,
        input: LiabilityDecisionInput,
        actor: &str,
    ) -> Result<LiabilityDecisionProjection, RepositoryError> {
        validate_liability(&input)?;
        let tenant = self.tenant();
        let actor = required(actor, "actor")?.to_owned();
        let zone = self.zone()?;
        let now = Utc::now();
        let now_text = format_utc(now);
        let business_date = zone.business_date(now).to_string();
        let evidence = evidence_json(&input.evidence_refs)?;
        let currency = currency(&input.currency)?;
        self.session.write_immediate(|tx| {
            let finding: Option<(String, String)> = tx.query_row(
                "SELECT id,order_id FROM rental_damage_findings WHERE tenant_id=?1 AND id=?2 AND status='open'",
                params![tenant,input.finding_id], |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional().map_err(sqlite)?;
            let Some((_, order_id)) = finding else { return Err(contract("open damage finding not found in the active tenant")); };
            let existing: Option<LiabilityDecisionProjection> = tx.query_row(
                "SELECT id,finding_id,decision,apportioned_amount_minor,currency,status,version FROM rental_liability_decisions WHERE tenant_id=?1 AND finding_id=?2",
                params![tenant,input.finding_id], row_liability,
            ).optional().map_err(sqlite)?;
            if let Some(existing) = existing {
                if existing.status == "open" && existing.decision == "manual_review" {
                    assert_order_settlement_unsealed(tx, &tenant, &order_id)?;
                    let changed = tx.execute(
                        "UPDATE rental_liability_decisions SET decision=?1,apportioned_amount_minor=?2,currency=?3,rationale=?4,evidence_refs_json=?5,status=?6,version=version+1,command_idempotency_key=?7,decided_by=?8,decided_at=?9,business_date=?10,updated_at=?9 WHERE tenant_id=?11 AND id=?12 AND version=?13",
                        params![input.decision,input.apportioned_amount_minor,currency,input.rationale,evidence,if input.decision=="manual_review" {"open"} else {"decided"},input.idempotency_key,actor,now_text,business_date,tenant,existing.id,existing.version],
                    ).map_err(sqlite)?;
                    if changed != 1 { return Err(contract("liability decision optimistic version conflict")); }
                    return load_liability(tx, &tenant, &existing.id);
                }
                if existing.decision == input.decision && existing.apportioned_amount_minor == input.apportioned_amount_minor && existing.currency == currency && existing.status == if input.decision == "manual_review" { "open" } else { "decided" } { return Ok(existing); }
                return Err(contract("a final liability decision is immutable; reopen only through an approved migration"));
            }
            assert_order_settlement_unsealed(tx, &tenant, &order_id)?;
            let id = Uuid::new_v4().to_string();
            let status = if input.decision == "manual_review" { "open" } else { "decided" };
            tx.execute(
                "INSERT INTO rental_liability_decisions (id,tenant_id,finding_id,decision,apportioned_amount_minor,currency,rationale,evidence_refs_json,status,version,command_idempotency_key,decided_by,decided_at,business_date,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,1,?10,?11,?12,?13,?12,?12)",
                params![id,tenant,input.finding_id,input.decision,input.apportioned_amount_minor,currency,input.rationale,evidence,status,input.idempotency_key,actor,now_text,business_date],
            ).map_err(sqlite)?;
            load_liability(tx, &tenant, &id)
        })
    }

    pub fn decide_repair(
        &self,
        input: RepairDecisionInput,
        actor: &str,
    ) -> Result<RepairCaseProjection, RepositoryError> {
        validate_repair_decision(&input.decision)?;
        let tenant = self.tenant();
        let actor = required(actor, "actor")?.to_owned();
        let zone = self.zone()?;
        let now = Utc::now();
        let now_text = format_utc(now);
        let business_date = zone.business_date(now).to_string();
        self.session.write_immediate(|tx| {
            let existing: Option<RepairCaseProjection> = tx.query_row(
                "SELECT id,finding_id,device_serial_no,decision,status,version FROM rental_repair_cases WHERE tenant_id=?1 AND finding_id=?2",
                params![tenant,input.finding_id], row_repair,
            ).optional().map_err(sqlite)?;
            if let Some(existing) = existing {
                if existing.decision == input.decision { return Ok(existing); }
                return Err(contract("repair decision is already authoritative for this finding"));
            }
            let (order_id, serial_no, liability_status): (String,String,String) = tx.query_row(
                "SELECT f.order_id,f.device_serial_no,l.status FROM rental_damage_findings f JOIN rental_liability_decisions l ON l.tenant_id=f.tenant_id AND l.finding_id=f.id WHERE f.tenant_id=?1 AND f.id=?2 AND f.status='open'",
                params![tenant,input.finding_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
            ).map_err(|_| contract("an authoritative liability decision is required before repair decision"))?;
            if liability_status != "decided" { return Err(contract("repair decision is blocked by unresolved liability")); }
            assert_order_settlement_unsealed(tx, &tenant, &order_id)?;
            assert_device_for_tenant(tx, &tenant, &serial_no)?;
            let status = input.decision.as_str();
            apply_device_inventory_state(tx, &tenant, &serial_no, status)?;
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO rental_repair_cases (id,tenant_id,finding_id,order_id,device_serial_no,decision,status,version,command_idempotency_key,decided_by,decided_at,completed_at,business_date,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,?9,?10,CASE WHEN ?7 IN ('retired','lost','no_action') THEN ?10 ELSE NULL END,?11,?10,?10)",
                params![id,tenant,input.finding_id,order_id,serial_no,input.decision,status,input.idempotency_key,actor,now_text,business_date],
            ).map_err(sqlite)?;
            load_repair(tx, &tenant, &id)
        })
    }

    pub fn transition_repair(
        &self,
        input: RepairTransitionInput,
    ) -> Result<RepairCaseProjection, RepositoryError> {
        validate_repair_status(&input.target_status)?;
        if input.expected_version < 1 {
            return Err(contract("expectedVersion must be positive"));
        }
        let tenant = self.tenant();
        let now = utc_now();
        self.session.write_immediate(|tx| {
            let current = load_repair(tx, &tenant, &input.repair_case_id)?;
            if current.version != input.expected_version { return Err(contract("repair case optimistic version conflict")); }
            let order_id: String = tx.query_row(
                "SELECT order_id FROM rental_repair_cases WHERE tenant_id=?1 AND id=?2",
                params![tenant, input.repair_case_id],
                |row| row.get(0),
            ).map_err(|_| contract("repair case not found in the active tenant"))?;
            assert_order_settlement_unsealed(tx, &tenant, &order_id)?;
            let legal = matches!((current.status.as_str(), input.target_status.as_str()),
                ("repair_required", "under_repair") | ("repair_required", "retired") | ("repair_required", "lost") |
                ("under_repair", "repaired") | ("under_repair", "retired") | ("under_repair", "lost"));
            if !legal { return Err(contract("illegal repair/inventory transition")); }
            apply_device_inventory_state(tx, &tenant, &current.device_serial_no, &input.target_status)?;
            let changed = tx.execute(
                "UPDATE rental_repair_cases SET status=?1,version=version+1,completed_at=CASE WHEN ?1 IN ('repaired','retired','lost','no_action') THEN ?2 ELSE completed_at END,updated_at=?2 WHERE tenant_id=?3 AND id=?4 AND version=?5",
                params![input.target_status,now,tenant,input.repair_case_id,input.expected_version],
            ).map_err(sqlite)?;
            if changed != 1 { return Err(contract("repair transition lost its expected version")); }
            load_repair(tx, &tenant, &input.repair_case_id)
        })
    }

    pub fn propose_settlement(
        &self,
        input: SettlementProposalInput,
        actor: &str,
    ) -> Result<SettlementCaseProjection, RepositoryError> {
        let tenant = self.tenant();
        let order_id = required(&input.order_id, "orderId")?.to_owned();
        let idempotency_key = required(&input.idempotency_key, "idempotencyKey")?.to_owned();
        let currency = currency(&input.currency)?;
        let actor = required(actor, "actor")?.to_owned();
        validate_lines(&input.lines)?;
        let zone = self.zone()?;
        let now = Utc::now();
        let now_text = format_utc(now);
        let business_date = zone.business_date(now).to_string();
        self.session.write_immediate(|tx| {
            let existing: Option<SettlementCaseProjection> = tx.query_row(
                "SELECT id,order_id,currency,total_amount_minor,deposit_deducted_minor,status,facts_hash FROM rental_settlement_cases WHERE tenant_id=?1 AND command_idempotency_key=?2",
                params![tenant,idempotency_key], row_settlement,
            ).optional().map_err(sqlite)?;
            if let Some(existing) = existing { return Ok(existing); }
            let existing_order: Option<SettlementCaseProjection> = tx.query_row(
                "SELECT id,order_id,currency,total_amount_minor,deposit_deducted_minor,status,facts_hash FROM rental_settlement_cases WHERE tenant_id=?1 AND order_id=?2",
                params![tenant,order_id], row_settlement,
            ).optional().map_err(sqlite)?;
            if existing_order.is_some() { return Err(contract("an authoritative settlement case already exists for this order")); }
            let order_exists: Option<String> = tx.query_row(
                "SELECT id FROM orders WHERE tenant_id=?1 AND id=?2",
                params![tenant, order_id], |row| row.get(0),
            ).optional().map_err(sqlite)?;
            if order_exists.is_none() { return Err(contract("settlement proposal requires an existing scoped order")); }
            let findings = load_settlement_findings(tx, &tenant, &order_id)?;
            validate_settlement_lines(&findings, &input.lines, &currency)?;
            let total: i64 = input.lines.iter().map(|line| line.amount_minor).sum();
            let facts_hash = settlement_hash(&order_id, &currency, &input.lines, &findings);
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO rental_settlement_cases (id,tenant_id,order_id,currency,total_amount_minor,deposit_deducted_minor,status,facts_hash,command_idempotency_key,proposed_by,proposed_at,accounting_business_date,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,0,'proposed',?6,?7,?8,?9,?10,?9,?9)",
                params![id,tenant,order_id,currency,total,facts_hash,idempotency_key,actor,now_text,business_date],
            ).map_err(sqlite)?;
            for line in &input.lines {
                let decision = findings.iter().find(|item| item.finding_id == line.finding_id).expect("validated line finding");
                tx.execute(
                    "INSERT INTO rental_settlement_lines (id,tenant_id,settlement_case_id,finding_id,liability_decision_id,line_kind,amount_minor,currency,description,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    params![Uuid::new_v4().to_string(),tenant,id,line.finding_id,decision.decision_id,line.line_kind,line.amount_minor,currency,line.description,now_text],
                ).map_err(sqlite)?;
            }
            tx.execute(
                "INSERT INTO rental_settlements (id,tenant_id,order_id,currency,amount_minor,facts_hash,status,blocker_code,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,'calculated',NULL,?7,?7) ON CONFLICT(tenant_id,order_id) DO UPDATE SET currency=excluded.currency,amount_minor=excluded.amount_minor,facts_hash=excluded.facts_hash,status='calculated',blocker_code=NULL,updated_at=excluded.updated_at",
                params![Uuid::new_v4().to_string(),tenant,order_id,currency,total,facts_hash,now_text],
            ).map_err(sqlite)?;
            load_settlement(tx, &tenant, &id)
        })
    }

    pub fn accept_settlement(
        &self,
        settlement_case_id: &str,
    ) -> Result<SettlementCaseProjection, RepositoryError> {
        let tenant = self.tenant();
        let id = required(settlement_case_id, "settlementCaseId")?.to_owned();
        let now = utc_now();
        self.session.write_immediate(|tx| {
            let existing = load_settlement(tx, &tenant, &id)?;
            if existing.status == "accepted" { return Ok(existing); }
            if !matches!(existing.status.as_str(), "proposed" | "resolved") { return Err(contract("settlement may be accepted only from proposed or resolved")); }
            if active_dispute_count(tx, &tenant, &id)? != 0 { return Err(contract("an active dispute blocks settlement acceptance")); }
            tx.execute("UPDATE rental_settlement_cases SET status='accepted',accepted_at=?1,updated_at=?1 WHERE tenant_id=?2 AND id=?3", params![now,tenant,id]).map_err(sqlite)?;
            load_settlement(tx, &tenant, &id)
        })
    }

    pub fn deduct_deposit(
        &self,
        input: DepositDeductionInput,
        actor: &str,
    ) -> Result<SettlementEffectAdmissionProjection, RepositoryError> {
        if input.amount_minor <= 0 {
            return Err(contract("deposit deduction amount must be positive"));
        }
        let tenant = self.tenant();
        let actor = required(actor, "actor")?.to_owned();
        let idempotency = required(&input.idempotency_key, "idempotencyKey")?.to_owned();
        let now = utc_now();
        self.session.write_immediate(|tx| {
            let case = load_settlement(tx, &tenant, &input.settlement_case_id)?;
            assert_order_settlement_unsealed(tx, &tenant, &case.order_id)?;
            if !matches!(case.status.as_str(), "accepted" | "resolved") { return Err(contract("deposit deduction requires an accepted settlement")); }
            let existing: Option<SettlementEffectAdmissionProjection> = tx.query_row(
                "SELECT e.id,e.settlement_case_id,e.effect_kind,e.amount_minor,e.currency,e.external_operation_id,NULL FROM rental_settlement_effect_admissions e WHERE e.tenant_id=?1 AND e.settlement_case_id=?2 AND e.idempotency_key=?3",
                params![tenant,input.settlement_case_id,idempotency], row_effect,
            ).optional().map_err(sqlite)?;
            if let Some(existing) = existing {
                if existing.effect_kind == "deposit_deduction" && existing.amount_minor == input.amount_minor { return Ok(existing); }
                return Err(contract("effect idempotency key was reused with a different financial intent"));
            }
            assert_financial_reservation(tx, &tenant, &case, input.amount_minor, 0)?;
            let (authority_kind, authority_id, expected_currency, state): (String,String,String,String) = tx.query_row(
                "SELECT authority_kind,authority_id,currency,state FROM integration_deposits WHERE tenant_id=?1 AND id=?2",
                params![tenant,input.deposit_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            ).map_err(|_| contract("deposit not found in the active tenant"))?;
            if authority_kind != "order" || authority_id != case.order_id { return Err(contract("deposit is not authorized for this tenant/order settlement")); }
            if expected_currency != case.currency { return Err(contract("deposit currency differs from settlement currency")); }
            if !matches!(state.as_str(), "recorded" | "held" | "partially_deducted") { return Err(contract("deposit is in an incompatible terminal state")); }
            let available = deposit_available(tx, &tenant, &input.deposit_id)?;
            if input.amount_minor > available { return Err(contract("deposit deduction exceeds available authorized deposit")); }
            let effect_id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO rental_settlement_effect_admissions (id,tenant_id,settlement_case_id,effect_kind,amount_minor,currency,deposit_id,external_operation_id,idempotency_key,business_authorization_ref,created_by,created_at) VALUES (?1,?2,?3,'deposit_deduction',?4,?5,?6,NULL,?7,?8,?9,?10)",
                params![effect_id,tenant,case.id,input.amount_minor,case.currency,input.deposit_id,idempotency,format!("settlement-case:{}",case.id),actor,now],
            ).map_err(sqlite)?;
            tx.execute(
                "INSERT INTO integration_deposit_ledger (id,tenant_id,deposit_id,entry_type,amount_minor,currency,external_operation_id,audit_ref,created_at) VALUES (?1,?2,?3,'deducted',?4,?5,NULL,?6,?7)",
                params![Uuid::new_v4().to_string(),tenant,input.deposit_id,input.amount_minor,case.currency,format!("r3-settlement-effect:{effect_id}"),now],
            ).map_err(sqlite)?;
            tx.execute("UPDATE integration_deposits SET state='partially_deducted',updated_at=?1 WHERE tenant_id=?2 AND id=?3", params![now,tenant,input.deposit_id]).map_err(sqlite)?;
            tx.execute("UPDATE rental_settlement_cases SET deposit_deducted_minor=deposit_deducted_minor+?1,updated_at=?2 WHERE tenant_id=?3 AND id=?4", params![input.amount_minor,now,tenant,case.id]).map_err(sqlite)?;
            load_effect(tx, &tenant, &effect_id)
        })
    }

    pub fn admit_additional_charge(
        &self,
        input: AdditionalChargeInput,
        actor: &str,
    ) -> Result<SettlementEffectAdmissionProjection, RepositoryError> {
        if input.amount_minor <= 0 {
            return Err(contract("additional charge amount must be positive"));
        }
        let tenant = self.tenant();
        let actor = required(actor, "actor")?.to_owned();
        let idempotency = required(&input.idempotency_key, "idempotencyKey")?.to_owned();
        let now = utc_now();
        let (effect, created_new) = self.session.write_immediate(|tx| {
            let case = load_settlement(tx, &tenant, &input.settlement_case_id)?;
            assert_order_settlement_unsealed(tx, &tenant, &case.order_id)?;
            if !matches!(case.status.as_str(), "accepted" | "resolved") { return Err(contract("additional charge intent requires an accepted settlement")); }
            let existing: Option<SettlementEffectAdmissionProjection> = tx.query_row(
                "SELECT e.id,e.settlement_case_id,e.effect_kind,e.amount_minor,e.currency,e.external_operation_id,o.state FROM rental_settlement_effect_admissions e LEFT JOIN external_operations o ON o.tenant_id=e.tenant_id AND o.id=e.external_operation_id WHERE e.tenant_id=?1 AND e.settlement_case_id=?2 AND e.idempotency_key=?3",
                params![tenant,case.id,idempotency], row_effect,
            ).optional().map_err(sqlite)?;
            if let Some(existing) = existing {
                if existing.effect_kind == "additional_charge" && existing.amount_minor == input.amount_minor { return Ok((existing, false)); }
                return Err(contract("effect idempotency key was reused with a different financial intent"));
            }
            assert_financial_reservation(tx, &tenant, &case, 0, input.amount_minor)?;
            let authorization_ref = format!("settlement-case:{}", case.id);
            let admitted = admit_settlement_charge_tx(tx, &SettlementChargeAdmission {
                tenant_id: &tenant,
                settlement_case_id: &case.id,
                order_id: &case.order_id,
                amount_minor: input.amount_minor,
                currency: &case.currency,
                business_authorization_ref: &authorization_ref,
                idempotency_key: &idempotency,
            }).map_err(contract)?;
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO rental_settlement_effect_admissions (id,tenant_id,settlement_case_id,effect_kind,amount_minor,currency,deposit_id,external_operation_id,idempotency_key,business_authorization_ref,created_by,created_at) VALUES (?1,?2,?3,'additional_charge',?4,?5,NULL,?6,?7,?8,?9,?10)",
                params![id,tenant,case.id,input.amount_minor,case.currency,admitted.external_operation_id,idempotency,authorization_ref,actor,now],
            ).map_err(sqlite)?;
            Ok((load_effect(tx, &tenant, &id)?, admitted.created_new))
        })?;
        // The canonical operation and settlement admission link have now
        // committed atomically. An idempotent reuse or any rolled-back write
        // must never increment the R2 admission metric.
        if created_new {
            self.metrics
                .integration_operation(IntegrationEvent::Admitted, OperationStateClass::Ready);
        }
        Ok(effect)
    }

    pub fn open_dispute(
        &self,
        input: OpenDisputeInput,
        actor: &str,
    ) -> Result<DisputeProjection, RepositoryError> {
        let tenant = self.tenant();
        let actor = required(actor, "actor")?.to_owned();
        let reason = required(&input.reason, "reason")?.to_owned();
        let idempotency = required(&input.idempotency_key, "idempotencyKey")?.to_owned();
        let evidence = evidence_json(&input.evidence_refs)?;
        let now = utc_now();
        self.session.write_immediate(|tx| {
            let case = load_settlement(tx, &tenant, &input.settlement_case_id)?;
            assert_order_settlement_unsealed(tx, &tenant, &case.order_id)?;
            if !matches!(case.status.as_str(), "proposed" | "accepted" | "resolved") { return Err(contract("settled cases cannot be disputed")); }
            let existing: Option<DisputeProjection> = tx.query_row(
                "SELECT id,settlement_case_id,status FROM rental_disputes WHERE tenant_id=?1 AND settlement_case_id=?2 AND command_idempotency_key=?3",
                params![tenant,case.id,idempotency], row_dispute,
            ).optional().map_err(sqlite)?;
            if let Some(existing) = existing { return Ok(existing); }
            let id = Uuid::new_v4().to_string();
            tx.execute("INSERT INTO rental_disputes (id,tenant_id,settlement_case_id,status,reason,evidence_refs_json,command_idempotency_key,opened_by,opened_at,created_at,updated_at) VALUES (?1,?2,?3,'open',?4,?5,?6,?7,?8,?8,?8)", params![id,tenant,case.id,reason,evidence,idempotency,actor,now]).map_err(sqlite)?;
            tx.execute("UPDATE rental_settlement_cases SET status='disputed',updated_at=?1 WHERE tenant_id=?2 AND id=?3", params![now,tenant,case.id]).map_err(sqlite)?;
            Ok(DisputeProjection { id, settlement_case_id: case.id, status: "open".into() })
        })
    }

    pub fn resolve_dispute(
        &self,
        input: ResolveDisputeInput,
        actor: &str,
    ) -> Result<DisputeProjection, RepositoryError> {
        let tenant = self.tenant();
        let actor = required(actor, "actor")?.to_owned();
        let note = required(&input.resolution_note, "resolutionNote")?.to_owned();
        let now = utc_now();
        self.session.write_immediate(|tx| {
            let dispute = load_dispute(tx, &tenant, &input.dispute_id)?;
            let order_id: String = tx.query_row(
                "SELECT s.order_id FROM rental_settlement_cases s JOIN rental_disputes d ON d.tenant_id=s.tenant_id AND d.settlement_case_id=s.id WHERE d.tenant_id=?1 AND d.id=?2",
                params![tenant, dispute.id],
                |row| row.get(0),
            ).map_err(|_| contract("settlement case not found in the active tenant"))?;
            assert_order_settlement_unsealed(tx, &tenant, &order_id)?;
            if !matches!(dispute.status.as_str(), "open" | "under_review") { return Err(contract("only an active dispute may be manually resolved")); }
            tx.execute("UPDATE rental_disputes SET status='resolved',resolved_by=?1,resolved_at=?2,resolution_note=?3,updated_at=?2 WHERE tenant_id=?4 AND id=?5", params![actor,now,note,tenant,dispute.id]).map_err(sqlite)?;
            if active_dispute_count(tx, &tenant, &dispute.settlement_case_id)? == 0 { tx.execute("UPDATE rental_settlement_cases SET status='resolved',updated_at=?1 WHERE tenant_id=?2 AND id=?3 AND status='disputed'", params![now,tenant,dispute.settlement_case_id]).map_err(sqlite)?; }
            Ok(DisputeProjection { id: dispute.id, settlement_case_id: dispute.settlement_case_id, status: "resolved".into() })
        })
    }

    pub fn complete_settlement(
        &self,
        settlement_case_id: &str,
    ) -> Result<SettlementCaseProjection, RepositoryError> {
        let tenant = self.tenant();
        let id = required(settlement_case_id, "settlementCaseId")?.to_owned();
        let now = utc_now();
        self.session.write_immediate(|tx| {
            let case = load_settlement(tx, &tenant, &id)?;
            if case.status == "settled" { return Ok(case); }
            if !matches!(case.status.as_str(), "accepted" | "resolved") { return Err(contract("settlement must be accepted and dispute-clear before terminal settlement")); }
            let facts = closure_facts_sql(tx, &tenant, &case.order_id).map_err(sqlite)?;
            if !facts.return_received || !facts.inspection_complete || facts.unresolved_damage_findings != 0 || facts.unresolved_liability_decisions != 0 || facts.unresolved_repair_cases != 0 || facts.active_disputes != 0 || facts.unresolved_effect_intents != 0 || facts.overdue_open || !facts.financial_balance_fully_accounted {
                return Err(contract("settlement requires received return, completed inspection, zero blockers, resolved R2 external operations, no overdue record, and exact accounting"));
            }
            tx.execute("UPDATE rental_settlement_cases SET status='settled',settled_at=?1,updated_at=?1 WHERE tenant_id=?2 AND id=?3", params![now,tenant,case.id]).map_err(sqlite)?;
            tx.execute("UPDATE rental_settlements SET status='terminal',blocker_code=NULL,updated_at=?1 WHERE tenant_id=?2 AND order_id=?3", params![now,tenant,case.order_id]).map_err(sqlite)?;
            append_outbox_tx(tx,&tenant,"order",&case.order_id,"SettlementComplete",&format!("r3-settlement-complete:{}",case.id),&serde_json::json!({"orderId":case.order_id,"settlementCaseId":case.id}),&now)?;
            load_settlement(tx, &tenant, &case.id)
        })
    }

    pub fn closure_facts(&self, order_id: &str) -> Result<R3ClosureFacts, RepositoryError> {
        let tenant = self.tenant();
        let order_id = required(order_id, "orderId")?.to_owned();
        self.session
            .read(|connection| closure_facts_sql(connection, &tenant, &order_id))
    }

    /// Close through the named lifecycle writer while the R3 terminal facts
    /// and lifecycle optimistic version are protected by one immediate
    /// transaction.  This deliberately has no read-then-write closure gap.
    pub fn close_order_atomic(
        &self,
        order_id: &str,
        expected_version: i64,
        actor: &str,
    ) -> Result<(), RepositoryError> {
        let tenant = self.tenant();
        let order_id = required(order_id, "orderId")?.to_owned();
        let actor = required(actor, "actor")?.to_owned();
        self.session.write_immediate(|tx| {
            let facts = closure_facts_sql(tx, &tenant, &order_id).map_err(sqlite)?;
            if !facts.close_ready() {
                return Err(contract(format!(
                    "R3 terminal closure blocked: return_received={}; inspection_complete={}; unresolved_damage_findings={}; unresolved_liability_decisions={}; unresolved_repair_cases={}; active_disputes={}; unresolved_effect_intents={}; overdue_open={}; settlement_terminal={}; financial_balance_fully_accounted={}",
                    facts.return_received,
                    facts.inspection_complete,
                    facts.unresolved_damage_findings,
                    facts.unresolved_liability_decisions,
                    facts.unresolved_repair_cases,
                    facts.active_disputes,
                    facts.unresolved_effect_intents,
                    facts.overdue_open,
                    facts.settlement_terminal,
                    facts.financial_balance_fully_accounted,
                )));
            }
            apply_action_in_transaction(
                tx,
                &tenant,
                &order_id,
                "close_order",
                expected_version,
                &actor,
                "R3 terminal settlement and closure invariant verified in the same transaction",
            )?;
            Ok(())
        })
    }

    fn tenant(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }

    fn zone(&self) -> Result<TenantBusinessTimeZone, RepositoryError> {
        TenantBusinessTimeZone::parse(&self.business_timezone()?).map_err(contract)
    }
}

#[derive(Debug)]
pub(in crate::repositories) struct FindingDecisionRow {
    pub(in crate::repositories) finding_id: String,
    pub(in crate::repositories) decision_id: String,
    pub(in crate::repositories) decision: String,
    pub(in crate::repositories) amount_minor: i64,
    pub(in crate::repositories) currency: String,
    pub(in crate::repositories) status: String,
}

fn load_settlement_findings(
    c: &rusqlite::Connection,
    tenant: &str,
    order_id: &str,
) -> Result<Vec<FindingDecisionRow>, RepositoryError> {
    let mut stmt = c.prepare("SELECT f.id,l.id,l.decision,l.apportioned_amount_minor,l.currency,l.status FROM rental_damage_findings f LEFT JOIN rental_liability_decisions l ON l.tenant_id=f.tenant_id AND l.finding_id=f.id WHERE f.tenant_id=?1 AND f.order_id=?2 AND f.status='open' ORDER BY f.id").map_err(sqlite)?;
    let rows = stmt
        .query_map(params![tenant, order_id], |row| {
            Ok(FindingDecisionRow {
                finding_id: row.get(0)?,
                decision_id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                decision: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                amount_minor: row.get::<_, Option<i64>>(3)?.unwrap_or_default(),
                currency: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                status: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        })
        .map_err(sqlite)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sqlite)
}

pub(in crate::repositories) fn validate_settlement_lines(
    findings: &[FindingDecisionRow],
    lines: &[SettlementLineInput],
    currency: &str,
) -> Result<(), RepositoryError> {
    for finding in findings {
        if finding.status != "decided" {
            return Err(contract(
                "all damage findings require an explicit non-manual liability decision before settlement",
            ));
        }
    }
    for line in lines {
        let finding = findings
            .iter()
            .find(|finding| finding.finding_id == line.finding_id)
            .ok_or_else(|| contract("settlement line finding is not part of this tenant/order"))?;
        if !matches!(finding.decision.as_str(), "liable" | "partial") {
            return Err(contract(
                "only liable or apportioned findings may create a financial settlement line",
            ));
        }
        if finding.currency != currency {
            return Err(contract("liability and settlement currencies differ"));
        }
        if line.amount_minor != finding.amount_minor {
            return Err(contract(
                "a liable or partial finding must be settled for its exact authorized liability amount; use an explicit immutable adjustment authority for any exception",
            ));
        }
    }
    if lines.len()
        != lines
            .iter()
            .map(|line| line.finding_id.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len()
    {
        return Err(contract("a finding may appear in only one settlement line"));
    }
    for finding in findings
        .iter()
        .filter(|finding| matches!(finding.decision.as_str(), "liable" | "partial"))
    {
        if !lines
            .iter()
            .any(|line| line.finding_id == finding.finding_id)
        {
            return Err(contract(
                "every liable or partial finding requires one exact settlement line",
            ));
        }
    }
    Ok(())
}

fn closure_facts_sql(
    c: &rusqlite::Connection,
    tenant: &str,
    order_id: &str,
) -> rusqlite::Result<R3ClosureFacts> {
    let return_received: i64 = c.query_row("SELECT COUNT(*) FROM rental_returns WHERE tenant_id=?1 AND order_id=?2 AND status='received'",params![tenant,order_id],|row|row.get(0))?;
    let counts: (i64,i64) = c.query_row("SELECT COUNT(*),COALESCE(SUM(CASE WHEN i.status IN ('passed','failed') THEN 1 ELSE 0 END),0) FROM rental_inspections i JOIN rental_return_items ri ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id JOIN rental_returns r ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id WHERE i.tenant_id=?1 AND r.order_id=?2",params![tenant,order_id],|row|Ok((row.get(0)?,row.get(1)?)))?;
    let unresolved_damage_findings: i64 = c.query_row("SELECT COUNT(*) FROM rental_damage_findings WHERE tenant_id=?1 AND order_id=?2 AND status='open' AND id NOT IN (SELECT finding_id FROM rental_liability_decisions WHERE tenant_id=?1 AND status='decided')",params![tenant,order_id],|row|row.get(0))?;
    let unresolved_liability_decisions: i64 = c.query_row("SELECT COUNT(*) FROM rental_liability_decisions l JOIN rental_damage_findings f ON f.tenant_id=l.tenant_id AND f.id=l.finding_id WHERE l.tenant_id=?1 AND f.order_id=?2 AND l.status<>'decided'",params![tenant,order_id],|row|row.get(0))?;
    let unresolved_repair_cases: i64 = c.query_row("SELECT COUNT(*) FROM rental_damage_findings f LEFT JOIN rental_repair_cases r ON r.tenant_id=f.tenant_id AND r.finding_id=f.id WHERE f.tenant_id=?1 AND f.order_id=?2 AND f.condition_code IN ('damaged','missing') AND f.status='open' AND (r.id IS NULL OR r.status IN ('repair_required','under_repair'))",params![tenant,order_id],|row|row.get(0))?;
    let active_disputes: i64 = c.query_row("SELECT COUNT(*) FROM rental_disputes d JOIN rental_settlement_cases s ON s.tenant_id=d.tenant_id AND s.id=d.settlement_case_id WHERE d.tenant_id=?1 AND s.order_id=?2 AND d.status IN ('open','under_review')",params![tenant,order_id],|row|row.get(0))?;
    let overdue_open: i64 = c.query_row("SELECT COUNT(*) FROM overdue_records WHERE tenant_id=?1 AND order_id=?2 AND status IN ('active','escalated_d1','escalated_d3','escalated_d7')",params![tenant,order_id],|row|row.get(0))?;
    let settlement_terminal: i64 = c.query_row("SELECT COUNT(*) FROM rental_settlement_cases WHERE tenant_id=?1 AND order_id=?2 AND status='settled'",params![tenant,order_id],|row|row.get(0))?;
    let financial_summary = load_financial_effect_summary(c, tenant, order_id)?;
    let settlement_total: Option<i64> = c.query_row(
        "SELECT total_amount_minor FROM rental_settlement_cases WHERE tenant_id=?1 AND order_id=?2",
        params![tenant, order_id],
        |row| row.get(0),
    ).optional()?;
    Ok(R3ClosureFacts {
        return_received: return_received == 1,
        inspection_complete: counts.0 > 0 && counts.0 == counts.1,
        unresolved_damage_findings,
        unresolved_liability_decisions,
        unresolved_repair_cases,
        active_disputes,
        unresolved_effect_intents: financial_summary.unresolved_effects,
        overdue_open: overdue_open > 0,
        settlement_terminal: settlement_terminal == 1,
        financial_balance_fully_accounted: settlement_total
            == Some(financial_summary.accounted_minor),
    })
}

fn active_dispute_count(
    c: &rusqlite::Connection,
    tenant: &str,
    case_id: &str,
) -> Result<i64, RepositoryError> {
    c.query_row("SELECT COUNT(*) FROM rental_disputes WHERE tenant_id=?1 AND settlement_case_id=?2 AND status IN ('open','under_review')",params![tenant,case_id],|row|row.get(0)).map_err(sqlite)
}
fn deposit_available(
    c: &rusqlite::Connection,
    tenant: &str,
    deposit_id: &str,
) -> Result<i64, RepositoryError> {
    c.query_row("SELECT COALESCE(SUM(CASE entry_type WHEN 'received' THEN amount_minor WHEN 'manual_adjustment' THEN amount_minor WHEN 'deducted' THEN -amount_minor WHEN 'refund_completed' THEN -amount_minor ELSE 0 END),0) FROM integration_deposit_ledger WHERE tenant_id=?1 AND deposit_id=?2",params![tenant,deposit_id],|row|row.get(0)).map_err(sqlite)
}

#[derive(Debug, Default)]
struct FinancialEffectSummary {
    accounted_minor: i64,
    reserved_minor: i64,
    unresolved_effects: i64,
}

/// R3 consumes this single R2-owned classification; it never writes outcome
/// state or interprets raw provider results itself.
fn load_financial_effect_summary(
    c: &rusqlite::Connection,
    tenant: &str,
    order_id: &str,
) -> rusqlite::Result<FinancialEffectSummary> {
    let mut statement = c.prepare(
        "SELECT e.effect_kind,e.amount_minor,o.state
         FROM rental_settlement_effect_admissions e
         JOIN rental_settlement_cases s ON s.tenant_id=e.tenant_id AND s.id=e.settlement_case_id
         LEFT JOIN external_operations o ON o.tenant_id=e.tenant_id AND o.id=e.external_operation_id
         WHERE e.tenant_id=?1 AND s.order_id=?2",
    )?;
    let rows = statement.query_map(params![tenant, order_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut summary = FinancialEffectSummary::default();
    for row in rows {
        let (effect_kind, amount_minor, operation_state) = row?;
        if effect_kind == "deposit_deduction" {
            summary.accounted_minor += amount_minor;
            summary.reserved_minor += amount_minor;
            continue;
        }
        match operation_state
            .as_deref()
            .and_then(OperationState::from_persisted)
            .map(|state| state.financial_effect_state())
            .unwrap_or(FinancialEffectState::Unresolved)
        {
            FinancialEffectState::Confirmed => {
                summary.accounted_minor += amount_minor;
                summary.reserved_minor += amount_minor;
            }
            FinancialEffectState::KnownNoEffect => {}
            FinancialEffectState::Unresolved => {
                summary.reserved_minor += amount_minor;
                summary.unresolved_effects += 1;
            }
        }
    }
    Ok(summary)
}

/// One reservation rule governs both deposit deduction and external charge
/// admission. Known-no-effect R2 charges release their reservation; confirmed
/// and unresolved R2 charges remain reserved.
fn assert_financial_reservation(
    tx: &rusqlite::Transaction<'_>,
    tenant: &str,
    case: &SettlementCaseProjection,
    proposed_deposit_minor: i64,
    proposed_charge_minor: i64,
) -> Result<(), RepositoryError> {
    let reserved = load_financial_effect_summary(tx, tenant, &case.order_id)
        .map_err(sqlite)?
        .reserved_minor;
    let proposed = proposed_deposit_minor
        .checked_add(proposed_charge_minor)
        .ok_or_else(|| contract("financial reservation amount overflow"))?;
    let total = reserved
        .checked_add(proposed)
        .ok_or_else(|| contract("financial reservation amount overflow"))?;
    if total > case.total_amount_minor {
        return Err(contract(
            "deposit deductions and R2-confirmed-or-unresolved admitted external charges exceed the authorized settlement total",
        ));
    }
    Ok(())
}

fn assert_order_settlement_unsealed(
    c: &rusqlite::Connection,
    tenant: &str,
    order_id: &str,
) -> Result<(), RepositoryError> {
    let terminal: i64 = c.query_row(
        "SELECT COUNT(*) FROM rental_settlement_cases WHERE tenant_id=?1 AND order_id=?2 AND status='settled'",
        params![tenant, order_id],
        |row| row.get(0),
    ).map_err(sqlite)?;
    if terminal == 0 {
        Ok(())
    } else {
        Err(contract(
            "terminal R3 settlement is sealed; amend or reopen requires an approved future authority",
        ))
    }
}

fn assert_device_for_tenant(
    c: &rusqlite::Connection,
    tenant: &str,
    serial: &str,
) -> Result<(), RepositoryError> {
    let present: Option<String> = c
        .query_row(
            "SELECT serialNo FROM devices WHERE tenant_id=?1 AND serialNo=?2",
            params![tenant, serial],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite)?;
    if present.is_some() {
        Ok(())
    } else {
        Err(contract(
            "device inventory record does not belong to the active tenant",
        ))
    }
}
fn apply_device_inventory_state(
    c: &rusqlite::Connection,
    tenant: &str,
    serial: &str,
    status: &str,
) -> Result<(), RepositoryError> {
    let device_status = match status {
        "repair_required" | "under_repair" => txt::STATUS_REPAIR,
        "repaired" | "no_action" => txt::STATUS_CHECKED_IN,
        "retired" => txt::STATUS_SCRAPPED,
        "lost" => txt::STATUS_LOST,
        _ => return Err(contract("unknown inventory disposition")),
    };
    let changed = c
        .execute(
            "UPDATE devices SET rentalStatus=?1 WHERE tenant_id=?2 AND serialNo=?3",
            params![device_status, tenant, serial],
        )
        .map_err(sqlite)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(contract(
            "device inventory record does not belong to the active tenant",
        ))
    }
}
fn load_liability(
    c: &rusqlite::Connection,
    tenant: &str,
    id: &str,
) -> Result<LiabilityDecisionProjection, RepositoryError> {
    c.query_row("SELECT id,finding_id,decision,apportioned_amount_minor,currency,status,version FROM rental_liability_decisions WHERE tenant_id=?1 AND id=?2",params![tenant,id],row_liability).map_err(sqlite)
}
fn load_repair(
    c: &rusqlite::Connection,
    tenant: &str,
    id: &str,
) -> Result<RepairCaseProjection, RepositoryError> {
    c.query_row("SELECT id,finding_id,device_serial_no,decision,status,version FROM rental_repair_cases WHERE tenant_id=?1 AND id=?2",params![tenant,id],row_repair).map_err(|_|contract("repair case not found in the active tenant"))
}
fn load_settlement(
    c: &rusqlite::Connection,
    tenant: &str,
    id: &str,
) -> Result<SettlementCaseProjection, RepositoryError> {
    c.query_row("SELECT id,order_id,currency,total_amount_minor,deposit_deducted_minor,status,facts_hash FROM rental_settlement_cases WHERE tenant_id=?1 AND id=?2",params![tenant,id],row_settlement).map_err(|_|contract("settlement case not found in the active tenant"))
}
fn load_effect(
    c: &rusqlite::Connection,
    tenant: &str,
    id: &str,
) -> Result<SettlementEffectAdmissionProjection, RepositoryError> {
    c.query_row("SELECT e.id,e.settlement_case_id,e.effect_kind,e.amount_minor,e.currency,e.external_operation_id,o.state FROM rental_settlement_effect_admissions e LEFT JOIN external_operations o ON o.tenant_id=e.tenant_id AND o.id=e.external_operation_id WHERE e.tenant_id=?1 AND e.id=?2",params![tenant,id],row_effect).map_err(|_|contract("settlement effect admission not found in the active tenant"))
}
fn load_dispute(
    c: &rusqlite::Connection,
    tenant: &str,
    id: &str,
) -> Result<DisputeProjection, RepositoryError> {
    c.query_row(
        "SELECT id,settlement_case_id,status FROM rental_disputes WHERE tenant_id=?1 AND id=?2",
        params![tenant, id],
        row_dispute,
    )
    .map_err(|_| contract("dispute not found in the active tenant"))
}
fn row_finding(row: &rusqlite::Row<'_>) -> rusqlite::Result<DamageFindingProjection> {
    Ok(DamageFindingProjection {
        id: row.get(0)?,
        order_id: row.get(1)?,
        inspection_id: row.get(2)?,
        device_serial_no: row.get(3)?,
        condition_code: row.get(4)?,
        status: row.get(5)?,
    })
}
fn row_liability(row: &rusqlite::Row<'_>) -> rusqlite::Result<LiabilityDecisionProjection> {
    Ok(LiabilityDecisionProjection {
        id: row.get(0)?,
        finding_id: row.get(1)?,
        decision: row.get(2)?,
        apportioned_amount_minor: row.get(3)?,
        currency: row.get(4)?,
        status: row.get(5)?,
        version: row.get(6)?,
    })
}
fn row_repair(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepairCaseProjection> {
    Ok(RepairCaseProjection {
        id: row.get(0)?,
        finding_id: row.get(1)?,
        device_serial_no: row.get(2)?,
        decision: row.get(3)?,
        status: row.get(4)?,
        version: row.get(5)?,
    })
}
fn row_settlement(row: &rusqlite::Row<'_>) -> rusqlite::Result<SettlementCaseProjection> {
    Ok(SettlementCaseProjection {
        id: row.get(0)?,
        order_id: row.get(1)?,
        currency: row.get(2)?,
        total_amount_minor: row.get(3)?,
        deposit_deducted_minor: row.get(4)?,
        status: row.get(5)?,
        facts_hash: row.get(6)?,
    })
}
fn row_effect(row: &rusqlite::Row<'_>) -> rusqlite::Result<SettlementEffectAdmissionProjection> {
    Ok(SettlementEffectAdmissionProjection {
        id: row.get(0)?,
        settlement_case_id: row.get(1)?,
        effect_kind: row.get(2)?,
        amount_minor: row.get(3)?,
        currency: row.get(4)?,
        external_operation_id: row.get(5)?,
        external_operation_state: row.get(6)?,
    })
}
fn row_dispute(row: &rusqlite::Row<'_>) -> rusqlite::Result<DisputeProjection> {
    Ok(DisputeProjection {
        id: row.get(0)?,
        settlement_case_id: row.get(1)?,
        status: row.get(2)?,
    })
}
fn emit_inspection_complete_if_ready(
    tx: &rusqlite::Transaction<'_>,
    tenant: &str,
    order_id: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    let counts:(i64,i64)=tx.query_row("SELECT COUNT(*),COALESCE(SUM(CASE WHEN i.status IN ('passed','failed') THEN 1 ELSE 0 END),0) FROM rental_inspections i JOIN rental_return_items ri ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id JOIN rental_returns r ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id WHERE i.tenant_id=?1 AND r.order_id=?2",params![tenant,order_id],|row|Ok((row.get(0)?,row.get(1)?))).map_err(sqlite)?;
    if counts.0 > 0 && counts.0 == counts.1 {
        append_outbox_tx(
            tx,
            tenant,
            "order",
            order_id,
            "InspectionComplete",
            &format!("inspection-complete:{order_id}"),
            &serde_json::json!({"orderId":order_id}),
            now,
        )?;
    }
    Ok(())
}
pub(in crate::repositories) fn validate_inspection_completion(
    input: &InspectionCompletionInput,
) -> Result<(), RepositoryError> {
    if input.expected_version < 1 {
        return Err(contract("expectedVersion must be positive"));
    }
    if !matches!(
        input.condition_code.as_str(),
        "good" | "damaged" | "missing" | "normal_wear"
    ) {
        return Err(contract("unknown inspection condition"));
    }
    if input.missing && input.condition_code != "missing" {
        return Err(contract(
            "missing inspection outcome must use conditionCode=missing",
        ));
    }
    if input.condition_code == "missing" && !input.missing {
        return Err(contract(
            "conditionCode=missing requires missing=true and can never pass inspection",
        ));
    }
    if input.normal_wear && input.condition_code != "normal_wear" {
        return Err(contract("normal wear must use conditionCode=normal_wear"));
    }
    Ok(())
}
pub(in crate::repositories) fn validate_finding(
    input: &DamageFindingInput,
) -> Result<(), RepositoryError> {
    required(&input.inspection_id, "inspectionId")?;
    required(&input.finding_code, "findingCode")?;
    required(&input.description, "description")?;
    required(&input.idempotency_key, "idempotencyKey")?;
    if !matches!(
        input.condition_code.as_str(),
        "damaged" | "missing" | "normal_wear"
    ) {
        return Err(contract(
            "damage finding condition must be damaged, missing, or normal_wear",
        ));
    }
    Ok(())
}
pub(in crate::repositories) fn validate_liability(
    input: &LiabilityDecisionInput,
) -> Result<(), RepositoryError> {
    required(&input.finding_id, "findingId")?;
    required(&input.rationale, "rationale")?;
    required(&input.idempotency_key, "idempotencyKey")?;
    currency(&input.currency)?;
    if !matches!(
        input.decision.as_str(),
        "liable" | "not_liable" | "partial" | "manual_review"
    ) {
        return Err(contract("unknown liability decision"));
    }
    if matches!(input.decision.as_str(), "liable" | "partial")
        && input.apportioned_amount_minor <= 0
    {
        return Err(contract(
            "liable and partial decisions require a positive authorized amount",
        ));
    }
    if matches!(input.decision.as_str(), "not_liable" | "manual_review")
        && input.apportioned_amount_minor != 0
    {
        return Err(contract(
            "not-liable and manual-review decisions must not authorize money",
        ));
    }
    Ok(())
}
pub(in crate::repositories) fn validate_repair_decision(
    value: &str,
) -> Result<(), RepositoryError> {
    if matches!(value, "repair_required" | "no_action" | "retired" | "lost") {
        Ok(())
    } else {
        Err(contract("unknown repair decision"))
    }
}
pub(in crate::repositories) fn validate_repair_status(value: &str) -> Result<(), RepositoryError> {
    if matches!(
        value,
        "repair_required" | "under_repair" | "repaired" | "retired" | "lost" | "no_action"
    ) {
        Ok(())
    } else {
        Err(contract("unknown repair status"))
    }
}
pub(in crate::repositories) fn validate_lines(
    lines: &[SettlementLineInput],
) -> Result<(), RepositoryError> {
    for line in lines {
        required(&line.finding_id, "findingId")?;
        required(&line.description, "line description")?;
        if line.amount_minor < 0 {
            return Err(contract("settlement line amount cannot be negative"));
        }
        if !matches!(
            line.line_kind.as_str(),
            "damage" | "missing_device" | "repair" | "overdue_adjustment"
        ) {
            return Err(contract("unknown settlement line kind"));
        }
    }
    Ok(())
}
pub(in crate::repositories) fn evidence_json(values: &[String]) -> Result<String, RepositoryError> {
    if values.iter().any(|value| value.trim().is_empty()) {
        return Err(contract("evidence references must not be blank"));
    }
    serde_json::to_string(values).map_err(|error| contract(error.to_string()))
}
pub(in crate::repositories) fn required<'a>(
    value: &'a str,
    name: &str,
) -> Result<&'a str, RepositoryError> {
    let value = value.trim();
    if value.is_empty() {
        Err(contract(format!("{name} must not be blank")))
    } else {
        Ok(value)
    }
}
pub(in crate::repositories) fn currency(value: &str) -> Result<String, RepositoryError> {
    let value = required(value, "currency")?.to_ascii_uppercase();
    if value.len() != 3
        || !value
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        Err(contract("currency must use ISO three-letter semantics"))
    } else {
        Ok(value)
    }
}
fn hash(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}
pub(in crate::repositories) fn settlement_hash(
    order_id: &str,
    currency: &str,
    lines: &[SettlementLineInput],
    findings: &[FindingDecisionRow],
) -> String {
    let mut material = format!("order={order_id};currency={currency};");
    for line in lines {
        material.push_str(&format!(
            "{}:{}:{};",
            line.finding_id, line.line_kind, line.amount_minor
        ));
    }
    for finding in findings {
        material.push_str(&format!(
            "finding={} decision_id={} decision={} amount={} currency={} status={};",
            finding.finding_id,
            finding.decision_id,
            finding.decision,
            finding.amount_minor,
            finding.currency,
            finding.status,
        ));
    }
    hash(&material)
}
fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn format_utc(instant: chrono::DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn sqlite(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}
pub(in crate::repositories) fn contract(message: impl Into<String>) -> RepositoryError {
    RepositoryError::ContractViolation(message.into())
}
