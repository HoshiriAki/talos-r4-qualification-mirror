#![cfg(feature = "postgres")]

use std::sync::Arc;

use chrono::{SecondsFormat, Utc};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::application::rental_time::TenantBusinessTimeZone;
use crate::integration::operation::{FinancialEffectState, OperationState};
use crate::integration::settlement_admission::{
    SettlementChargeAdmission, admit_settlement_charge_pg,
};
use crate::observability::{IntegrationEvent, MetricsSink, OperationStateClass};
use crate::repositories::lifecycle_postgres_write::apply_action_in_pg_transaction;
use crate::repositories::r3_settlement::{
    AdditionalChargeInput, DamageFindingInput, DamageFindingProjection, DepositDeductionInput,
    DisputeProjection, FindingDecisionRow, InspectionCompletionInput,
    InspectionCompletionProjection, LiabilityDecisionInput, LiabilityDecisionProjection,
    OpenDisputeInput, R3ClosureFacts, RepairCaseProjection, RepairDecisionInput,
    RepairTransitionInput, ResolveDisputeInput, SettlementCaseProjection,
    SettlementEffectAdmissionProjection, SettlementProposalInput, contract, currency,
    evidence_json, required, settlement_hash, validate_finding, validate_inspection_completion,
    validate_liability, validate_lines, validate_repair_decision, validate_repair_status,
    validate_settlement_lines,
};
use crate::repositories::{RepositoryError, RepositorySession, ScopedRepositories};
use crate::utils::constants::txt;

pub(in crate::repositories) struct PostgresR3SettlementRepository<'a> {
    session: &'a RepositorySession,
    metrics: Arc<dyn MetricsSink>,
}

impl<'a> PostgresR3SettlementRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
            metrics: scoped.metrics(),
        }
    }

    pub(in crate::repositories) fn configure_business_timezone(
        &self,
        time_zone_id: &str,
        actor: &str,
    ) -> Result<String, RepositoryError> {
        let tenant = self.tenant();
        let zone = TenantBusinessTimeZone::parse(time_zone_id).map_err(contract)?;
        let actor = required(actor, "actor")?.to_owned();
        let zone_id = zone.id().to_owned();
        let now = utc_now();

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "INSERT INTO tenant_business_time_zones                      (tenant_id,time_zone_id,configured_by,created_at,updated_at)                      VALUES ($1,$2,$3,$4,$4)                      ON CONFLICT (tenant_id) DO UPDATE SET                        time_zone_id=EXCLUDED.time_zone_id,                        configured_by=EXCLUDED.configured_by,                        updated_at=EXCLUDED.updated_at",
                )
                .bind(&tenant)
                .bind(&zone_id)
                .bind(&actor)
                .bind(&now)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;
                Ok(zone_id)
            })
        })
    }

    pub(in crate::repositories) fn business_timezone(&self) -> Result<String, RepositoryError> {
        let tenant = self.tenant();
        self.session
            .pg_read(move |connection| {
                Box::pin(async move {
                    let row = sqlx::query_scalar::<_, String>(
                        "SELECT time_zone_id FROM tenant_business_time_zones WHERE tenant_id=$1",
                    )
                    .bind(&tenant)
                    .fetch_optional(&mut *connection)
                    .await?;
                    row.ok_or(sqlx::Error::RowNotFound)
                })
            })
            .map_err(|error| match error {
                RepositoryError::Postgres(detail) if detail.contains("no rows returned") => {
                    contract("tenant business timezone is not configured")
                }
                RepositoryError::Postgres(detail) if detail.contains("RowNotFound") => {
                    contract("tenant business timezone is not configured")
                }
                other => other,
            })
    }

    pub(in crate::repositories) fn complete_inspection(
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

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT i.status,r.order_id,i.version,i.condition_code,i.missing,                             i.normal_wear,i.evidence_refs_json,i.business_date                      FROM rental_inspections i                      JOIN rental_return_items ri                        ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id                      JOIN rental_returns r                        ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id                      WHERE i.tenant_id=$1 AND i.id=$2                      FOR UPDATE OF i",
                )
                .bind(&tenant)
                .bind(&input.inspection_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("inspection missing or foreign to the active tenant"))?;

                let status: String = row.try_get("status").map_err(pg_error)?;
                let order_id: String = row.try_get("order_id").map_err(pg_error)?;
                let version: i64 = row.try_get("version").map_err(pg_error)?;
                let recorded_condition: String =
                    row.try_get("condition_code").map_err(pg_error)?;
                let recorded_missing: bool = row.try_get("missing").map_err(pg_error)?;
                let recorded_wear: bool = row.try_get("normal_wear").map_err(pg_error)?;
                let recorded_evidence: String =
                    row.try_get("evidence_refs_json").map_err(pg_error)?;
                let recorded_business_date: Option<String> =
                    row.try_get("business_date").map_err(pg_error)?;

                let terminal = if input.condition_code == "damaged" || input.missing {
                    "failed"
                } else {
                    "passed"
                };

                if matches!(status.as_str(), "passed" | "failed") {
                    if version == input.expected_version + 1
                        && status == terminal
                        && recorded_condition == input.condition_code
                        && recorded_missing == input.missing
                        && recorded_wear == input.normal_wear
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
                    return Err(contract(
                        "terminal inspection completion is idempotent only for identical facts and expected version",
                    ));
                }

                if version != input.expected_version {
                    return Err(contract("inspection optimistic version conflict"));
                }
                if status != "in_progress" {
                    return Err(contract(
                        "inspection must be in_progress before a final result is recorded",
                    ));
                }

                let changed = sqlx::query(
                    "UPDATE rental_inspections                      SET status=$1,condition_code=$2,missing=$3,normal_wear=$4,                          evidence_refs_json=$5,business_date=$6,inspected_by=$7,                          completed_at=$8,updated_at=$8,version=version+1                      WHERE tenant_id=$9 AND id=$10 AND version=$11",
                )
                .bind(terminal)
                .bind(&input.condition_code)
                .bind(input.missing)
                .bind(input.normal_wear)
                .bind(&evidence)
                .bind(&business_date)
                .bind(&actor)
                .bind(&now_text)
                .bind(&tenant)
                .bind(&input.inspection_id)
                .bind(input.expected_version)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?
                .rows_affected();

                if changed != 1 {
                    return Err(contract("inspection finalization lost its expected version"));
                }

                emit_inspection_complete_if_ready_pg(
                    connection,
                    &tenant,
                    &order_id,
                    &now_text,
                )
                .await?;

                Ok(InspectionCompletionProjection {
                    inspection_id: input.inspection_id,
                    order_id,
                    status: terminal.into(),
                    condition_code: input.condition_code,
                    business_date,
                })
            })
        })
    }

    pub(in crate::repositories) fn create_damage_finding(
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

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                if let Some(existing) = load_finding_by_idempotency_pg(
                    connection,
                    &tenant,
                    &input.inspection_id,
                    &input.idempotency_key,
                )
                .await?
                {
                    return Ok(existing);
                }

                let row = sqlx::query(
                    "SELECT r.order_id,i.device_serial_no,i.status,i.condition_code                      FROM rental_inspections i                      JOIN rental_return_items ri                        ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id                      JOIN rental_returns r                        ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id                      WHERE i.tenant_id=$1 AND i.id=$2                      FOR SHARE OF i",
                )
                .bind(&tenant)
                .bind(&input.inspection_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("inspection not found in the active tenant"))?;

                let order_id: String = row.try_get("order_id").map_err(pg_error)?;
                let device_serial_no: String =
                    row.try_get("device_serial_no").map_err(pg_error)?;
                let inspection_status: String = row.try_get("status").map_err(pg_error)?;
                let condition: String = row.try_get("condition_code").map_err(pg_error)?;

                if !matches!(inspection_status.as_str(), "passed" | "failed") {
                    return Err(contract(
                        "a damage finding requires terminal inspection evidence",
                    ));
                }
                if input.condition_code != "normal_wear" && condition != input.condition_code {
                    return Err(contract(
                        "damage finding condition conflicts with the final inspection outcome",
                    ));
                }

                assert_order_settlement_unsealed_pg(connection, &tenant, &order_id).await?;

                let id = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO rental_damage_findings                      (id,tenant_id,inspection_id,order_id,device_serial_no,finding_code,                       condition_code,description,evidence_refs_json,status,command_idempotency_key,                       found_by,occurred_at,business_date,created_at,updated_at)                      VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'open',$10,$11,$12,$13,$12,$12)",
                )
                .bind(&id)
                .bind(&tenant)
                .bind(&input.inspection_id)
                .bind(&order_id)
                .bind(&device_serial_no)
                .bind(&input.finding_code)
                .bind(&input.condition_code)
                .bind(&input.description)
                .bind(&evidence)
                .bind(&input.idempotency_key)
                .bind(&actor)
                .bind(&now_text)
                .bind(&business_date)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(DamageFindingProjection {
                    id,
                    order_id,
                    inspection_id: input.inspection_id,
                    device_serial_no,
                    condition_code: input.condition_code,
                    status: "open".into(),
                })
            })
        })
    }

    pub(in crate::repositories) fn decide_liability(
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

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let finding = sqlx::query(
                    "SELECT id,order_id FROM rental_damage_findings                      WHERE tenant_id=$1 AND id=$2 AND status='open' FOR SHARE",
                )
                .bind(&tenant)
                .bind(&input.finding_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;

                let Some(finding) = finding else {
                    return Err(contract(
                        "open damage finding not found in the active tenant",
                    ));
                };
                let order_id: String = finding.try_get("order_id").map_err(pg_error)?;

                if let Some(existing) =
                    load_liability_by_finding_pg(connection, &tenant, &input.finding_id, true)
                        .await?
                {
                    if existing.status == "open" && existing.decision == "manual_review" {
                        assert_order_settlement_unsealed_pg(connection, &tenant, &order_id).await?;
                        let next_status = if input.decision == "manual_review" {
                            "open"
                        } else {
                            "decided"
                        };
                        let changed = sqlx::query(
                            "UPDATE rental_liability_decisions                              SET decision=$1,apportioned_amount_minor=$2,currency=$3,rationale=$4,                                  evidence_refs_json=$5,status=$6,version=version+1,                                  command_idempotency_key=$7,decided_by=$8,decided_at=$9,                                  business_date=$10,updated_at=$9                              WHERE tenant_id=$11 AND id=$12 AND version=$13",
                        )
                        .bind(&input.decision)
                        .bind(input.apportioned_amount_minor)
                        .bind(&currency)
                        .bind(&input.rationale)
                        .bind(&evidence)
                        .bind(next_status)
                        .bind(&input.idempotency_key)
                        .bind(&actor)
                        .bind(&now_text)
                        .bind(&business_date)
                        .bind(&tenant)
                        .bind(&existing.id)
                        .bind(existing.version)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?
                        .rows_affected();

                        if changed != 1 {
                            return Err(contract(
                                "liability decision optimistic version conflict",
                            ));
                        }
                        return load_liability_pg(connection, &tenant, &existing.id).await;
                    }

                    let expected_status = if input.decision == "manual_review" {
                        "open"
                    } else {
                        "decided"
                    };
                    if existing.decision == input.decision
                        && existing.apportioned_amount_minor == input.apportioned_amount_minor
                        && existing.currency == currency
                        && existing.status == expected_status
                    {
                        return Ok(existing);
                    }
                    return Err(contract(
                        "a final liability decision is immutable; reopen only through an approved migration",
                    ));
                }

                assert_order_settlement_unsealed_pg(connection, &tenant, &order_id).await?;
                let id = Uuid::new_v4().to_string();
                let status = if input.decision == "manual_review" {
                    "open"
                } else {
                    "decided"
                };
                sqlx::query(
                    "INSERT INTO rental_liability_decisions                      (id,tenant_id,finding_id,decision,apportioned_amount_minor,currency,rationale,                       evidence_refs_json,status,version,command_idempotency_key,decided_by,decided_at,                       business_date,created_at,updated_at)                      VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,1,$10,$11,$12,$13,$12,$12)",
                )
                .bind(&id)
                .bind(&tenant)
                .bind(&input.finding_id)
                .bind(&input.decision)
                .bind(input.apportioned_amount_minor)
                .bind(&currency)
                .bind(&input.rationale)
                .bind(&evidence)
                .bind(status)
                .bind(&input.idempotency_key)
                .bind(&actor)
                .bind(&now_text)
                .bind(&business_date)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                load_liability_pg(connection, &tenant, &id).await
            })
        })
    }

    pub(in crate::repositories) fn decide_repair(
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

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                if let Some(existing) =
                    load_repair_by_finding_pg(connection, &tenant, &input.finding_id, true).await?
                {
                    if existing.decision == input.decision {
                        return Ok(existing);
                    }
                    return Err(contract(
                        "repair decision is already authoritative for this finding",
                    ));
                }

                let row = sqlx::query(
                    "SELECT f.order_id,f.device_serial_no,l.status                      FROM rental_damage_findings f                      JOIN rental_liability_decisions l                        ON l.tenant_id=f.tenant_id AND l.finding_id=f.id                      WHERE f.tenant_id=$1 AND f.id=$2 AND f.status='open'                      FOR SHARE OF f,l",
                )
                .bind(&tenant)
                .bind(&input.finding_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| {
                    contract(
                        "an authoritative liability decision is required before repair decision",
                    )
                })?;

                let order_id: String = row.try_get("order_id").map_err(pg_error)?;
                let serial_no: String = row.try_get("device_serial_no").map_err(pg_error)?;
                let liability_status: String = row.try_get("status").map_err(pg_error)?;
                if liability_status != "decided" {
                    return Err(contract("repair decision is blocked by unresolved liability"));
                }

                assert_order_settlement_unsealed_pg(connection, &tenant, &order_id).await?;
                assert_device_for_tenant_pg(connection, &tenant, &serial_no).await?;
                apply_device_inventory_state_pg(
                    connection,
                    &tenant,
                    &serial_no,
                    &input.decision,
                )
                .await?;

                let id = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO rental_repair_cases                      (id,tenant_id,finding_id,order_id,device_serial_no,decision,status,version,                       command_idempotency_key,decided_by,decided_at,completed_at,business_date,                       created_at,updated_at)                      VALUES ($1,$2,$3,$4,$5,$6,$7,1,$8,$9,$10,                              CASE WHEN $7 IN ('retired','lost','no_action') THEN $10 ELSE NULL END,                              $11,$10,$10)",
                )
                .bind(&id)
                .bind(&tenant)
                .bind(&input.finding_id)
                .bind(&order_id)
                .bind(&serial_no)
                .bind(&input.decision)
                .bind(&input.decision)
                .bind(&input.idempotency_key)
                .bind(&actor)
                .bind(&now_text)
                .bind(&business_date)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                load_repair_pg(connection, &tenant, &id).await
            })
        })
    }

    pub(in crate::repositories) fn transition_repair(
        &self,
        input: RepairTransitionInput,
    ) -> Result<RepairCaseProjection, RepositoryError> {
        validate_repair_status(&input.target_status)?;
        if input.expected_version < 1 {
            return Err(contract("expectedVersion must be positive"));
        }
        let tenant = self.tenant();
        let now = utc_now();

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let current =
                    load_repair_for_update_pg(connection, &tenant, &input.repair_case_id).await?;
                if current.version != input.expected_version {
                    return Err(contract("repair case optimistic version conflict"));
                }
                let order_id = sqlx::query_scalar::<_, String>(
                    "SELECT order_id FROM rental_repair_cases                      WHERE tenant_id=$1 AND id=$2",
                )
                .bind(&tenant)
                .bind(&input.repair_case_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("repair case not found in the active tenant"))?;

                assert_order_settlement_unsealed_pg(connection, &tenant, &order_id).await?;

                let legal = matches!(
                    (current.status.as_str(), input.target_status.as_str()),
                    ("repair_required", "under_repair")
                        | ("repair_required", "retired")
                        | ("repair_required", "lost")
                        | ("under_repair", "repaired")
                        | ("under_repair", "retired")
                        | ("under_repair", "lost")
                );
                if !legal {
                    return Err(contract("illegal repair/inventory transition"));
                }

                apply_device_inventory_state_pg(
                    connection,
                    &tenant,
                    &current.device_serial_no,
                    &input.target_status,
                )
                .await?;

                let changed = sqlx::query(
                    "UPDATE rental_repair_cases                      SET status=$1,version=version+1,                          completed_at=CASE WHEN $1 IN ('repaired','retired','lost','no_action')                                            THEN $2 ELSE completed_at END,                          updated_at=$2                      WHERE tenant_id=$3 AND id=$4 AND version=$5",
                )
                .bind(&input.target_status)
                .bind(&now)
                .bind(&tenant)
                .bind(&input.repair_case_id)
                .bind(input.expected_version)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?
                .rows_affected();

                if changed != 1 {
                    return Err(contract("repair transition lost its expected version"));
                }

                load_repair_pg(connection, &tenant, &input.repair_case_id).await
            })
        })
    }

    pub(in crate::repositories) fn propose_settlement(
        &self,
        input: SettlementProposalInput,
        actor: &str,
    ) -> Result<SettlementCaseProjection, RepositoryError> {
        let tenant = self.tenant();
        let order_id = required(&input.order_id, "orderId")?.to_owned();
        let idempotency_key = required(&input.idempotency_key, "idempotencyKey")?.to_owned();
        let settlement_currency = currency(&input.currency)?;
        let actor = required(actor, "actor")?.to_owned();
        validate_lines(&input.lines)?;
        let zone = self.zone()?;
        let now = Utc::now();
        let now_text = format_utc(now);
        let business_date = zone.business_date(now).to_string();

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                if let Some(existing) = load_settlement_by_idempotency_pg(
                    connection,
                    &tenant,
                    &idempotency_key,
                )
                .await?
                {
                    return Ok(existing);
                }

                if load_settlement_by_order_pg(connection, &tenant, &order_id)
                    .await?
                    .is_some()
                {
                    return Err(contract(
                        "an authoritative settlement case already exists for this order",
                    ));
                }

                let order_exists = sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM orders WHERE tenant_id=$1 AND id=$2)",
                )
                .bind(&tenant)
                .bind(&order_id)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;
                if !order_exists {
                    return Err(contract(
                        "settlement proposal requires an existing scoped order",
                    ));
                }

                let findings = load_settlement_findings_pg(connection, &tenant, &order_id).await?;
                validate_settlement_lines(&findings, &input.lines, &settlement_currency)?;
                let total: i64 = input.lines.iter().map(|line| line.amount_minor).sum();
                let facts_hash =
                    settlement_hash(&order_id, &settlement_currency, &input.lines, &findings);
                let id = Uuid::new_v4().to_string();

                sqlx::query(
                    "INSERT INTO rental_settlement_cases                      (id,tenant_id,order_id,currency,total_amount_minor,deposit_deducted_minor,                       status,facts_hash,command_idempotency_key,proposed_by,proposed_at,                       accounting_business_date,created_at,updated_at)                      VALUES ($1,$2,$3,$4,$5,0,'proposed',$6,$7,$8,$9,$10,$9,$9)",
                )
                .bind(&id)
                .bind(&tenant)
                .bind(&order_id)
                .bind(&settlement_currency)
                .bind(total)
                .bind(&facts_hash)
                .bind(&idempotency_key)
                .bind(&actor)
                .bind(&now_text)
                .bind(&business_date)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                for line in &input.lines {
                    let decision = findings
                        .iter()
                        .find(|item| item.finding_id == line.finding_id)
                        .expect("validated line finding");
                    sqlx::query(
                        "INSERT INTO rental_settlement_lines                          (id,tenant_id,settlement_case_id,finding_id,liability_decision_id,                           line_kind,amount_minor,currency,description,created_at)                          VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
                    )
                    .bind(Uuid::new_v4().to_string())
                    .bind(&tenant)
                    .bind(&id)
                    .bind(&line.finding_id)
                    .bind(&decision.decision_id)
                    .bind(&line.line_kind)
                    .bind(line.amount_minor)
                    .bind(&settlement_currency)
                    .bind(&line.description)
                    .bind(&now_text)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                }

                sqlx::query(
                    "INSERT INTO rental_settlements                      (id,tenant_id,order_id,currency,amount_minor,facts_hash,status,blocker_code,                       created_at,updated_at)                      VALUES ($1,$2,$3,$4,$5,$6,'calculated',NULL,$7,$7)                      ON CONFLICT (tenant_id,order_id) DO UPDATE SET                        currency=EXCLUDED.currency,amount_minor=EXCLUDED.amount_minor,                        facts_hash=EXCLUDED.facts_hash,status='calculated',blocker_code=NULL,                        updated_at=EXCLUDED.updated_at",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(&tenant)
                .bind(&order_id)
                .bind(&settlement_currency)
                .bind(total)
                .bind(&facts_hash)
                .bind(&now_text)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                load_settlement_pg(connection, &tenant, &id).await
            })
        })
    }

    pub(in crate::repositories) fn accept_settlement(
        &self,
        settlement_case_id: &str,
    ) -> Result<SettlementCaseProjection, RepositoryError> {
        let tenant = self.tenant();
        let id = required(settlement_case_id, "settlementCaseId")?.to_owned();
        let now = utc_now();

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let existing = load_settlement_for_update_pg(connection, &tenant, &id).await?;
                if existing.status == "accepted" {
                    return Ok(existing);
                }
                if !matches!(existing.status.as_str(), "proposed" | "resolved") {
                    return Err(contract(
                        "settlement may be accepted only from proposed or resolved",
                    ));
                }
                if active_dispute_count_pg(connection, &tenant, &id).await? != 0 {
                    return Err(contract("an active dispute blocks settlement acceptance"));
                }

                sqlx::query(
                    "UPDATE rental_settlement_cases                      SET status='accepted',accepted_at=$1,updated_at=$1                      WHERE tenant_id=$2 AND id=$3",
                )
                .bind(&now)
                .bind(&tenant)
                .bind(&id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                load_settlement_pg(connection, &tenant, &id).await
            })
        })
    }

    pub(in crate::repositories) fn deduct_deposit(
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

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let case =
                    load_settlement_for_update_pg(connection, &tenant, &input.settlement_case_id)
                        .await?;
                assert_order_settlement_unsealed_pg(connection, &tenant, &case.order_id).await?;
                if !matches!(case.status.as_str(), "accepted" | "resolved") {
                    return Err(contract(
                        "deposit deduction requires an accepted settlement",
                    ));
                }

                if let Some(existing) = load_effect_by_idempotency_pg(
                    connection,
                    &tenant,
                    &input.settlement_case_id,
                    &idempotency,
                )
                .await?
                {
                    if existing.effect_kind == "deposit_deduction"
                        && existing.amount_minor == input.amount_minor
                    {
                        return Ok(existing);
                    }
                    return Err(contract(
                        "effect idempotency key was reused with a different financial intent",
                    ));
                }

                assert_financial_reservation_pg(connection, &tenant, &case, input.amount_minor, 0)
                    .await?;

                let deposit = sqlx::query(
                    "SELECT authority_kind,authority_id,currency,state                      FROM integration_deposits                      WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
                )
                .bind(&tenant)
                .bind(&input.deposit_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("deposit not found in the active tenant"))?;

                let authority_kind: String =
                    deposit.try_get("authority_kind").map_err(pg_error)?;
                let authority_id: String = deposit.try_get("authority_id").map_err(pg_error)?;
                let expected_currency: String =
                    deposit.try_get("currency").map_err(pg_error)?;
                let state: String = deposit.try_get("state").map_err(pg_error)?;

                if authority_kind != "order" || authority_id != case.order_id {
                    return Err(contract(
                        "deposit is not authorized for this tenant/order settlement",
                    ));
                }
                if expected_currency != case.currency {
                    return Err(contract("deposit currency differs from settlement currency"));
                }
                if !matches!(state.as_str(), "recorded" | "held" | "partially_deducted") {
                    return Err(contract("deposit is in an incompatible terminal state"));
                }

                let available =
                    deposit_available_pg(connection, &tenant, &input.deposit_id).await?;
                if input.amount_minor > available {
                    return Err(contract(
                        "deposit deduction exceeds available authorized deposit",
                    ));
                }

                let effect_id = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO rental_settlement_effect_admissions                      (id,tenant_id,settlement_case_id,effect_kind,amount_minor,currency,deposit_id,                       external_operation_id,idempotency_key,business_authorization_ref,created_by,created_at)                      VALUES ($1,$2,$3,'deposit_deduction',$4,$5,$6,NULL,$7,$8,$9,$10)",
                )
                .bind(&effect_id)
                .bind(&tenant)
                .bind(&case.id)
                .bind(input.amount_minor)
                .bind(&case.currency)
                .bind(&input.deposit_id)
                .bind(&idempotency)
                .bind(format!("settlement-case:{}", case.id))
                .bind(&actor)
                .bind(&now)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                sqlx::query(
                    "INSERT INTO integration_deposit_ledger                      (id,tenant_id,deposit_id,entry_type,amount_minor,currency,external_operation_id,                       audit_ref,created_at)                      VALUES ($1,$2,$3,'deducted',$4,$5,NULL,$6,$7)",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(&tenant)
                .bind(&input.deposit_id)
                .bind(input.amount_minor)
                .bind(&case.currency)
                .bind(format!("r3-settlement-effect:{effect_id}"))
                .bind(&now)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                sqlx::query(
                    "UPDATE integration_deposits                      SET state='partially_deducted',updated_at=$1                      WHERE tenant_id=$2 AND id=$3",
                )
                .bind(&now)
                .bind(&tenant)
                .bind(&input.deposit_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                sqlx::query(
                    "UPDATE rental_settlement_cases                      SET deposit_deducted_minor=deposit_deducted_minor+$1,updated_at=$2                      WHERE tenant_id=$3 AND id=$4",
                )
                .bind(input.amount_minor)
                .bind(&now)
                .bind(&tenant)
                .bind(&case.id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                load_effect_pg(connection, &tenant, &effect_id).await
            })
        })
    }

    pub(in crate::repositories) fn admit_additional_charge(
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

        let (effect, created_new) = self.session.pg_write_serializable_repository(
            move |connection| {
                Box::pin(async move {
                    let case = load_settlement_for_update_pg(
                        connection,
                        &tenant,
                        &input.settlement_case_id,
                    )
                    .await?;
                    assert_order_settlement_unsealed_pg(connection, &tenant, &case.order_id).await?;
                    if !matches!(case.status.as_str(), "accepted" | "resolved") {
                        return Err(contract(
                            "additional charge intent requires an accepted settlement",
                        ));
                    }

                    if let Some(existing) = load_effect_by_idempotency_pg(
                        connection,
                        &tenant,
                        &case.id,
                        &idempotency,
                    )
                    .await?
                    {
                        if existing.effect_kind == "additional_charge"
                            && existing.amount_minor == input.amount_minor
                        {
                            return Ok((existing, false));
                        }
                        return Err(contract(
                            "effect idempotency key was reused with a different financial intent",
                        ));
                    }

                    assert_financial_reservation_pg(
                        connection,
                        &tenant,
                        &case,
                        0,
                        input.amount_minor,
                    )
                    .await?;

                    let authorization_ref = format!("settlement-case:{}", case.id);
                    let admitted = admit_settlement_charge_pg(
                        connection,
                        &SettlementChargeAdmission {
                            tenant_id: &tenant,
                            settlement_case_id: &case.id,
                            order_id: &case.order_id,
                            amount_minor: input.amount_minor,
                            currency: &case.currency,
                            business_authorization_ref: &authorization_ref,
                            idempotency_key: &idempotency,
                        },
                    )
                    .await
                    .map_err(contract)?;

                    let id = Uuid::new_v4().to_string();
                    sqlx::query(
                        "INSERT INTO rental_settlement_effect_admissions                          (id,tenant_id,settlement_case_id,effect_kind,amount_minor,currency,deposit_id,                           external_operation_id,idempotency_key,business_authorization_ref,created_by,created_at)                          VALUES ($1,$2,$3,'additional_charge',$4,$5,NULL,$6,$7,$8,$9,$10)",
                    )
                    .bind(&id)
                    .bind(&tenant)
                    .bind(&case.id)
                    .bind(input.amount_minor)
                    .bind(&case.currency)
                    .bind(&admitted.external_operation_id)
                    .bind(&idempotency)
                    .bind(&authorization_ref)
                    .bind(&actor)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    Ok((
                        load_effect_pg(connection, &tenant, &id).await?,
                        admitted.created_new,
                    ))
                })
            },
        )?;

        if created_new {
            self.metrics
                .integration_operation(IntegrationEvent::Admitted, OperationStateClass::Ready);
        }
        Ok(effect)
    }

    pub(in crate::repositories) fn open_dispute(
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

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let case =
                    load_settlement_for_update_pg(connection, &tenant, &input.settlement_case_id)
                        .await?;
                assert_order_settlement_unsealed_pg(connection, &tenant, &case.order_id).await?;
                if !matches!(case.status.as_str(), "proposed" | "accepted" | "resolved") {
                    return Err(contract("settled cases cannot be disputed"));
                }

                if let Some(existing) = load_dispute_by_idempotency_pg(
                    connection,
                    &tenant,
                    &case.id,
                    &idempotency,
                )
                .await?
                {
                    return Ok(existing);
                }

                let id = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO rental_disputes                      (id,tenant_id,settlement_case_id,status,reason,evidence_refs_json,                       command_idempotency_key,opened_by,opened_at,created_at,updated_at)                      VALUES ($1,$2,$3,'open',$4,$5,$6,$7,$8,$8,$8)",
                )
                .bind(&id)
                .bind(&tenant)
                .bind(&case.id)
                .bind(&reason)
                .bind(&evidence)
                .bind(&idempotency)
                .bind(&actor)
                .bind(&now)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                sqlx::query(
                    "UPDATE rental_settlement_cases                      SET status='disputed',updated_at=$1 WHERE tenant_id=$2 AND id=$3",
                )
                .bind(&now)
                .bind(&tenant)
                .bind(&case.id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(DisputeProjection {
                    id,
                    settlement_case_id: case.id,
                    status: "open".into(),
                })
            })
        })
    }

    pub(in crate::repositories) fn resolve_dispute(
        &self,
        input: ResolveDisputeInput,
        actor: &str,
    ) -> Result<DisputeProjection, RepositoryError> {
        let tenant = self.tenant();
        let actor = required(actor, "actor")?.to_owned();
        let note = required(&input.resolution_note, "resolutionNote")?.to_owned();
        let now = utc_now();

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let dispute =
                    load_dispute_for_update_pg(connection, &tenant, &input.dispute_id).await?;
                let order_id = sqlx::query_scalar::<_, String>(
                    "SELECT s.order_id                      FROM rental_settlement_cases s                      JOIN rental_disputes d                        ON d.tenant_id=s.tenant_id AND d.settlement_case_id=s.id                      WHERE d.tenant_id=$1 AND d.id=$2",
                )
                .bind(&tenant)
                .bind(&dispute.id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("settlement case not found in the active tenant"))?;

                assert_order_settlement_unsealed_pg(connection, &tenant, &order_id).await?;
                if !matches!(dispute.status.as_str(), "open" | "under_review") {
                    return Err(contract(
                        "only an active dispute may be manually resolved",
                    ));
                }

                sqlx::query(
                    "UPDATE rental_disputes                      SET status='resolved',resolved_by=$1,resolved_at=$2,resolution_note=$3,                          updated_at=$2                      WHERE tenant_id=$4 AND id=$5",
                )
                .bind(&actor)
                .bind(&now)
                .bind(&note)
                .bind(&tenant)
                .bind(&dispute.id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                if active_dispute_count_pg(connection, &tenant, &dispute.settlement_case_id)
                    .await?
                    == 0
                {
                    sqlx::query(
                        "UPDATE rental_settlement_cases                          SET status='resolved',updated_at=$1                          WHERE tenant_id=$2 AND id=$3 AND status='disputed'",
                    )
                    .bind(&now)
                    .bind(&tenant)
                    .bind(&dispute.settlement_case_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                }

                Ok(DisputeProjection {
                    id: dispute.id,
                    settlement_case_id: dispute.settlement_case_id,
                    status: "resolved".into(),
                })
            })
        })
    }

    pub(in crate::repositories) fn complete_settlement(
        &self,
        settlement_case_id: &str,
    ) -> Result<SettlementCaseProjection, RepositoryError> {
        let tenant = self.tenant();
        let id = required(settlement_case_id, "settlementCaseId")?.to_owned();
        let now = utc_now();

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let case = load_settlement_for_update_pg(connection, &tenant, &id).await?;
                if case.status == "settled" {
                    return Ok(case);
                }
                if !matches!(case.status.as_str(), "accepted" | "resolved") {
                    return Err(contract(
                        "settlement must be accepted and dispute-clear before terminal settlement",
                    ));
                }

                let facts = closure_facts_pg(connection, &tenant, &case.order_id).await?;
                if !facts.return_received
                    || !facts.inspection_complete
                    || facts.unresolved_damage_findings != 0
                    || facts.unresolved_liability_decisions != 0
                    || facts.unresolved_repair_cases != 0
                    || facts.active_disputes != 0
                    || facts.unresolved_effect_intents != 0
                    || facts.overdue_open
                    || !facts.financial_balance_fully_accounted
                {
                    return Err(contract(
                        "settlement requires received return, completed inspection, zero blockers, resolved R2 external operations, no overdue record, and exact accounting",
                    ));
                }

                sqlx::query(
                    "UPDATE rental_settlement_cases                      SET status='settled',settled_at=$1,updated_at=$1                      WHERE tenant_id=$2 AND id=$3",
                )
                .bind(&now)
                .bind(&tenant)
                .bind(&case.id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                sqlx::query(
                    "UPDATE rental_settlements                      SET status='terminal',blocker_code=NULL,updated_at=$1                      WHERE tenant_id=$2 AND order_id=$3",
                )
                .bind(&now)
                .bind(&tenant)
                .bind(&case.order_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                append_outbox_pg(
                    connection,
                    &tenant,
                    "order",
                    &case.order_id,
                    "SettlementComplete",
                    &format!("r3-settlement-complete:{}", case.id),
                    &serde_json::json!({
                        "orderId": case.order_id,
                        "settlementCaseId": case.id
                    }),
                    &now,
                )
                .await?;

                load_settlement_pg(connection, &tenant, &case.id).await
            })
        })
    }

    pub(in crate::repositories) fn closure_facts(
        &self,
        order_id: &str,
    ) -> Result<R3ClosureFacts, RepositoryError> {
        let tenant = self.tenant();
        let order_id = required(order_id, "orderId")?.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move { closure_facts_pg_sqlx(connection, &tenant, &order_id).await })
        })
    }

    pub(in crate::repositories) fn close_order_atomic(
        &self,
        order_id: &str,
        expected_version: i64,
        actor: &str,
    ) -> Result<(), RepositoryError> {
        let tenant = self.tenant();
        let order_id = required(order_id, "orderId")?.to_owned();
        let actor = required(actor, "actor")?.to_owned();

        self.session.pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let facts = closure_facts_pg(connection, &tenant, &order_id).await?;
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

                apply_action_in_pg_transaction(
                    connection,
                    &tenant,
                    &order_id,
                    "close_order",
                    expected_version,
                    &actor,
                    "R3 terminal settlement and closure invariant verified in the same transaction",
                )
                .await?;
                Ok(())
            })
        })
    }

    fn tenant(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }

    fn zone(&self) -> Result<TenantBusinessTimeZone, RepositoryError> {
        TenantBusinessTimeZone::parse(&self.business_timezone()?).map_err(contract)
    }
}

#[derive(Debug, Default)]
struct FinancialEffectSummaryPg {
    accounted_minor: i64,
    reserved_minor: i64,
    unresolved_effects: i64,
}

async fn load_finding_by_idempotency_pg(
    connection: &mut PgConnection,
    tenant: &str,
    inspection_id: &str,
    idempotency_key: &str,
) -> Result<Option<DamageFindingProjection>, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,order_id,inspection_id,device_serial_no,condition_code,status          FROM rental_damage_findings          WHERE tenant_id=$1 AND inspection_id=$2 AND command_idempotency_key=$3",
    )
    .bind(tenant)
    .bind(inspection_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?;
    row.map(map_finding).transpose()
}

async fn load_liability_by_finding_pg(
    connection: &mut PgConnection,
    tenant: &str,
    finding_id: &str,
    for_update: bool,
) -> Result<Option<LiabilityDecisionProjection>, RepositoryError> {
    let sql = if for_update {
        "SELECT id,finding_id,decision,apportioned_amount_minor,currency,status,version          FROM rental_liability_decisions WHERE tenant_id=$1 AND finding_id=$2 FOR UPDATE"
    } else {
        "SELECT id,finding_id,decision,apportioned_amount_minor,currency,status,version          FROM rental_liability_decisions WHERE tenant_id=$1 AND finding_id=$2"
    };
    let row = sqlx::query(sql)
        .bind(tenant)
        .bind(finding_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(pg_error)?;
    row.map(map_liability).transpose()
}

async fn load_liability_pg(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<LiabilityDecisionProjection, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,finding_id,decision,apportioned_amount_minor,currency,status,version          FROM rental_liability_decisions WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| contract("liability decision not found in the active tenant"))?;
    map_liability(row)
}

async fn load_repair_by_finding_pg(
    connection: &mut PgConnection,
    tenant: &str,
    finding_id: &str,
    for_update: bool,
) -> Result<Option<RepairCaseProjection>, RepositoryError> {
    let sql = if for_update {
        "SELECT id,finding_id,device_serial_no,decision,status,version          FROM rental_repair_cases WHERE tenant_id=$1 AND finding_id=$2 FOR UPDATE"
    } else {
        "SELECT id,finding_id,device_serial_no,decision,status,version          FROM rental_repair_cases WHERE tenant_id=$1 AND finding_id=$2"
    };
    let row = sqlx::query(sql)
        .bind(tenant)
        .bind(finding_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(pg_error)?;
    row.map(map_repair).transpose()
}

async fn load_repair_for_update_pg(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<RepairCaseProjection, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,finding_id,device_serial_no,decision,status,version          FROM rental_repair_cases WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| contract("repair case not found in the active tenant"))?;
    map_repair(row)
}

async fn load_repair_pg(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<RepairCaseProjection, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,finding_id,device_serial_no,decision,status,version          FROM rental_repair_cases WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| contract("repair case not found in the active tenant"))?;
    map_repair(row)
}

async fn load_settlement_by_idempotency_pg(
    connection: &mut PgConnection,
    tenant: &str,
    idempotency_key: &str,
) -> Result<Option<SettlementCaseProjection>, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,order_id,currency,total_amount_minor,deposit_deducted_minor,status,facts_hash          FROM rental_settlement_cases WHERE tenant_id=$1 AND command_idempotency_key=$2",
    )
    .bind(tenant)
    .bind(idempotency_key)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?;
    row.map(map_settlement).transpose()
}

async fn load_settlement_by_order_pg(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: &str,
) -> Result<Option<SettlementCaseProjection>, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,order_id,currency,total_amount_minor,deposit_deducted_minor,status,facts_hash          FROM rental_settlement_cases WHERE tenant_id=$1 AND order_id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?;
    row.map(map_settlement).transpose()
}

async fn load_settlement_pg(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<SettlementCaseProjection, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,order_id,currency,total_amount_minor,deposit_deducted_minor,status,facts_hash          FROM rental_settlement_cases WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| contract("settlement case not found in the active tenant"))?;
    map_settlement(row)
}

async fn load_settlement_for_update_pg(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<SettlementCaseProjection, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,order_id,currency,total_amount_minor,deposit_deducted_minor,status,facts_hash          FROM rental_settlement_cases WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| contract("settlement case not found in the active tenant"))?;
    map_settlement(row)
}

async fn load_effect_by_idempotency_pg(
    connection: &mut PgConnection,
    tenant: &str,
    settlement_case_id: &str,
    idempotency_key: &str,
) -> Result<Option<SettlementEffectAdmissionProjection>, RepositoryError> {
    let row = sqlx::query(
        "SELECT e.id,e.settlement_case_id,e.effect_kind,e.amount_minor,e.currency,                 e.external_operation_id,o.state AS external_operation_state          FROM rental_settlement_effect_admissions e          LEFT JOIN external_operations o            ON o.tenant_id=e.tenant_id AND o.id=e.external_operation_id          WHERE e.tenant_id=$1 AND e.settlement_case_id=$2 AND e.idempotency_key=$3",
    )
    .bind(tenant)
    .bind(settlement_case_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?;
    row.map(map_effect).transpose()
}

async fn load_effect_pg(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<SettlementEffectAdmissionProjection, RepositoryError> {
    let row = sqlx::query(
        "SELECT e.id,e.settlement_case_id,e.effect_kind,e.amount_minor,e.currency,                 e.external_operation_id,o.state AS external_operation_state          FROM rental_settlement_effect_admissions e          LEFT JOIN external_operations o            ON o.tenant_id=e.tenant_id AND o.id=e.external_operation_id          WHERE e.tenant_id=$1 AND e.id=$2",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| contract("settlement effect admission not found in the active tenant"))?;
    map_effect(row)
}

async fn load_dispute_by_idempotency_pg(
    connection: &mut PgConnection,
    tenant: &str,
    case_id: &str,
    idempotency_key: &str,
) -> Result<Option<DisputeProjection>, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,settlement_case_id,status FROM rental_disputes          WHERE tenant_id=$1 AND settlement_case_id=$2 AND command_idempotency_key=$3",
    )
    .bind(tenant)
    .bind(case_id)
    .bind(idempotency_key)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?;
    row.map(map_dispute).transpose()
}

async fn load_dispute_for_update_pg(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<DisputeProjection, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,settlement_case_id,status FROM rental_disputes          WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| contract("dispute not found in the active tenant"))?;
    map_dispute(row)
}

async fn load_settlement_findings_pg(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: &str,
) -> Result<Vec<FindingDecisionRow>, RepositoryError> {
    let rows = sqlx::query(
        "SELECT f.id AS finding_id,l.id AS decision_id,l.decision,                 l.apportioned_amount_minor,l.currency,l.status          FROM rental_damage_findings f          LEFT JOIN rental_liability_decisions l            ON l.tenant_id=f.tenant_id AND l.finding_id=f.id          WHERE f.tenant_id=$1 AND f.order_id=$2 AND f.status='open'          ORDER BY f.id",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(pg_error)?;

    rows.into_iter()
        .map(|row| {
            Ok(FindingDecisionRow {
                finding_id: row.try_get("finding_id").map_err(pg_error)?,
                decision_id: row
                    .try_get::<Option<String>, _>("decision_id")
                    .map_err(pg_error)?
                    .unwrap_or_default(),
                decision: row
                    .try_get::<Option<String>, _>("decision")
                    .map_err(pg_error)?
                    .unwrap_or_default(),
                amount_minor: row
                    .try_get::<Option<i64>, _>("apportioned_amount_minor")
                    .map_err(pg_error)?
                    .unwrap_or_default(),
                currency: row
                    .try_get::<Option<String>, _>("currency")
                    .map_err(pg_error)?
                    .unwrap_or_default(),
                status: row
                    .try_get::<Option<String>, _>("status")
                    .map_err(pg_error)?
                    .unwrap_or_default(),
            })
        })
        .collect()
}

async fn active_dispute_count_pg(
    connection: &mut PgConnection,
    tenant: &str,
    case_id: &str,
) -> Result<i64, RepositoryError> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM rental_disputes          WHERE tenant_id=$1 AND settlement_case_id=$2            AND status IN ('open','under_review')",
    )
    .bind(tenant)
    .bind(case_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)
}

async fn deposit_available_pg(
    connection: &mut PgConnection,
    tenant: &str,
    deposit_id: &str,
) -> Result<i64, RepositoryError> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(CASE entry_type            WHEN 'received' THEN amount_minor            WHEN 'manual_adjustment' THEN amount_minor            WHEN 'deducted' THEN -amount_minor            WHEN 'refund_completed' THEN -amount_minor            ELSE 0 END),0)::bigint          FROM integration_deposit_ledger          WHERE tenant_id=$1 AND deposit_id=$2",
    )
    .bind(tenant)
    .bind(deposit_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)
}

async fn load_financial_effect_summary_pg(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: &str,
) -> Result<FinancialEffectSummaryPg, RepositoryError> {
    let rows = sqlx::query(
        "SELECT e.effect_kind,e.amount_minor,o.state          FROM rental_settlement_effect_admissions e          JOIN rental_settlement_cases s            ON s.tenant_id=e.tenant_id AND s.id=e.settlement_case_id          LEFT JOIN external_operations o            ON o.tenant_id=e.tenant_id AND o.id=e.external_operation_id          WHERE e.tenant_id=$1 AND s.order_id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(pg_error)?;

    let mut summary = FinancialEffectSummaryPg::default();
    for row in rows {
        let effect_kind: String = row.try_get("effect_kind").map_err(pg_error)?;
        let amount_minor: i64 = row.try_get("amount_minor").map_err(pg_error)?;
        let operation_state: Option<String> = row.try_get("state").map_err(pg_error)?;

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

async fn assert_financial_reservation_pg(
    connection: &mut PgConnection,
    tenant: &str,
    case: &SettlementCaseProjection,
    proposed_deposit_minor: i64,
    proposed_charge_minor: i64,
) -> Result<(), RepositoryError> {
    let reserved = load_financial_effect_summary_pg(connection, tenant, &case.order_id)
        .await?
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

async fn assert_order_settlement_unsealed_pg(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: &str,
) -> Result<(), RepositoryError> {
    let terminal = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM rental_settlement_cases          WHERE tenant_id=$1 AND order_id=$2 AND status='settled'",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;
    if terminal == 0 {
        Ok(())
    } else {
        Err(contract(
            "terminal R3 settlement is sealed; amend or reopen requires an approved future authority",
        ))
    }
}

async fn assert_device_for_tenant_pg(
    connection: &mut PgConnection,
    tenant: &str,
    serial: &str,
) -> Result<(), RepositoryError> {
    let present = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM devices WHERE tenant_id=$1 AND serialno=$2)",
    )
    .bind(tenant)
    .bind(serial)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;
    if present {
        Ok(())
    } else {
        Err(contract(
            "device inventory record does not belong to the active tenant",
        ))
    }
}

async fn apply_device_inventory_state_pg(
    connection: &mut PgConnection,
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

    let changed =
        sqlx::query("UPDATE devices SET rentalstatus=$1 WHERE tenant_id=$2 AND serialno=$3")
            .bind(device_status)
            .bind(tenant)
            .bind(serial)
            .execute(&mut *connection)
            .await
            .map_err(pg_error)?
            .rows_affected();

    if changed == 1 {
        Ok(())
    } else {
        Err(contract(
            "device inventory record does not belong to the active tenant",
        ))
    }
}

async fn closure_facts_pg(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: &str,
) -> Result<R3ClosureFacts, RepositoryError> {
    closure_facts_inner_pg(connection, tenant, order_id)
        .await
        .map_err(pg_error)
}

async fn closure_facts_pg_sqlx(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: &str,
) -> Result<R3ClosureFacts, sqlx::Error> {
    closure_facts_inner_pg(connection, tenant, order_id).await
}

async fn closure_facts_inner_pg(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: &str,
) -> Result<R3ClosureFacts, sqlx::Error> {
    let return_received = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM rental_returns          WHERE tenant_id=$1 AND order_id=$2 AND status='received'",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await?;

    let counts = sqlx::query(
        "SELECT COUNT(*)::bigint AS total,                 COALESCE(SUM(CASE WHEN i.status IN ('passed','failed') THEN 1 ELSE 0 END),0)::bigint AS terminal          FROM rental_inspections i          JOIN rental_return_items ri            ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id          JOIN rental_returns r            ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id          WHERE i.tenant_id=$1 AND r.order_id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await?;
    let total: i64 = counts.try_get("total")?;
    let terminal: i64 = counts.try_get("terminal")?;

    let unresolved_damage_findings = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM rental_damage_findings          WHERE tenant_id=$1 AND order_id=$2 AND status='open'            AND id NOT IN (SELECT finding_id FROM rental_liability_decisions                           WHERE tenant_id=$1 AND status='decided')",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await?;

    let unresolved_liability_decisions = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint          FROM rental_liability_decisions l          JOIN rental_damage_findings f            ON f.tenant_id=l.tenant_id AND f.id=l.finding_id          WHERE l.tenant_id=$1 AND f.order_id=$2 AND l.status<>'decided'",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await?;

    let unresolved_repair_cases = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint          FROM rental_damage_findings f          LEFT JOIN rental_repair_cases r            ON r.tenant_id=f.tenant_id AND r.finding_id=f.id          WHERE f.tenant_id=$1 AND f.order_id=$2            AND f.condition_code IN ('damaged','missing') AND f.status='open'            AND (r.id IS NULL OR r.status IN ('repair_required','under_repair'))",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await?;

    let active_disputes = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint          FROM rental_disputes d          JOIN rental_settlement_cases s            ON s.tenant_id=d.tenant_id AND s.id=d.settlement_case_id          WHERE d.tenant_id=$1 AND s.order_id=$2            AND d.status IN ('open','under_review')",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await?;

    let overdue_open = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM overdue_records          WHERE tenant_id=$1 AND order_id=$2            AND status IN ('active','escalated_d1','escalated_d3','escalated_d7')",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await?;

    let settlement_terminal = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM rental_settlement_cases          WHERE tenant_id=$1 AND order_id=$2 AND status='settled'",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await?;

    let rows = sqlx::query(
        "SELECT e.effect_kind,e.amount_minor,o.state          FROM rental_settlement_effect_admissions e          JOIN rental_settlement_cases s            ON s.tenant_id=e.tenant_id AND s.id=e.settlement_case_id          LEFT JOIN external_operations o            ON o.tenant_id=e.tenant_id AND o.id=e.external_operation_id          WHERE e.tenant_id=$1 AND s.order_id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_all(&mut *connection)
    .await?;

    let mut financial = FinancialEffectSummaryPg::default();
    for row in rows {
        let effect_kind: String = row.try_get("effect_kind")?;
        let amount_minor: i64 = row.try_get("amount_minor")?;
        let operation_state: Option<String> = row.try_get("state")?;
        if effect_kind == "deposit_deduction" {
            financial.accounted_minor += amount_minor;
            financial.reserved_minor += amount_minor;
            continue;
        }
        match operation_state
            .as_deref()
            .and_then(OperationState::from_persisted)
            .map(|state| state.financial_effect_state())
            .unwrap_or(FinancialEffectState::Unresolved)
        {
            FinancialEffectState::Confirmed => {
                financial.accounted_minor += amount_minor;
                financial.reserved_minor += amount_minor;
            }
            FinancialEffectState::KnownNoEffect => {}
            FinancialEffectState::Unresolved => {
                financial.reserved_minor += amount_minor;
                financial.unresolved_effects += 1;
            }
        }
    }

    let settlement_total = sqlx::query_scalar::<_, i64>(
        "SELECT total_amount_minor FROM rental_settlement_cases          WHERE tenant_id=$1 AND order_id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_optional(&mut *connection)
    .await?;

    Ok(R3ClosureFacts {
        return_received: return_received == 1,
        inspection_complete: total > 0 && total == terminal,
        unresolved_damage_findings,
        unresolved_liability_decisions,
        unresolved_repair_cases,
        active_disputes,
        unresolved_effect_intents: financial.unresolved_effects,
        overdue_open: overdue_open > 0,
        settlement_terminal: settlement_terminal == 1,
        financial_balance_fully_accounted: settlement_total == Some(financial.accounted_minor),
    })
}

async fn emit_inspection_complete_if_ready_pg(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    let row = sqlx::query(
        "SELECT COUNT(*)::bigint AS total,                 COALESCE(SUM(CASE WHEN i.status IN ('passed','failed') THEN 1 ELSE 0 END),0)::bigint AS terminal          FROM rental_inspections i          JOIN rental_return_items ri            ON ri.tenant_id=i.tenant_id AND ri.id=i.return_item_id          JOIN rental_returns r            ON r.tenant_id=ri.tenant_id AND r.id=ri.return_id          WHERE i.tenant_id=$1 AND r.order_id=$2",
    )
    .bind(tenant)
    .bind(order_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;

    let total: i64 = row.try_get("total").map_err(pg_error)?;
    let terminal: i64 = row.try_get("terminal").map_err(pg_error)?;
    if total > 0 && total == terminal {
        append_outbox_pg(
            connection,
            tenant,
            "order",
            order_id,
            "InspectionComplete",
            &format!("inspection-complete:{order_id}"),
            &serde_json::json!({"orderId":order_id}),
            now,
        )
        .await?;
    }
    Ok(())
}

async fn append_outbox_pg(
    connection: &mut PgConnection,
    tenant: &str,
    source_kind: &str,
    source_id: &str,
    message_type: &str,
    idempotency_key: &str,
    payload: &serde_json::Value,
    now: &str,
) -> Result<(), RepositoryError> {
    sqlx::query(
        "INSERT INTO domain_outbox          (id,tenant_id,source_kind,source_id,message_type,idempotency_key,payload_json,           payload_version,state,available_at,created_at)          VALUES ($1,$2,$3,$4,$5,$6,$7,1,'pending',$8,$8)          ON CONFLICT (tenant_id,idempotency_key) DO NOTHING",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(tenant)
    .bind(source_kind)
    .bind(source_id)
    .bind(message_type)
    .bind(idempotency_key)
    .bind(payload.to_string())
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(pg_error)?;
    Ok(())
}

fn map_finding(row: sqlx::postgres::PgRow) -> Result<DamageFindingProjection, RepositoryError> {
    Ok(DamageFindingProjection {
        id: row.try_get("id").map_err(pg_error)?,
        order_id: row.try_get("order_id").map_err(pg_error)?,
        inspection_id: row.try_get("inspection_id").map_err(pg_error)?,
        device_serial_no: row.try_get("device_serial_no").map_err(pg_error)?,
        condition_code: row.try_get("condition_code").map_err(pg_error)?,
        status: row.try_get("status").map_err(pg_error)?,
    })
}

fn map_liability(
    row: sqlx::postgres::PgRow,
) -> Result<LiabilityDecisionProjection, RepositoryError> {
    Ok(LiabilityDecisionProjection {
        id: row.try_get("id").map_err(pg_error)?,
        finding_id: row.try_get("finding_id").map_err(pg_error)?,
        decision: row.try_get("decision").map_err(pg_error)?,
        apportioned_amount_minor: row.try_get("apportioned_amount_minor").map_err(pg_error)?,
        currency: row.try_get("currency").map_err(pg_error)?,
        status: row.try_get("status").map_err(pg_error)?,
        version: row.try_get("version").map_err(pg_error)?,
    })
}

fn map_repair(row: sqlx::postgres::PgRow) -> Result<RepairCaseProjection, RepositoryError> {
    Ok(RepairCaseProjection {
        id: row.try_get("id").map_err(pg_error)?,
        finding_id: row.try_get("finding_id").map_err(pg_error)?,
        device_serial_no: row.try_get("device_serial_no").map_err(pg_error)?,
        decision: row.try_get("decision").map_err(pg_error)?,
        status: row.try_get("status").map_err(pg_error)?,
        version: row.try_get("version").map_err(pg_error)?,
    })
}

fn map_settlement(row: sqlx::postgres::PgRow) -> Result<SettlementCaseProjection, RepositoryError> {
    Ok(SettlementCaseProjection {
        id: row.try_get("id").map_err(pg_error)?,
        order_id: row.try_get("order_id").map_err(pg_error)?,
        currency: row.try_get("currency").map_err(pg_error)?,
        total_amount_minor: row.try_get("total_amount_minor").map_err(pg_error)?,
        deposit_deducted_minor: row.try_get("deposit_deducted_minor").map_err(pg_error)?,
        status: row.try_get("status").map_err(pg_error)?,
        facts_hash: row.try_get("facts_hash").map_err(pg_error)?,
    })
}

fn map_effect(
    row: sqlx::postgres::PgRow,
) -> Result<SettlementEffectAdmissionProjection, RepositoryError> {
    Ok(SettlementEffectAdmissionProjection {
        id: row.try_get("id").map_err(pg_error)?,
        settlement_case_id: row.try_get("settlement_case_id").map_err(pg_error)?,
        effect_kind: row.try_get("effect_kind").map_err(pg_error)?,
        amount_minor: row.try_get("amount_minor").map_err(pg_error)?,
        currency: row.try_get("currency").map_err(pg_error)?,
        external_operation_id: row.try_get("external_operation_id").map_err(pg_error)?,
        external_operation_state: row.try_get("external_operation_state").map_err(pg_error)?,
    })
}

fn map_dispute(row: sqlx::postgres::PgRow) -> Result<DisputeProjection, RepositoryError> {
    Ok(DisputeProjection {
        id: row.try_get("id").map_err(pg_error)?,
        settlement_case_id: row.try_get("settlement_case_id").map_err(pg_error)?,
        status: row.try_get("status").map_err(pg_error)?,
    })
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn format_utc(value: chrono::DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn pg_error(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}
