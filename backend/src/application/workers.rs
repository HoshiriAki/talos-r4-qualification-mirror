use std::sync::Arc;
use std::time::Duration;

use crate::repositories::{AuthSecurityRepository, MaintenanceCompatibilityRepository};
use chrono::{DateTime, Utc};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_core::transport::http_client::HttpClient;
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, ExecutionPlane, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::observability::{MetricsSink, NoopMetrics, WorkflowEvent, WorkflowOutcome};
use crate::repositories::{RepositoryProvider, WorkflowStepState, WorkflowWorkerTenantSource};

use super::{Clock, DurableRentalProcessManager};

pub const MAINTENANCE_INTERVAL_SECONDS: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkerJobId {
    SyncOrderStatuses,
    SeedOverdueTasks,
    CleanupExpiredSessions,
    ProcessDurableRentalWorkflows,
}

impl WorkerJobId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SyncOrderStatuses => "sync_order_statuses",
            Self::SeedOverdueTasks => "seed_overdue_tasks",
            Self::CleanupExpiredSessions => "cleanup_expired_sessions",
            Self::ProcessDurableRentalWorkflows => "process_durable_rental_workflows",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenanceEffect {
    OrderStatusesSynchronized {
        reserved: usize,
        activated: usize,
    },
    OverdueTasksSeeded {
        created: usize,
    },
    ExpiredSessionsCleaned,
    RentalWorkflowsProcessed {
        tenants: usize,
        steps: usize,
        failures: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerScope {
    Platform,
    Tenant(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerRunOutcome {
    Succeeded(MaintenanceEffect),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRunReport {
    pub job: WorkerJobId,
    pub scope: WorkerScope,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub outcome: WorkerRunOutcome,
    pub correlation_id: String,
}

#[derive(Clone)]
pub struct WorkerContextFactory {
    clock: Arc<dyn Clock>,
    http_client: Arc<dyn HttpClient>,
}

impl WorkerContextFactory {
    pub fn new(clock: Arc<dyn Clock>, http_client: Arc<dyn HttpClient>) -> Self {
        Self { clock, http_client }
    }

    pub fn platform_context(&self, job: WorkerJobId) -> Result<ExecutionContext, String> {
        ExecutionContext::new(
            ActorIdentity::system(),
            TenantScope::platform(),
            DataScope::platform(Revision::new(format!("worker:{}:platform", job.as_str()))?),
            ExecutionMode::Normal,
            self.fresh_request_id(job)?,
            None,
            self.http_client.clone(),
        )
    }

    pub fn tenant_context(
        &self,
        job: WorkerJobId,
        tenant_id: TenantId,
    ) -> Result<ExecutionContext, String> {
        ExecutionContext::new(
            ActorIdentity::system(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(
                tenant_id,
                Revision::new(format!("worker:{}:tenant", job.as_str()))?,
            )?,
            ExecutionMode::Normal,
            self.fresh_request_id(job)?,
            None,
            self.http_client.clone(),
        )
    }

    fn fresh_request_id(&self, job: WorkerJobId) -> Result<RequestId, String> {
        RequestId::new(format!(
            "worker:{}:{}:{}",
            job.as_str(),
            self.clock.now_utc().timestamp_micros(),
            uuid::Uuid::new_v4()
        ))
    }

    fn now_utc(&self) -> DateTime<Utc> {
        self.clock.now_utc()
    }
}

pub trait MaintenancePort: Send + Sync {
    fn sync_order_statuses(&self, ctx: &ExecutionContext) -> Result<MaintenanceEffect, String>;

    fn seed_overdue_tasks(&self, ctx: &ExecutionContext) -> Result<MaintenanceEffect, String>;

    fn cleanup_expired_sessions(&self, ctx: &ExecutionContext)
    -> Result<MaintenanceEffect, String>;
}

pub trait RentalWorkflowWorkerPort: Send + Sync {
    fn process_due(&self) -> Result<MaintenanceEffect, String>;
}

struct NoopRentalWorkflowWorker;
impl RentalWorkflowWorkerPort for NoopRentalWorkflowWorker {
    fn process_due(&self) -> Result<MaintenanceEffect, String> {
        Ok(MaintenanceEffect::RentalWorkflowsProcessed {
            tenants: 0,
            steps: 0,
            failures: 0,
        })
    }
}

pub struct DurableRentalWorkflowWorker {
    tenant_source: WorkflowWorkerTenantSource,
    contexts: WorkerContextFactory,
    manager: Arc<DurableRentalProcessManager>,
    repositories: Arc<dyn RepositoryProvider>,
    clock: Arc<dyn Clock>,
    metrics: Arc<dyn MetricsSink>,
}

impl DurableRentalWorkflowWorker {
    pub fn new(
        pool: Pool<SqliteConnectionManager>,
        contexts: WorkerContextFactory,
        repositories: Arc<dyn RepositoryProvider>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self::new_with_metrics(pool, contexts, repositories, clock, Arc::new(NoopMetrics))
    }

    pub(crate) fn new_with_metrics(
        pool: Pool<SqliteConnectionManager>,
        contexts: WorkerContextFactory,
        repositories: Arc<dyn RepositoryProvider>,
        clock: Arc<dyn Clock>,
        metrics: Arc<dyn MetricsSink>,
    ) -> Self {
        Self::new_with_tenant_source_and_metrics(
            WorkflowWorkerTenantSource::new(pool),
            contexts,
            repositories,
            clock,
            metrics,
        )
    }

    pub(crate) fn new_with_tenant_source(
        tenant_source: WorkflowWorkerTenantSource,
        contexts: WorkerContextFactory,
        repositories: Arc<dyn RepositoryProvider>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self::new_with_tenant_source_and_metrics(
            tenant_source,
            contexts,
            repositories,
            clock,
            Arc::new(NoopMetrics),
        )
    }

    pub(crate) fn new_with_tenant_source_and_metrics(
        tenant_source: WorkflowWorkerTenantSource,
        contexts: WorkerContextFactory,
        repositories: Arc<dyn RepositoryProvider>,
        clock: Arc<dyn Clock>,
        metrics: Arc<dyn MetricsSink>,
    ) -> Self {
        let manager_repositories = repositories.clone();
        let manager_clock = clock.clone();
        Self {
            tenant_source,
            contexts,
            manager: Arc::new(DurableRentalProcessManager::new(
                manager_repositories,
                manager_clock,
                Arc::new(super::UnavailableProductionRentalEffects),
            )),
            repositories,
            clock,
            metrics,
        }
    }
}

impl RentalWorkflowWorkerPort for DurableRentalWorkflowWorker {
    fn process_due(&self) -> Result<MaintenanceEffect, String> {
        let tenants = self.tenant_source.discover_due_tenants(100)?;
        let mut steps = 0;
        let mut failures = 0;
        for raw in &tenants {
            let tenant_id = match TenantId::new(raw.clone()) {
                Ok(value) => value,
                Err(_) => {
                    failures += 1;
                    self.metrics
                        .workflow(WorkflowEvent::Blocked, WorkflowOutcome::Failed);
                    continue;
                }
            };
            let ctx = match self
                .contexts
                .tenant_context(WorkerJobId::ProcessDurableRentalWorkflows, tenant_id)
            {
                Ok(value) => value,
                Err(_) => {
                    failures += 1;
                    self.metrics
                        .workflow(WorkflowEvent::Blocked, WorkflowOutcome::Failed);
                    continue;
                }
            };
            match self.repositories.bind(&ctx).and_then(|scoped| {
                scoped
                    .workflows()
                    .consume_domain_events(self.clock.now_utc(), 100)
            }) {
                Ok(consumed) => {
                    steps += consumed;
                    for _ in 0..consumed {
                        self.metrics
                            .workflow(WorkflowEvent::InboxConsumed, WorkflowOutcome::Succeeded);
                        self.metrics
                            .workflow(WorkflowEvent::OutboxDelivered, WorkflowOutcome::Succeeded);
                    }
                }
                Err(_) => {
                    failures += 1;
                    self.metrics
                        .workflow(WorkflowEvent::Blocked, WorkflowOutcome::Failed);
                    continue;
                }
            }
            for result in self.manager.run_batch(&ctx, "rental-workflow-worker", 25) {
                match result {
                    Ok(run) => {
                        steps += 1;
                        self.metrics
                            .workflow(WorkflowEvent::StepProcessed, WorkflowOutcome::Succeeded);
                        let (event, outcome) = match run.final_state {
                            WorkflowStepState::Succeeded => {
                                (WorkflowEvent::StepCompleted, WorkflowOutcome::Succeeded)
                            }
                            WorkflowStepState::RetryScheduled => {
                                (WorkflowEvent::RetryScheduled, WorkflowOutcome::Retryable)
                            }
                            WorkflowStepState::Blocked => {
                                (WorkflowEvent::Blocked, WorkflowOutcome::Blocked)
                            }
                            WorkflowStepState::Compensating => (
                                WorkflowEvent::CompensationScheduled,
                                WorkflowOutcome::Blocked,
                            ),
                            WorkflowStepState::ManualReview => (
                                WorkflowEvent::ManualResolutionRequired,
                                WorkflowOutcome::Blocked,
                            ),
                            WorkflowStepState::Pending | WorkflowStepState::Running => {
                                (WorkflowEvent::Blocked, WorkflowOutcome::Failed)
                            }
                        };
                        self.metrics.workflow(event, outcome);
                    }
                    Err(_) => {
                        failures += 1;
                        self.metrics
                            .workflow(WorkflowEvent::Blocked, WorkflowOutcome::Failed);
                    }
                }
            }
        }
        Ok(MaintenanceEffect::RentalWorkflowsProcessed {
            tenants: tenants.len(),
            steps,
            failures,
        })
    }
}

pub struct LegacyMaintenanceAdapter {
    maintenance_repository: MaintenanceCompatibilityRepository,
    auth_security_repository: AuthSecurityRepository,
}

impl LegacyMaintenanceAdapter {
    pub fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            maintenance_repository: MaintenanceCompatibilityRepository::new(pool.clone()),
            auth_security_repository: AuthSecurityRepository::new(pool),
        }
    }

    pub(crate) fn new_with_auth_security_repository(
        pool: Pool<SqliteConnectionManager>,
        auth_security_repository: AuthSecurityRepository,
    ) -> Self {
        Self {
            maintenance_repository: MaintenanceCompatibilityRepository::new(pool),
            auth_security_repository,
        }
    }

    pub(crate) fn new_with_repositories(
        maintenance_repository: MaintenanceCompatibilityRepository,
        auth_security_repository: AuthSecurityRepository,
    ) -> Self {
        Self {
            maintenance_repository,
            auth_security_repository,
        }
    }

    fn validate_context(ctx: &ExecutionContext) -> Result<(), String> {
        let actor = ctx.actor();
        if actor.id().is_some()
            || actor.role().is_some()
            || actor.authority().is_some()
            || !ctx.tenant_scope().is_platform()
            || !ctx.data_scope().is_platform()
            || ctx.plane() != ExecutionPlane::PlatformControl
            || !matches!(ctx.execution_mode(), ExecutionMode::Normal)
        {
            return Err("WORKER_CONTEXT_INVALID: legacy maintenance requires normal system/platform context".into());
        }
        Ok(())
    }
}

impl MaintenancePort for LegacyMaintenanceAdapter {
    fn sync_order_statuses(&self, ctx: &ExecutionContext) -> Result<MaintenanceEffect, String> {
        Self::validate_context(ctx)?;
        let today = crate::utils::time::shanghai_now_date_key();
        let result = self
            .maintenance_repository
            .sync_order_statuses(&today)
            .map_err(|error| error.to_string())?;
        Ok(MaintenanceEffect::OrderStatusesSynchronized {
            reserved: result.reserved,
            activated: result.activated,
        })
    }

    fn seed_overdue_tasks(&self, ctx: &ExecutionContext) -> Result<MaintenanceEffect, String> {
        Self::validate_context(ctx)?;
        let today = crate::utils::time::shanghai_now_date_key();
        let now = crate::utils::time::shanghai_now_iso();
        let created = self
            .maintenance_repository
            .seed_overdue_tasks(&today, &now)
            .map_err(|error| error.to_string())?;
        Ok(MaintenanceEffect::OverdueTasksSeeded { created })
    }

    fn cleanup_expired_sessions(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<MaintenanceEffect, String> {
        Self::validate_context(ctx)?;
        crate::services::auth_service::cleanup_expired_sessions_with_repository(
            &self.auth_security_repository,
        )
        .map_err(|error| error.to_string())?;
        Ok(MaintenanceEffect::ExpiredSessionsCleaned)
    }
}

pub trait WorkerRunner: Send + Sync {
    fn run_startup(&self) -> Vec<WorkerRunReport>;
    fn run_periodic_tick(&self) -> Vec<WorkerRunReport>;
    fn spawn_periodic(self: Arc<Self>) -> tokio::task::JoinHandle<()>;
}

pub struct MaintenanceWorkerRunner {
    context_factory: WorkerContextFactory,
    maintenance: Arc<dyn MaintenancePort>,
    rental_workflow: Arc<dyn RentalWorkflowWorkerPort>,
}

impl MaintenanceWorkerRunner {
    pub fn new(
        context_factory: WorkerContextFactory,
        maintenance: Arc<dyn MaintenancePort>,
    ) -> Self {
        Self {
            context_factory,
            maintenance,
            rental_workflow: Arc::new(NoopRentalWorkflowWorker),
        }
    }

    pub fn with_rental_workflow(
        mut self,
        rental_workflow: Arc<dyn RentalWorkflowWorkerPort>,
    ) -> Self {
        self.rental_workflow = rental_workflow;
        self
    }

    fn run_job(&self, job: WorkerJobId) -> WorkerRunReport {
        let started_at = self.context_factory.now_utc();
        let context_failure_correlation = format!(
            "worker:{}:context-error:{}",
            job.as_str(),
            uuid::Uuid::new_v4()
        );
        let context = self.context_factory.platform_context(job);
        let (scope, correlation_id, outcome) = match context {
            Ok(ctx) => {
                let outcome = match job {
                    WorkerJobId::SyncOrderStatuses => self.maintenance.sync_order_statuses(&ctx),
                    WorkerJobId::SeedOverdueTasks => self.maintenance.seed_overdue_tasks(&ctx),
                    WorkerJobId::CleanupExpiredSessions => {
                        self.maintenance.cleanup_expired_sessions(&ctx)
                    }
                    WorkerJobId::ProcessDurableRentalWorkflows => {
                        self.rental_workflow.process_due()
                    }
                }
                .map(WorkerRunOutcome::Succeeded)
                .unwrap_or_else(WorkerRunOutcome::Failed);
                (
                    WorkerScope::Platform,
                    ctx.correlation_id().as_str().to_string(),
                    outcome,
                )
            }
            Err(error) => (
                WorkerScope::Platform,
                context_failure_correlation,
                WorkerRunOutcome::Failed(error),
            ),
        };
        let report = WorkerRunReport {
            job,
            scope,
            started_at,
            finished_at: self.context_factory.now_utc(),
            outcome,
            correlation_id,
        };
        match &report.outcome {
            WorkerRunOutcome::Succeeded(effect) => tracing::info!(
                job = report.job.as_str(),
                correlation_id = report.correlation_id,
                effect = ?effect,
                "maintenance worker completed"
            ),
            WorkerRunOutcome::Failed(_) => tracing::warn!(
                job = report.job.as_str(),
                correlation_id = report.correlation_id,
                error_class = "maintenance_job_failed",
                "maintenance worker failed"
            ),
        }
        report
    }
}

impl WorkerRunner for MaintenanceWorkerRunner {
    fn run_startup(&self) -> Vec<WorkerRunReport> {
        [
            WorkerJobId::SyncOrderStatuses,
            WorkerJobId::SeedOverdueTasks,
            WorkerJobId::ProcessDurableRentalWorkflows,
        ]
        .into_iter()
        .map(|job| self.run_job(job))
        .collect()
    }

    fn run_periodic_tick(&self) -> Vec<WorkerRunReport> {
        [
            WorkerJobId::CleanupExpiredSessions,
            WorkerJobId::SeedOverdueTasks,
            WorkerJobId::ProcessDurableRentalWorkflows,
        ]
        .into_iter()
        .map(|job| self.run_job(job))
        .collect()
    }

    fn spawn_periodic(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(MAINTENANCE_INTERVAL_SECONDS));
            loop {
                interval.tick().await;
                let runner = self.clone();
                if tokio::task::spawn_blocking(move || runner.run_periodic_tick())
                    .await
                    .is_err()
                {
                    tracing::error!(
                        error_class = "maintenance_blocking_task_failed",
                        "maintenance worker blocking task failed"
                    );
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use chrono::{TimeZone, Utc};
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, ExecutionPlane, NoopHttpClient,
        RequestId, Revision, TenantId, TenantScope,
    };

    use crate::application::FixedClock;

    use super::{
        LegacyMaintenanceAdapter, MAINTENANCE_INTERVAL_SECONDS, MaintenanceEffect, MaintenancePort,
        MaintenanceWorkerRunner, WorkerContextFactory, WorkerJobId, WorkerRunOutcome, WorkerRunner,
    };

    #[derive(Default)]
    struct RecordingMaintenancePort {
        calls: Mutex<Vec<(WorkerJobId, String)>>,
        failure: Mutex<Option<WorkerJobId>>,
    }

    impl RecordingMaintenancePort {
        fn record(
            &self,
            job: WorkerJobId,
            ctx: &system_core::ExecutionContext,
            effect: MaintenanceEffect,
        ) -> Result<MaintenanceEffect, String> {
            assert!(ctx.actor().id().is_none());
            assert!(ctx.actor().role().is_none());
            assert!(ctx.authority().is_none());
            assert!(ctx.tenant_scope().is_platform());
            assert!(ctx.data_scope().is_platform());
            assert_eq!(ctx.plane(), ExecutionPlane::PlatformControl);
            assert!(matches!(ctx.execution_mode(), ExecutionMode::Normal));
            self.calls
                .lock()
                .unwrap()
                .push((job, ctx.correlation_id().as_str().to_string()));
            if *self.failure.lock().unwrap() == Some(job) {
                Err(format!("forced failure: {}", job.as_str()))
            } else {
                Ok(effect)
            }
        }
    }

    impl MaintenancePort for RecordingMaintenancePort {
        fn sync_order_statuses(
            &self,
            ctx: &system_core::ExecutionContext,
        ) -> Result<MaintenanceEffect, String> {
            self.record(
                WorkerJobId::SyncOrderStatuses,
                ctx,
                MaintenanceEffect::OrderStatusesSynchronized {
                    reserved: 2,
                    activated: 3,
                },
            )
        }

        fn seed_overdue_tasks(
            &self,
            ctx: &system_core::ExecutionContext,
        ) -> Result<MaintenanceEffect, String> {
            self.record(
                WorkerJobId::SeedOverdueTasks,
                ctx,
                MaintenanceEffect::OverdueTasksSeeded { created: 5 },
            )
        }

        fn cleanup_expired_sessions(
            &self,
            ctx: &system_core::ExecutionContext,
        ) -> Result<MaintenanceEffect, String> {
            self.record(
                WorkerJobId::CleanupExpiredSessions,
                ctx,
                MaintenanceEffect::ExpiredSessionsCleaned,
            )
        }
    }

    fn fixture(
        port: Arc<RecordingMaintenancePort>,
    ) -> (MaintenanceWorkerRunner, WorkerContextFactory) {
        let instant = Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
        let clock = Arc::new(FixedClock::new(instant));
        let contexts = WorkerContextFactory::new(clock, Arc::new(NoopHttpClient));
        (
            MaintenanceWorkerRunner::new(contexts.clone(), port),
            contexts,
        )
    }

    fn untrusted_tenant_context() -> ExecutionContext {
        let tenant_id = TenantId::new("tenant-invalid-maintenance").unwrap();
        ExecutionContext::new(
            ActorIdentity::system(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("worker-test-production").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new("worker-test-tenant-context").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn context_factory_builds_fresh_platform_system_contexts() {
        let (_runner, factory) = fixture(Arc::new(RecordingMaintenancePort::default()));
        let first = factory
            .platform_context(WorkerJobId::SyncOrderStatuses)
            .unwrap();
        let second = factory
            .platform_context(WorkerJobId::SyncOrderStatuses)
            .unwrap();
        assert!(first.actor().id().is_none());
        assert!(first.tenant_scope().is_platform());
        assert!(first.data_scope().is_platform());
        assert_eq!(first.plane(), ExecutionPlane::PlatformControl);
        assert!(matches!(first.execution_mode(), ExecutionMode::Normal));
        assert_ne!(first.correlation_id(), second.correlation_id());
    }

    #[test]
    fn startup_and_periodic_orders_are_deterministic_and_failures_do_not_skip() {
        let port = Arc::new(RecordingMaintenancePort::default());
        *port.failure.lock().unwrap() = Some(WorkerJobId::SyncOrderStatuses);
        let (runner, _) = fixture(port.clone());

        let startup = runner.run_startup();
        assert_eq!(
            startup.iter().map(|report| report.job).collect::<Vec<_>>(),
            vec![
                WorkerJobId::SyncOrderStatuses,
                WorkerJobId::SeedOverdueTasks,
                WorkerJobId::ProcessDurableRentalWorkflows,
            ]
        );
        assert!(matches!(startup[0].outcome, WorkerRunOutcome::Failed(_)));
        assert!(matches!(
            startup[1].outcome,
            WorkerRunOutcome::Succeeded(MaintenanceEffect::OverdueTasksSeeded { created: 5 })
        ));
        assert_eq!(startup[0].started_at, startup[0].finished_at);
        assert_ne!(startup[0].correlation_id, startup[1].correlation_id);

        *port.failure.lock().unwrap() = None;
        let periodic = runner.run_periodic_tick();
        assert_eq!(
            periodic.iter().map(|report| report.job).collect::<Vec<_>>(),
            vec![
                WorkerJobId::CleanupExpiredSessions,
                WorkerJobId::SeedOverdueTasks,
                WorkerJobId::ProcessDurableRentalWorkflows,
            ]
        );
    }

    #[test]
    fn legacy_adapter_rejects_non_platform_context_before_database_access() {
        let adapter =
            LegacyMaintenanceAdapter::new(Pool::new(SqliteConnectionManager::memory()).unwrap());
        assert!(
            adapter
                .cleanup_expired_sessions(&untrusted_tenant_context())
                .unwrap_err()
                .contains("WORKER_CONTEXT_INVALID")
        );
    }

    #[test]
    fn production_interval_is_exactly_five_minutes() {
        assert_eq!(MAINTENANCE_INTERVAL_SECONDS, 300);
    }
}
