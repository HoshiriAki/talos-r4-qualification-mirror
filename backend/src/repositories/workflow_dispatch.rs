use chrono::{DateTime, Utc};

use crate::repositories::workflow::{
    ClaimedWorkflowStep, ScopedWorkflowRepository as SqliteWorkflowRepository, WorkflowInstance,
    WorkflowStep, WorkflowStepState,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::workflow_postgres::PostgresWorkflowRepository;

pub struct ScopedWorkflowRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedWorkflowRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn start_from_message(
        &self,
        message_id: &str,
        order_id: &str,
        now: DateTime<Utc>,
    ) -> Result<WorkflowInstance, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkflowRepository::new(self.scoped.session())
                .start_from_message(message_id, order_id, now);
        }
        SqliteWorkflowRepository::new(self.scoped).start_from_message(message_id, order_id, now)
    }

    pub fn consume_domain_events(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkflowRepository::new(self.scoped.session())
                .consume_domain_events(now, limit);
        }
        SqliteWorkflowRepository::new(self.scoped).consume_domain_events(now, limit)
    }

    pub fn get(&self, id: &str) -> Result<Option<WorkflowInstance>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkflowRepository::new(self.scoped.session()).get(id);
        }
        SqliteWorkflowRepository::new(self.scoped).get(id)
    }

    pub fn order_has_open_blockers(&self, order_id: &str) -> Result<bool, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkflowRepository::new(self.scoped.session())
                .order_has_open_blockers(order_id);
        }
        SqliteWorkflowRepository::new(self.scoped).order_has_open_blockers(order_id)
    }

    pub fn steps(&self, instance_id: &str) -> Result<Vec<WorkflowStep>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkflowRepository::new(self.scoped.session()).steps(instance_id);
        }
        SqliteWorkflowRepository::new(self.scoped).steps(instance_id)
    }

    pub fn claim_due(
        &self,
        worker: &str,
        now: DateTime<Utc>,
        lease_seconds: i64,
    ) -> Result<Option<ClaimedWorkflowStep>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkflowRepository::new(self.scoped.session()).claim_due(
                worker,
                now,
                lease_seconds,
            );
        }
        SqliteWorkflowRepository::new(self.scoped).claim_due(worker, now, lease_seconds)
    }

    pub fn transition(
        &self,
        step_id: &str,
        next: WorkflowStepState,
        now: DateTime<Utc>,
        next_eligible_at: Option<DateTime<Utc>>,
        error: Option<&str>,
    ) -> Result<WorkflowStep, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkflowRepository::new(self.scoped.session()).transition(
                step_id,
                next,
                now,
                next_eligible_at,
                error,
            );
        }
        SqliteWorkflowRepository::new(self.scoped).transition(
            step_id,
            next,
            now,
            next_eligible_at,
            error,
        )
    }

    pub fn add_blocker(
        &self,
        instance_id: &str,
        step_key: &str,
        code: &str,
        detail: &str,
        now: DateTime<Utc>,
    ) -> Result<String, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkflowRepository::new(self.scoped.session()).add_blocker(
                instance_id,
                step_key,
                code,
                detail,
                now,
            );
        }
        SqliteWorkflowRepository::new(self.scoped).add_blocker(
            instance_id,
            step_key,
            code,
            detail,
            now,
        )
    }

    pub fn add_manual_task(
        &self,
        instance_id: &str,
        step_key: &str,
        code: &str,
        detail: &str,
        now: DateTime<Utc>,
    ) -> Result<String, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkflowRepository::new(self.scoped.session()).add_manual_task(
                instance_id,
                step_key,
                code,
                detail,
                now,
            );
        }
        SqliteWorkflowRepository::new(self.scoped).add_manual_task(
            instance_id,
            step_key,
            code,
            detail,
            now,
        )
    }

    pub fn resolve_blocker(
        &self,
        blocker_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkflowRepository::new(self.scoped.session())
                .resolve_blocker(blocker_id, now);
        }
        SqliteWorkflowRepository::new(self.scoped).resolve_blocker(blocker_id, now)
    }

    pub fn resolve_manual_task(
        &self,
        task_id: &str,
        decision: &str,
        now: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkflowRepository::new(self.scoped.session())
                .resolve_manual_task(task_id, decision, now);
        }
        SqliteWorkflowRepository::new(self.scoped).resolve_manual_task(task_id, decision, now)
    }
}
