#![cfg(feature = "postgres")]

use crate::repositories::RepositoryError;
use crate::repositories::optical_sop::{
    OpticalCompleteOutcome, OpticalCreateOutcome, OpticalInspectionProjection,
    OpticalListProjection, OpticalSopMutationError, OpticalStatsProjection, OpticalUpdateOutcome,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresOpticalSopRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresOpticalSopRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn create(
        &self,
        order_id: &str,
        device_serial_no: &str,
        inspector_id: &str,
        now: &str,
    ) -> Result<OpticalCreateOutcome, OpticalSopMutationError> {
        crate::repositories::optical_sop_postgres_mutation::create(
            self.session,
            order_id,
            device_serial_no,
            inspector_id,
            now,
        )
    }

    pub(in crate::repositories) fn update_step(
        &self,
        id: i64,
        step: &str,
        ok: bool,
        note: Option<&str>,
        now: &str,
    ) -> Result<OpticalUpdateOutcome, OpticalSopMutationError> {
        crate::repositories::optical_sop_postgres_mutation::update_step(
            self.session,
            id,
            step,
            ok,
            note,
            now,
        )
    }

    pub(in crate::repositories) fn complete(
        &self,
        id: i64,
        operator: &str,
        now: &str,
    ) -> Result<OpticalCompleteOutcome, OpticalSopMutationError> {
        crate::repositories::optical_sop_postgres_mutation::complete(
            self.session,
            id,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn get(
        &self,
        id: i64,
    ) -> Result<Option<OpticalInspectionProjection>, RepositoryError> {
        crate::repositories::optical_sop_postgres_read::get(self.session, id)
    }

    pub(in crate::repositories) fn list(
        &self,
        order_id: Option<&str>,
        device_serial_no: Option<&str>,
        overall_grade: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<OpticalListProjection, RepositoryError> {
        crate::repositories::optical_sop_postgres_read::list(
            self.session,
            order_id,
            device_serial_no,
            overall_grade,
            page,
            page_size,
        )
    }

    pub(in crate::repositories) fn stats(&self) -> Result<OpticalStatsProjection, RepositoryError> {
        crate::repositories::optical_sop_postgres_read::stats(self.session)
    }
}
