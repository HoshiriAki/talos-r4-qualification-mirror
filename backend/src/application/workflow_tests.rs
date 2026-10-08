use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, Namespace,
    NoopHttpClient, PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision,
    SimulationId, TenantId, TenantMembershipId, TenantRole, TenantScope,
};

use crate::application::{
    DurableRentalProcessManager, FixedClock, RentalEffectOutcome, RentalWorkflowEffects,
    UnavailableProductionRentalEffects,
};
use crate::repositories::{
    RENTAL_DEFINITION_HASH, RENTAL_DEFINITION_ID, RENTAL_DEFINITION_VERSION, RENTAL_STEPS,
    RepositoryProvider, SqliteRepositoryProvider, WorkflowStepState,
};

struct FixtureEffects(RentalEffectOutcome);
impl RentalWorkflowEffects for FixtureEffects {
    fn execute(
        &self,
        _step_key: &str,
        _order_id: &str,
        _idempotency_key: &str,
        _ctx: &ExecutionContext,
    ) -> RentalEffectOutcome {
        self.0.clone()
    }
}

fn pool() -> Pool<SqliteConnectionManager> {
    let pool = Pool::builder()
        .max_size(1)
        .build(SqliteConnectionManager::memory())
        .unwrap();
    pool.get()
        .unwrap()
        .execute_batch(include_str!(
            "../db/migrations/057_durable_rental_workflow.sql"
        ))
        .unwrap();
    pool
}

fn tenant_context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            format!("actor-{tenant}"),
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new(format!("membership-{tenant}")).unwrap(),
                tenant_id: tenant_id.clone(),
                role: TenantRole::Admin,
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("workflow-test").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn preview_context() -> ExecutionContext {
    let tenant = TenantId::new("tenant-a").unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "platform",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-member").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant.clone()),
        DataScope::production(tenant, Revision::new("preview").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("preview-session").unwrap()),
        RequestId::new("preview-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn simulation_context() -> ExecutionContext {
    let tenant = TenantId::new("tenant-a").unwrap();
    let simulation = SimulationId::new("simulation-a").unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "platform",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-member").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant.clone()),
        DataScope::new(
            tenant,
            Namespace::Simulation(simulation.clone()),
            Revision::new("simulation-base").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Simulation(simulation),
        RequestId::new("simulation-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[test]
fn durable_identity_steps_inbox_and_restart_are_persisted_and_idempotent() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let ctx = tenant_context("tenant-a", "start");
    let scoped = provider.bind(&ctx).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 8, 19, 8, 0, 0).unwrap();
    let first = scoped
        .workflows()
        .start_from_message("message-1", "order-1", now)
        .unwrap();
    let duplicate = scoped
        .workflows()
        .start_from_message("message-1", "order-1", now)
        .unwrap();
    let second_message = scoped
        .workflows()
        .start_from_message("message-2", "order-1", now)
        .unwrap();
    assert_eq!(first.id, duplicate.id);
    assert_eq!(first.id, second_message.id);
    assert_eq!(first.definition_id, RENTAL_DEFINITION_ID);
    assert_eq!(first.definition_version, RENTAL_DEFINITION_VERSION);
    assert_eq!(first.definition_hash, RENTAL_DEFINITION_HASH);
    assert_eq!(
        scoped.workflows().steps(&first.id).unwrap().len(),
        RENTAL_STEPS.len()
    );

    drop(scoped);
    let reconstructed = SqliteRepositoryProvider::new(pool)
        .bind(&tenant_context("tenant-a", "restart"))
        .unwrap();
    assert_eq!(
        reconstructed.workflows().get(&first.id).unwrap().unwrap(),
        first
    );
    assert_eq!(
        reconstructed.workflows().steps(&first.id).unwrap()[0].step_key,
        "order_confirmed"
    );
}

#[test]
fn claim_is_exclusive_retry_resumes_and_illegal_transitions_fail_closed() {
    let provider = SqliteRepositoryProvider::new(pool());
    let scoped = provider.bind(&tenant_context("tenant-a", "claim")).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 8, 19, 8, 0, 0).unwrap();
    scoped
        .workflows()
        .start_from_message("message-claim", "order-claim", now)
        .unwrap();
    let claimed = scoped
        .workflows()
        .claim_due("worker-a", now, 60)
        .unwrap()
        .unwrap();
    assert_eq!(claimed.step.state, WorkflowStepState::Running);
    assert!(
        scoped
            .workflows()
            .claim_due("worker-b", now, 60)
            .unwrap()
            .is_none()
    );
    scoped
        .workflows()
        .transition(
            &claimed.step.id,
            WorkflowStepState::RetryScheduled,
            now,
            Some(now + Duration::seconds(30)),
            Some("retry"),
        )
        .unwrap();
    assert!(
        scoped
            .workflows()
            .claim_due("worker-b", now + Duration::seconds(29), 60)
            .unwrap()
            .is_none()
    );
    let resumed = scoped
        .workflows()
        .claim_due("worker-b", now + Duration::seconds(30), 60)
        .unwrap()
        .unwrap();
    scoped
        .workflows()
        .transition(
            &resumed.step.id,
            WorkflowStepState::Succeeded,
            now + Duration::seconds(31),
            None,
            None,
        )
        .unwrap();
    let illegal = scoped
        .workflows()
        .transition(
            &resumed.step.id,
            WorkflowStepState::RetryScheduled,
            now + Duration::seconds(32),
            Some(now + Duration::seconds(60)),
            None,
        )
        .unwrap_err();
    assert!(illegal.to_string().contains("illegal workflow transition"));
}

#[test]
fn blocker_manual_tenant_and_execution_mode_boundaries_survive() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let now = Utc.with_ymd_and_hms(2026, 8, 19, 8, 0, 0).unwrap();
    let scoped_a = provider.bind(&tenant_context("tenant-a", "a")).unwrap();
    let instance = scoped_a
        .workflows()
        .start_from_message("message-a", "order-a", now)
        .unwrap();
    let blocker_id = scoped_a
        .workflows()
        .add_blocker(
            &instance.id,
            "shipment_delivered",
            "R2_UNAVAILABLE",
            "provider absent",
            now,
        )
        .unwrap();
    let manual_id = scoped_a
        .workflows()
        .add_manual_task(
            &instance.id,
            "risk_cases_resolved",
            "REVIEW",
            "operator decision",
            now,
        )
        .unwrap();
    assert!(
        provider
            .bind(&tenant_context("tenant-b", "b"))
            .unwrap()
            .workflows()
            .get(&instance.id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        provider
            .bind(&preview_context())
            .unwrap()
            .workflows()
            .start_from_message("preview", "order", now)
            .unwrap_err()
            .code(),
        "REPOSITORY_PREVIEW_WRITE_DENIED"
    );
    let simulation_error = match provider.bind(&simulation_context()) {
        Ok(_) => panic!("simulation binding must fail closed"),
        Err(error) => error,
    };
    assert_eq!(simulation_error.code(), "REPOSITORY_SIMULATION_UNSUPPORTED");
    let connection = pool.get().unwrap();
    let blockers: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM workflow_blockers WHERE tenant_id='tenant-a' AND status='open'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let manual: i64 = connection.query_row("SELECT COUNT(*) FROM manual_decision_tasks WHERE tenant_id='tenant-a' AND status='open'", [], |row| row.get(0)).unwrap();
    assert_eq!((blockers, manual), (1, 1));
    drop(connection);
    scoped_a
        .workflows()
        .resolve_blocker(&blocker_id, now)
        .unwrap();
    scoped_a
        .workflows()
        .resolve_manual_task(&manual_id, "approved", now)
        .unwrap();
    let connection = pool.get().unwrap();
    let resolved: i64 = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM workflow_blockers WHERE status='resolved') + (SELECT COUNT(*) FROM manual_decision_tasks WHERE status='resolved' AND decision='approved')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(resolved, 2);
}

fn effect_case(outcome: RentalEffectOutcome) -> WorkflowStepState {
    let pool = pool();
    let provider: Arc<dyn RepositoryProvider> =
        Arc::new(SqliteRepositoryProvider::new(pool.clone()));
    let ctx = tenant_context("tenant-a", "effect");
    let now = Utc.with_ymd_and_hms(2026, 8, 19, 8, 0, 0).unwrap();
    let instance = provider
        .bind(&ctx)
        .unwrap()
        .workflows()
        .start_from_message("effect-message", "effect-order", now)
        .unwrap();
    pool.get().unwrap().execute(
        "UPDATE workflow_steps SET state='succeeded' WHERE tenant_id='tenant-a' AND workflow_instance_id=?1 AND sequence_no<5",
        [&instance.id],
    ).unwrap();
    let manager = DurableRentalProcessManager::new(
        provider,
        Arc::new(FixedClock::new(now)),
        Arc::new(FixtureEffects(outcome)),
    );
    manager
        .run_one(&ctx, "fixture-worker")
        .unwrap()
        .unwrap()
        .final_state
}

#[test]
fn deterministic_effects_cover_success_retry_blocker_manual_and_compensation_without_network() {
    assert_eq!(
        effect_case(RentalEffectOutcome::Succeeded),
        WorkflowStepState::Succeeded
    );
    assert_eq!(
        effect_case(RentalEffectOutcome::Retryable("retry".into())),
        WorkflowStepState::RetryScheduled
    );
    assert_eq!(
        effect_case(RentalEffectOutcome::Blocked {
            code: "BLOCK".into(),
            detail: "blocked".into()
        }),
        WorkflowStepState::Blocked
    );
    assert_eq!(
        effect_case(RentalEffectOutcome::ManualReview {
            code: "MANUAL".into(),
            detail: "manual".into()
        }),
        WorkflowStepState::ManualReview
    );
    assert_eq!(
        effect_case(RentalEffectOutcome::CompensationRequired(
            "compensate".into()
        )),
        WorkflowStepState::Compensating
    );
}

#[test]
fn production_future_capability_is_visibly_blocked() {
    let pool = pool();
    let provider: Arc<dyn RepositoryProvider> =
        Arc::new(SqliteRepositoryProvider::new(pool.clone()));
    let ctx = tenant_context("tenant-a", "production-effect");
    let now = Utc.with_ymd_and_hms(2026, 8, 19, 8, 0, 0).unwrap();
    let instance = provider
        .bind(&ctx)
        .unwrap()
        .workflows()
        .start_from_message("production-message", "production-order", now)
        .unwrap();
    pool.get().unwrap().execute("UPDATE workflow_steps SET state='succeeded' WHERE tenant_id='tenant-a' AND workflow_instance_id=?1 AND sequence_no<5", [&instance.id]).unwrap();
    let manager = DurableRentalProcessManager::new(
        provider,
        Arc::new(FixedClock::new(now)),
        Arc::new(UnavailableProductionRentalEffects),
    );
    let run = manager.run_one(&ctx, "production-worker").unwrap().unwrap();
    assert_eq!(run.step_key, "shipment_create_requested");
    assert_eq!(run.final_state, WorkflowStepState::Blocked);
}
