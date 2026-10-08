use std::sync::Arc;

use chrono::Duration;
use system_core::ExecutionContext;

use crate::repositories::{
    ClaimedWorkflowStep, RENTAL_DEFINITION_HASH, RENTAL_DEFINITION_ID, RENTAL_DEFINITION_VERSION,
    RepositoryProvider, WorkflowStepState,
};

use super::Clock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RentalEffectOutcome {
    Succeeded,
    Retryable(String),
    Blocked { code: String, detail: String },
    ManualReview { code: String, detail: String },
    CompensationRequired(String),
}

pub trait RentalWorkflowEffects: Send + Sync {
    fn execute(
        &self,
        step_key: &str,
        order_id: &str,
        idempotency_key: &str,
        ctx: &ExecutionContext,
    ) -> RentalEffectOutcome;
}

pub struct UnavailableProductionRentalEffects;

impl RentalWorkflowEffects for UnavailableProductionRentalEffects {
    fn execute(
        &self,
        step_key: &str,
        _order_id: &str,
        _idempotency_key: &str,
        _ctx: &ExecutionContext,
    ) -> RentalEffectOutcome {
        RentalEffectOutcome::Blocked {
            code: "DOWNSTREAM_AUTHORITY_UNAVAILABLE".into(),
            detail: format!(
                "{step_key} is owned by R1-P7 or R2 and is not available in production"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RentalWorkflowRun {
    pub workflow_id: String,
    pub step_key: String,
    pub final_state: WorkflowStepState,
}

pub struct DurableRentalProcessManager {
    repositories: Arc<dyn RepositoryProvider>,
    clock: Arc<dyn Clock>,
    effects: Arc<dyn RentalWorkflowEffects>,
}

impl DurableRentalProcessManager {
    pub fn new(
        repositories: Arc<dyn RepositoryProvider>,
        clock: Arc<dyn Clock>,
        effects: Arc<dyn RentalWorkflowEffects>,
    ) -> Self {
        Self {
            repositories,
            clock,
            effects,
        }
    }

    pub fn run_batch(
        &self,
        ctx: &ExecutionContext,
        worker_id: &str,
        limit: usize,
    ) -> Vec<Result<RentalWorkflowRun, String>> {
        (0..limit.clamp(1, 100))
            .map_while(|_| match self.run_one(ctx, worker_id) {
                Ok(Some(run)) => Some(Ok(run)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    pub fn run_one(
        &self,
        ctx: &ExecutionContext,
        worker_id: &str,
    ) -> Result<Option<RentalWorkflowRun>, String> {
        let scoped = self
            .repositories
            .bind(ctx)
            .map_err(|error| error.to_string())?;
        let now = self.clock.now_utc();
        let Some(claimed) = scoped
            .workflows()
            .claim_due(worker_id, now, 60)
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        self.verify_definition(&claimed)?;

        let outcome = match claimed.step.step_key.as_str() {
            "order_confirmed" => match scoped
                .lifecycles()
                .get(&claimed.instance.source_id)
                .map_err(|error| error.to_string())?
            {
                Some(lifecycle) if lifecycle.commercial_status == "confirmed" => {
                    RentalEffectOutcome::Succeeded
                }
                _ => RentalEffectOutcome::Blocked {
                    code: "ORDER_NOT_CONFIRMED".into(),
                    detail: "canonical R1-P5 lifecycle is not confirmed".into(),
                },
            },
            "contract_required" | "payment_required" => RentalEffectOutcome::Succeeded,
            "reservation_confirmed" => match scoped
                .reservations()
                .find_by_order(&claimed.instance.source_id)
                .map_err(|error| error.to_string())?
            {
                Some(reservation) if reservation.status == "confirmed" => {
                    RentalEffectOutcome::Succeeded
                }
                _ => RentalEffectOutcome::Blocked {
                    code: "RESERVATION_NOT_CONFIRMED".into(),
                    detail: "canonical R1-P4 reservation is missing or incomplete".into(),
                },
            },
            "allocation_complete" => {
                if scoped
                    .reservations()
                    .allocation_complete_for_order(&claimed.instance.source_id)
                    .map_err(|error| error.to_string())?
                {
                    RentalEffectOutcome::Succeeded
                } else {
                    RentalEffectOutcome::Blocked {
                        code: "ALLOCATION_INCOMPLETE".into(),
                        detail: "server-owned R1-P4 allocation completeness is false".into(),
                    }
                }
            }
            "return_received" => match scoped.rental_closure().facts(&claimed.instance.source_id).map_err(|error| error.to_string())? {
                facts if facts.return_received => RentalEffectOutcome::Succeeded,
                _ => RentalEffectOutcome::Blocked { code: "RETURN_INCOMPLETE".into(), detail: "canonical R1-P7 Return is not complete".into() },
            },
            "inspection_complete" => match scoped.rental_closure().facts(&claimed.instance.source_id).map_err(|error| error.to_string())? {
                facts if facts.inspection_complete => RentalEffectOutcome::Succeeded,
                _ => RentalEffectOutcome::Blocked { code: "INSPECTION_INCOMPLETE".into(), detail: "per-allocation R1-P7 inspections are not terminal".into() },
            },
            "risk_cases_resolved" => match scoped.rental_closure().facts(&claimed.instance.source_id).map_err(|error| error.to_string())? {
                facts if facts.inspection_complete && facts.open_damage_reviews == 0 => RentalEffectOutcome::Succeeded,
                _ => RentalEffectOutcome::ManualReview { code: "DAMAGE_REVIEW_REQUIRED".into(), detail: "an R1-P7 damage review placeholder remains open".into() },
            },
            "settlement_complete" => match scoped.rental_closure().facts(&claimed.instance.source_id).map_err(|error| error.to_string())? {
                facts if facts.settlement_terminal => RentalEffectOutcome::Succeeded,
                _ => RentalEffectOutcome::Blocked { code: "SETTLEMENT_NOT_TERMINAL".into(), detail: "real financial authority is unavailable or settlement is incomplete".into() },
            },
            "close_order" => RentalEffectOutcome::Blocked { code: "CLOSE_AUTHORITIES_NOT_AVAILABLE".into(), detail: "deposit, shipment, invoice, overdue, damage/repair and audit reconciliation authorities are not all available in R1; close_order remains fail closed".into() },
            step => self.effects.execute(
                step,
                &claimed.instance.source_id,
                &format!("workflow:{}:{}", claimed.instance.id, claimed.step.step_key),
                ctx,
            ),
        };

        let final_state = match outcome {
            RentalEffectOutcome::Succeeded => WorkflowStepState::Succeeded,
            RentalEffectOutcome::Retryable(detail) => {
                scoped
                    .workflows()
                    .transition(
                        &claimed.step.id,
                        WorkflowStepState::RetryScheduled,
                        now,
                        Some(now + Duration::seconds(60)),
                        Some(&detail),
                    )
                    .map_err(|error| error.to_string())?;
                return Ok(Some(run(&claimed, WorkflowStepState::RetryScheduled)));
            }
            RentalEffectOutcome::Blocked { code, detail } => {
                scoped
                    .workflows()
                    .add_blocker(
                        &claimed.instance.id,
                        &claimed.step.step_key,
                        &code,
                        &detail,
                        now,
                    )
                    .map_err(|error| error.to_string())?;
                WorkflowStepState::Blocked
            }
            RentalEffectOutcome::ManualReview { code, detail } => {
                scoped
                    .workflows()
                    .add_manual_task(
                        &claimed.instance.id,
                        &claimed.step.step_key,
                        &code,
                        &detail,
                        now,
                    )
                    .map_err(|error| error.to_string())?;
                WorkflowStepState::ManualReview
            }
            RentalEffectOutcome::CompensationRequired(detail) => {
                scoped
                    .workflows()
                    .transition(
                        &claimed.step.id,
                        WorkflowStepState::Compensating,
                        now,
                        None,
                        Some(&detail),
                    )
                    .map_err(|error| error.to_string())?;
                return Ok(Some(run(&claimed, WorkflowStepState::Compensating)));
            }
        };
        scoped
            .workflows()
            .transition(&claimed.step.id, final_state, now, None, None)
            .map_err(|error| error.to_string())?;
        Ok(Some(run(&claimed, final_state)))
    }

    fn verify_definition(&self, claimed: &ClaimedWorkflowStep) -> Result<(), String> {
        if claimed.instance.definition_id != RENTAL_DEFINITION_ID
            || claimed.instance.definition_version != RENTAL_DEFINITION_VERSION
            || claimed.instance.definition_hash != RENTAL_DEFINITION_HASH
        {
            return Err(
                "WORKFLOW_DEFINITION_MISMATCH: explicit instance migration required".into(),
            );
        }
        Ok(())
    }
}

fn run(claimed: &ClaimedWorkflowStep, final_state: WorkflowStepState) -> RentalWorkflowRun {
    RentalWorkflowRun {
        workflow_id: claimed.instance.id.clone(),
        step_key: claimed.step.step_key.clone(),
        final_state,
    }
}
