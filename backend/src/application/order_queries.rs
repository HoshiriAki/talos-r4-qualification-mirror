use std::sync::Arc;

use system_core::ExecutionContext;

use crate::repositories::{
    OrderListRequest, OrderReadPage, OrderReadProjection, RepositoryError, RepositoryProvider,
};

#[derive(Clone)]
pub struct OrderQueryService {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl OrderQueryService {
    pub fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    pub fn list(
        &self,
        ctx: &ExecutionContext,
        request: &OrderListRequest,
    ) -> Result<OrderReadPage, RepositoryError> {
        self.repository_provider.bind(ctx)?.orders().list(request)
    }

    pub fn get_by_id(
        &self,
        ctx: &ExecutionContext,
        id: &str,
    ) -> Result<Option<OrderReadProjection>, RepositoryError> {
        self.repository_provider.bind(ctx)?.orders().get_by_id(id)
    }
}
