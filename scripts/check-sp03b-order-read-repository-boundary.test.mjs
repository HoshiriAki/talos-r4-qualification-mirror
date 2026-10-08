#!/usr/bin/env node

import assert from 'node:assert/strict'

import { checkSp08ScopedRepositoryBoundary } from './check-sp08-scoped-repository-boundary.mjs'
import { checkSp03bOrderReadRepositoryBoundary } from './check-sp03b-order-read-repository-boundary.mjs'

const clean = {
  'backend/src/repositories/order_read.rs': `
pub enum OrderSortField { StartDate, EndDate, TotalPrice, CreatedAt }
pub enum OrderSortDirection { Asc, Desc }
pub struct OrderListRequest {
  pub page: u32,
  pub page_size: u32,
  pub order_no: Option<String>,
  pub status: Option<String>,
  pub keyword: Option<String>,
  pub sort_by: OrderSortField,
  pub sort_direction: OrderSortDirection,
}
pub struct OrderReadProjection { pub id: String }
pub struct OrderReadPage { pub orders: Vec<OrderReadProjection> }
pub struct ScopedOrderReadRepository<'a> {
  session: &'a SqliteRepositorySession,
}
impl ScopedOrderReadRepository<'_> {
  pub(in crate::repositories) fn new(scoped: &ScopedRepositories) -> Self { Self { session: scoped.session() } }
  pub fn list(&self, request: &OrderListRequest) -> Result<OrderReadPage, RepositoryError> {
    let tenant_id = self.session.binding().tenant_id().as_str();
    let page_size = request.page_size.clamp(1, 200);
    let a = "o.tenant_id = ?";
    let b = "od.tenant_id = ?";
    let c = "WHERE tenant_id = ?";
    self.session.read(|connection| { todo!() })
  }
  pub fn get_by_id(&self, id: &str) -> Result<Option<OrderReadProjection>, RepositoryError> {
    let tenant_id = self.session.binding().tenant_id().as_str();
    self.session.read(|connection| { todo!() })
  }
}`,
  'backend/src/repositories/contracts/provider.rs': `
pub struct ScopedRepositories { session: SqliteRepositorySession }
impl ScopedRepositories {
  pub fn orders(&self) -> ScopedOrderReadRepository<'_> { ScopedOrderReadRepository::new(self) }
}`,
  'backend/src/application/order_queries.rs': `
pub struct OrderQueryService {
  repository_provider: Arc<dyn RepositoryProvider>,
}
impl OrderQueryService {
  pub fn list(&self, ctx: &ExecutionContext, request: &OrderListRequest) -> Result<OrderReadPage, RepositoryError> {
    self.repository_provider.bind(ctx)?.orders().list(request)
  }
  pub fn get_by_id(&self, ctx: &ExecutionContext, id: &str) -> Result<Option<OrderReadProjection>, RepositoryError> {
    self.repository_provider.bind(ctx)?.orders().get_by_id(id)
  }
}`,
  'backend/src/application/services.rs': `
pub struct ApplicationServices {
  clock: Arc<dyn Clock>,
  module_client: Arc<dyn ModuleClient>,
  worker_runner: Arc<dyn WorkerRunner>,
  repository_provider: Arc<dyn RepositoryProvider>,
}
impl ApplicationServices {
  pub fn order_queries(&self) -> OrderQueryService { OrderQueryService::new(self.repository_provider()) }
}`,
  'backend/src/routes/orders.rs': 'pub fn routes() {}',
}

function changed(path, transform) {
  const fixture = structuredClone(clean)
  fixture[path] = transform(fixture[path] ?? '')
  return fixture
}

function expectFailure(number, fixture, evidence, changedPaths = [], options = {}) {
  const failures = checkSp03bOrderReadRepositoryBoundary(fixture, changedPaths, options)
  assert(
    failures.some((item) => item.evidence.includes(evidence)),
    `${number}. expected failure containing: ${evidence}\n${JSON.stringify(failures, null, 2)}`,
  )
}

assert.deepEqual(checkSp03bOrderReadRepositoryBoundary(clean), [], '1. clean boundary passes')
expectFailure(2, changed('backend/src/repositories/order_read.rs', (source) => source.replace('ScopedOrderReadRepository', 'MissingOrderRepository')), 'concrete typed production adapter')
expectFailure(3, changed('backend/src/repositories/order_read.rs', (source) => source.replace('pub(in crate::repositories) fn new', 'pub fn new')), 'construction must remain inside')
expectFailure(4, changed('backend/src/repositories/order_read.rs', (source) => source.replace('pub fn list', 'fn list')), 'typed list and get_by_id')
expectFailure(5, changed('backend/src/repositories/order_read.rs', (source) => source.replace('request: &OrderListRequest', 'request: &OrderListRequest, tenant: TenantId')), 'must not accept raw tenant')
expectFailure(6, changed('backend/src/repositories/order_read.rs', (source) => source.replace('pub order_no:', 'pub tenant_id: String,\n  pub order_no:')), 'must not carry tenant identity')
expectFailure(7, changed('backend/src/repositories/order_read.rs', (source) => source.replaceAll('self.session.binding().tenant_id()', 'TenantId::new("default")')), 'must derive from RepositoryBinding')
expectFailure(8, changed('backend/src/repositories/order_read.rs', (source) => source.replaceAll('tenant_id = ?', 'scope = ?')), 'must bind tenant_id in SQL')
expectFailure(9, changed('backend/src/repositories/order_read.rs', (source) => `${source}\nfn write() { let sql = "INSERT INTO orders VALUES (?)"; }`), 'must remain read-only')
expectFailure(10, changed('backend/src/repositories/order_read.rs', (source) => source.replace("session: &'a SqliteRepositorySession", 'pool: Pool<SqliteConnectionManager>')), 'scoped session rather than own raw storage')
expectFailure(11, changed('backend/src/repositories/order_read.rs', (source) => source.replace('pub sort_by: OrderSortField', 'pub sort_by: String')), 'closed enums')
expectFailure(12, changed('backend/src/repositories/order_read.rs', (source) => source.replace('request.page_size.clamp(1, 200)', 'request.page_size')), 'bounded to 1..=200')
expectFailure(13, changed('backend/src/repositories/contracts/provider.rs', (source) => source.replace('pub fn orders', 'fn orders')), 'must expose the typed order read adapter')
expectFailure(14, changed('backend/src/application/order_queries.rs', (source) => source.replace('self.repository_provider.bind(ctx)?.orders()', 'todo!()')), 'bind through RepositoryProvider')
expectFailure(15, changed('backend/src/application/order_queries.rs', (source) => source.replace('repository_provider: Arc<dyn RepositoryProvider>', 'pool: Pool<SqliteConnectionManager>')), 'must own only RepositoryProvider')
expectFailure(16, changed('backend/src/application/services.rs', (source) => source.replace('pub fn order_queries', 'fn order_queries')), 'derive OrderQueryService')
expectFailure(17, changed('backend/src/application/services.rs', (source) => source.replace('repository_provider:', 'order_queries: OrderQueryService,\n  repository_provider:')), 'without adding a fifth field')
expectFailure(18, changed('backend/src/routes/orders.rs', () => 'fn route(service: OrderQueryService) { let _ = service; }'), 'without owning repository/query types directly')
expectFailure(19, clean, 'exceed the authorized', ['.harness/feature_list.json'], { enforceSliceScope: true })
expectFailure(20, clean, 'exceed the authorized', ['backend/official/order/src/order.rs'], { enforceSliceScope: true })
expectFailure(21, clean, 'exceed the authorized', ['backend/src/services/order_service.rs'], { enforceSliceScope: true })
expectFailure(22, clean, 'exceed the authorized', ['backend/src/db/migrations/053_orders.sql'], { enforceSliceScope: true })
expectFailure(23, clean, 'changed-path discovery failed closed', ['GIT_CHANGED_PATH_DISCOVERY_FAILED:boom'])

const testOnlyWrite = changed('backend/src/repositories/order_read.rs', (source) => `${source}\n#[cfg(test)]\nmod tests { const SQL: &str = "INSERT INTO orders VALUES (?)"; }`)
assert.deepEqual(checkSp03bOrderReadRepositoryBoundary(testOnlyWrite), [], '24. test-only schema and fixture writes remain allowed')
assert(
  checkSp08ScopedRepositoryBoundary({}).some((item) => item.rule === 'TALOS-OPS-018'),
  '25. TALOS-OPS-018 remains importable and active',
)
assert.deepEqual(
  checkSp03bOrderReadRepositoryBoundary(clean, ['backend/src/routes/notify.rs']),
  [],
  '26. post-merge changes outside the Order read surface are not blocked by historical slice scope',
)
const approvedFacadeRoute = changed(
  'backend/src/routes/orders.rs',
  () => 'fn route(services: ApplicationServices) { let _ = services.order_queries(); }',
)
assert.deepEqual(
  checkSp03bOrderReadRepositoryBoundary(approvedFacadeRoute),
  [],
  '27. later-stage routes may consume the derived ApplicationServices order query facade',
)

console.log('SP-03B order read repository boundary checker self-test passed: 27 cases.')
