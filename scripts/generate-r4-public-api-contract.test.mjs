import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import './generate-r4-public-api-capabilities.test.mjs'
import {
  contract,
  governancePolicy,
  mountedRouteFactories,
  mountedRouteModules,
  parseRouteCalls,
  routeFactoryBody,
  validate,
} from './generate-r4-public-api-contract.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const governanceFixture = `
const API_PREFIXES: &[&str] = &[
    "/api",
    "/auth",
    "/metrics",
];

impl IngressSurface {
    pub fn classify(path: &str) -> Self {
        if path.starts_with("/api/machine/") {
            Self::MachineBearer
        } else if path.starts_with("/api/integrations/webhooks/") {
            Self::ProviderWebhook
        } else if path == "/health" || path == "/ready" {
            Self::PublicDiagnostic
        } else if is_api_path(path) {
            Self::BrowserSession
        } else {
            Self::WebUi
        }
    }
}

/// HTTP resource profile fixture boundary.
impl ApiResourceProfile {
    pub fn classify(path: &str) -> Self {
        match path {
            "/api/upload-a"
            | "/api/upload-b" => Self::Upload,
            "/api/export-a" | "/api/export-b" => Self::Extended,
            "/health" | "/ready" => Self::NoBody,
            _ if path.starts_with("/api/integrations/webhooks/") => Self::Webhook,
            _ if is_api_path(path) => Self::Json,
            _ => Self::NoBody,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ApiGovernancePolicy;
`

const policy = governancePolicy(governanceFixture)
assert.equal(policy.machinePrefix, '/api/machine/')
assert.equal(policy.webhookPrefix, '/api/integrations/webhooks/')
assert.notEqual(policy.machinePrefix, policy.webhookPrefix)
assert.deepEqual([...policy.diagnosticPaths], ['/health', '/ready'])
assert.deepEqual([...policy.uploadRoutes], ['/api/upload-a', '/api/upload-b'])
assert.deepEqual([...policy.extendedRoutes], ['/api/export-a', '/api/export-b'])
assert.deepEqual([...policy.noBodyRoutes], ['/health', '/ready'])
assert(policy.apiPrefixes.includes('/metrics'))

const routesModFixture = `
Router::new()
    .merge(auth::auth_routes())
    .merge(metrics::metrics_routes())
    .merge(integrations::integration_routes(rate_limiter.clone()))
`
assert.deepEqual(mountedRouteModules(routesModFixture), ['auth', 'integrations', 'metrics'])
assert.deepEqual(mountedRouteFactories(routesModFixture), [
  { moduleName: 'auth', factoryName: 'auth_routes' },
  { moduleName: 'integrations', factoryName: 'integration_routes' },
  { moduleName: 'metrics', factoryName: 'metrics_routes' },
])

const moduleFixture = `
pub fn machine_routes() -> Router<State> {
    Router::new()
        .route("/api/machine/{version}/tenants/{tenant}/execute", post(execute))
        .route("/api/integrations/webhooks/{endpoint_token}", post(webhook))
        .route("/metrics", get(metrics))
        .route("/health", get(health))
        .route("/ready", get(ready))
}

#[cfg(test)]
mod tests {
    fn test_router() {
        let _app = Router::new().route("/api/test-only-ghost", get(test_only));
    }
}
`
const mountedBody = routeFactoryBody(moduleFixture, 'machine_routes')
assert(!mountedBody.includes('/api/test-only-ghost'))
const endpoints = parseRouteCalls(mountedBody, 'fixture.rs#machine_routes', policy)
assert(!endpoints.some(endpoint => endpoint.path === '/api/test-only-ghost'))
const byPath = new Map(endpoints.map(endpoint => [endpoint.path, endpoint]))
assert.equal(
  byPath.get('/api/machine/{version}/tenants/{tenant}/execute').ingressSurface,
  'machine-bearer',
)
assert.equal(
  byPath.get('/api/integrations/webhooks/{endpoint_token}').ingressSurface,
  'provider-webhook',
)
assert.equal(
  byPath.get('/api/integrations/webhooks/{endpoint_token}').resourceProfile,
  'webhook',
)
assert.equal(byPath.get('/metrics').ingressSurface, 'browser-session')
assert.equal(byPath.get('/metrics').resourceProfile, 'json')
assert.equal(byPath.get('/health').ingressSurface, 'public-diagnostic')
assert.equal(byPath.get('/health').resourceProfile, 'no-body')
assert.equal(byPath.get('/ready').ingressSurface, 'public-diagnostic')
assert.equal(byPath.get('/ready').resourceProfile, 'no-body')

// The production contract parser supports literal Router::route factories only.
// Every mounted factory must own at least one literal route; delegation wrappers
// would otherwise create a silent hole in the generated public contract.
const routesModPath = path.join(ROOT, 'backend/src/routes/mod.rs')
const actualRoutesMod = fs.readFileSync(routesModPath, 'utf8')
for (const { moduleName, factoryName } of mountedRouteFactories(actualRoutesMod)) {
  const source = fs.readFileSync(path.join(ROOT, `backend/src/routes/${moduleName}.rs`), 'utf8')
  const body = routeFactoryBody(source, factoryName)
  assert(
    body.includes('.route('),
    `${moduleName}::${factoryName} is mounted but owns no literal Router::route entries`,
  )
  for (const unsupported of ['.merge(', '.nest(', '.nest_service(', '.route_service(']) {
    assert(
      !body.includes(unsupported),
      `${moduleName}::${factoryName} uses unsupported route composition ${unsupported}`,
    )
  }
}

const actualContract = contract()
validate(actualContract)
assert(actualContract.endpoints.some(endpoint => endpoint.method === 'GET' && endpoint.path === '/metrics'))
assert(actualContract.endpoints.some(endpoint => endpoint.method === 'GET' && endpoint.path === '/ready'))
assert(!actualContract.endpoints.some(endpoint => endpoint.path === '/webhooks/sf-express'))
assert(!actualContract.endpoints.some(endpoint => endpoint.source.includes('#test_router')))

console.log('R4-P4 public API contract parser tests: PASS')
