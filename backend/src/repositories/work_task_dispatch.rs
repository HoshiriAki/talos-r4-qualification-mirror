use crate::repositories::work_task::{
    SqliteWorkTaskRepository, WorkTaskListRequest, WorkTaskMutation, WorkTaskProjection,
    WorkTaskState,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::work_task_postgres::PostgresWorkTaskRepository;

pub struct ScopedWorkTaskRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedWorkTaskRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn list(
        &self,
        request: &WorkTaskListRequest,
    ) -> Result<Vec<WorkTaskProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkTaskRepository::new(self.scoped.session()).list(request);
        }
        SqliteWorkTaskRepository::new(self.scoped).list(request)
    }

    pub fn get_state(&self, id: &str) -> Result<Option<WorkTaskState>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkTaskRepository::new(self.scoped.session()).get_state(id);
        }
        SqliteWorkTaskRepository::new(self.scoped).get_state(id)
    }

    pub fn update_status(
        &self,
        id: &str,
        expected_version: i64,
        new_status: &str,
        now: &str,
    ) -> Result<Option<WorkTaskMutation>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkTaskRepository::new(self.scoped.session()).update_status(
                id,
                expected_version,
                new_status,
                now,
            );
        }
        SqliteWorkTaskRepository::new(self.scoped).update_status(
            id,
            expected_version,
            new_status,
            now,
        )
    }

    pub fn send_to_pc(
        &self,
        id: &str,
        expected_version: i64,
        now: &str,
    ) -> Result<Option<WorkTaskMutation>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresWorkTaskRepository::new(self.scoped.session()).send_to_pc(
                id,
                expected_version,
                now,
            );
        }
        SqliteWorkTaskRepository::new(self.scoped).send_to_pc(id, expected_version, now)
    }
}
