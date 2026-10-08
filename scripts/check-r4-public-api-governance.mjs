import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  contract as capabilityContract,
  validate as validateCapabilities,
} from './generate-r4-public-api-capabilities.mjs'

export const paths = {
  governance: 'backend/src/middleware/api_governance.rs',
  runtimeTests: 'backend/src/middleware/api_governance_runtime_tests.rs',
  csrf: 'backend/src/middleware/csrf.rs',
  proxy: 'backend/src/middleware/trusted_proxy.rs',
  routes: 'backend/src/routes/mod.rs',
  authRoutes: 'backend/src/routes/auth.rs',
  machine: 'backend/src/routes/machine_api.rs',
  state: 'backend/src/state.rs',
  metrics: 'backend/src/routes/metrics.rs',
  main: 'backend/src/main.rs',
  config: 'backend/src/config.rs',
  error: 'backend/src/error.rs',
  generator: 'scripts/generate-r4-public-api-contract.mjs',
  generatorTest: 'scripts/generate-r4-public-api-contract.test.mjs',
  capabilities: 'scripts/generate-r4-public-api-capabilities.mjs',
  capabilitiesTest: 'scripts/generate-r4-public-api-capabilities.test.mjs',
  packageJson: 'package.json',
  workflow: '.github/workflows/exact-head-qualification.yml',
  frontendIndex: 'frontend/index.html',
  themeBootstrap: 'frontend/public/theme-bootstrap.js',
}

export function check(read) {
  const failures = []
  const requireTokens = (file, tokens) => {
    const source = read(file)
    if (source === undefined) {
      failures.push(`${file}: missing file`)
      return ''
    }
    for (const token of tokens) {
      if (!source.includes(token)) failures.push(`${file}: missing ${token}`)
    }
    return source
  }

  const governance = requireTokens(paths.governance, [
    'enum IngressSurface', 'BrowserSession', 'MachineBearer', 'ProviderWebhook',
    'PublicDiagnostic', 'RequestId', 'x-request-id', 'no-store', 'is_api_path',
    '"/metrics"',
    'enum ApiResourceProfile', 'DefaultBodyLimit::max', 'request_body_limit_bytes()',
    'JSON_REQUEST_BODY_LIMIT_BYTES', 'UPLOAD_REQUEST_BODY_LIMIT_BYTES',
    'WEBHOOK_REQUEST_BODY_LIMIT_BYTES', 'timeout_secs()',
    'ApiDeadline', 'expires_at: Instant', 'remaining(&self)',
    'hard_timeout_allowed', 'Method::GET | Method::HEAD | Method::OPTIONS',
    'if hard_timeout_allowed(&method)', 'tokio::time::timeout(budget, next.run(request))',
    'next.run(request).await', 'hard_timeout_never_cancels_mutation_methods',
    'Registry/idempotency/provider-specific code owns outcome',
    'HTTP_PAYLOAD_TOO_LARGE', 'HTTP_DEADLINE_EXCEEDED',
    'HTTP_METHOD_NOT_ALLOWED', 'HTTP_UNSUPPORTED_MEDIA_TYPE',
    'HTTP_UNPROCESSABLE_CONTENT', 'HTTP_REQUEST_HEADERS_TOO_LARGE',
    'normalize_framework_rejection', 'response_is_json',
    'content-security-policy', 'frame-ancestors \'self\'', 'x-frame-options',
    'strict-transport-security', 'pub fn from_config',
    'public_diagnostic_is_non_cacheable_and_uses_api_transport_errors',
  ])
  if (/tokio::time::timeout\(\s*budget,\s*next\.run\(request\)\s*\)\.await\s*;/.test(governance)) {
    failures.push(`${paths.governance}: unconditional outer timeout can erase mutation outcome`)
  }
  for (const prefix of ['/api/machine/', '/api/integrations/webhooks/']) {
    if (!governance.includes(prefix)) failures.push(`${paths.governance}: missing governed prefix ${prefix}`)
  }
  for (const upload of [
    '/users/import-orders', '/users/import-devices-by-orderno',
    '/users/import-notes-by-orderno', '/devices/import-excel',
  ]) {
    if (!governance.includes(upload)) failures.push(`${paths.governance}: upload profile missing ${upload}`)
  }

  requireTokens(paths.runtimeTests, [
    'cors_preflight_is_inside_governance_and_receives_request_id',
    'oversized_json_is_stable_and_client_request_id_is_replaced',
    'framework_unsupported_media_type_uses_stable_json_envelope',
    'public_diagnostic_is_no_store_and_normalizes_method_rejection',
  ])

  const csrf = requireTokens(paths.csrf, [
    'IngressSurface::classify', 'csrf_exempt()', 'AUTH_CSRF_REJECTED',
    'State(state): State<Arc<AppState>>', 'csrf_source_matches',
    'url::Url::parse', 'state.config.public_https',
    'require_https && parsed.scheme() != "https"',
    'origin_host.eq_ignore_ascii_case(expected_host)',
    'csrf_authority_preserves_explicit_port_and_normalizes_default_port',
    'public_https_rejects_same_authority_http_origin',
  ])
  if (csrf.includes('req.uri().path().starts_with("/api/machine/")')) {
    failures.push(`${paths.csrf}: machine CSRF exemption bypasses ingress contract`)
  }
  if (csrf.includes('req.uri().path().starts_with("/api/integrations/webhooks/")')) {
    failures.push(`${paths.csrf}: webhook CSRF exemption bypasses ingress contract`)
  }

  requireTokens(paths.proxy, [
    'TRUSTED_PROXY_COUNT', 'TRUSTED_PROXY_CIDRS', 'ConnectInfo(peer_addr)',
    'if !self.is_trusted(peer_ip)', 'headers.remove(&X_FORWARDED_FOR)',
    'headers.remove(&X_FORWARDED_PROTO)', 'headers.remove(&X_FORWARDED_HOST)',
    'direct_client_cannot_spoof_forwarded_client_ip',
    'multi_hop_chain_rejects_untrusted_intermediate_proxy',
    'ResolvedClientIp(resolved)',
  ])

  const authRoutes = requireTokens(paths.authRoutes, [
    'use crate::middleware::trusted_proxy::ResolvedClientIp;',
    'client_ip: Option<Extension<ResolvedClientIp>>',
    'get_client_ip(client_ip.as_ref())',
    'login_rate_key_uses_only_resolved_client_ip_extension',
  ])
  for (const forbidden of ['TRUSTED_PROXY_COUNT', 'x-forwarded-for', 'x-real-ip']) {
    if (authRoutes.toLowerCase().includes(forbidden.toLowerCase())) {
      failures.push(`${paths.authRoutes}: auth rate limiting reinterprets raw proxy input: ${forbidden}`)
    }
  }

  requireTokens(paths.machine, [
    'header::COOKIE', 'x-talos-authority', 'x-talos-execution-mode',
    'header::AUTHORIZATION', 'execute_machine_with_repository(', 'MACHINE_INGRESS_RATE_LIMITED', 'no-store',
    'ResolvedClientIp', '.extensions()', '.get::<ResolvedClientIp>()',
    'ConnectInfo(peer_addr)',
  ])

  const state = requireTokens(paths.state, [
    'FIXTURE_WEBHOOK_RATE_LIMIT', 'FIXTURE_WEBHOOK_RATE_WINDOW',
    'FIXTURE_WEBHOOK_RATE_BUCKETS',
    'let key = hex::encode(Sha256::digest(endpoint_token.as_bytes()));',
    'fixture_webhook_rate_limit_is_scoped_to_endpoint_token_digest',
  ])
  if (state.includes('format!("{peer_ip}:{token_digest}")')) {
    failures.push(`${paths.state}: fixture webhook limiter still keys on reverse-proxy peer address`)
  }

  const routes = requireTokens(paths.routes, [
    'create_router(state: Arc<AppState>, metrics: Arc<RuntimeMetrics>)',
    'CorsLayer::new()', '.allow_credentials(true)',
    'Method::GET', 'Method::POST', 'Method::DELETE',
    'idempotency-key', 'x-talos-authority', 'x-request-id', 'x-correlation-id',
    '.layer(cors)', 'CORS sits inside the common governance boundary',
    'api_governance::ApiGovernancePolicy::from_config',
    'api_governance::api_governance', 'TrustedProxyPolicy::from_env()',
    'trusted_proxy_headers', 'HTTP_ROUTE_NOT_FOUND', 'WEB_ROUTE_NOT_FOUND',
    'state.clone()', 'csrf::csrf_check', '.merge(metrics::metrics_routes())',
  ])
  for (const forbidden of ['allow_headers(Any)', 'allow_methods(Any)', 'allow_origin(Any)']) {
    if (routes.includes(forbidden)) failures.push(`${paths.routes}: wildcard credential CORS remains: ${forbidden}`)
  }
  const corsLayer = routes.indexOf('.layer(cors)')
  const governanceLayer = routes.indexOf('api_governance_policy,\n            api_governance::api_governance')
  if (corsLayer === -1 || governanceLayer === -1 || corsLayer > governanceLayer) {
    failures.push(`${paths.routes}: CORS must be source-composed before governance so governance executes outside preflight`)
  }

  requireTokens(paths.metrics, [
    '.route("/metrics", get(metrics))', 'PlatformOperationsManage', 'public_https: false',
  ])

  const main = requireTokens(paths.main, [
    'let mut config = AppConfig::from_env()',
    'CORS_ALLOWED_ORIGIN is required in production', 'normalize_cors_origin',
    'invalid CORS_ALLOWED_ORIGIN', 'config.cors_allowed_origin = Some(cors_origin)',
    'routes::create_router(state, metrics)',
  ])
  for (const forbidden of [
    'RequestBodyLimitLayer::new(50 * 1024 * 1024)',
    'routes::create_router(state, metrics).layer(cors)',
    'routes::create_router(state, metrics, cors)',
  ]) {
    if (main.includes(forbidden)) failures.push(`${paths.main}: forbidden public API composition bypass remains: ${forbidden}`)
  }

  const config = requireTokens(paths.config, [
    'let auth_cookie_secure = if is_production', 'auth_cookie_secure,',
    'public_https', 'PUBLIC_HTTPS', 'validate_vite_dev_url',
    'parsed.scheme() != "http"', 'address.is_loopback()',
    'is_production || raw.is_empty()', 'vite_redirect_target_is_dev_loopback_only',
    'normalize_cors_origin', 'wildcard origin is forbidden with credentials',
    'origin must not contain userinfo', 'origin must not contain path, query or fragment',
    'PUBLIC_HTTPS requires an https origin',
    'credential_cors_rejects_wildcard_and_non_origin_urls',
  ])
  if (/if is_production\s*\{\s*false/.test(config)) {
    failures.push(`${paths.config}: production cookie Secure may not downgrade`)
  }

  const error = requireTokens(paths.error, [
    'HTTP_BAD_REQUEST', 'HTTP_NOT_FOUND', 'HTTP_CONFLICT', 'HTTP_INTERNAL_ERROR',
    'code: Some(code.to_string())', 'generic_client_errors_receive_stable_public_codes',
  ])
  if (/AppError::BadRequest\(_\)\s*=>\s*\(StatusCode::BAD_REQUEST,\s*None\)/.test(error)) {
    failures.push(`${paths.error}: generic bad request has no stable public code`)
  }
  if (/AppError::NotFound\(_\)\s*=>\s*\(StatusCode::NOT_FOUND,\s*None\)/.test(error)) {
    failures.push(`${paths.error}: generic not found has no stable public code`)
  }

  const generator = requireTokens(paths.generator, [
    "schema: 'talos.public-api-contract/v1'", 'ROUTES_DIR', 'ROUTES_MOD',
    'mountedRouteFactories', 'mountedRouteModules', 'routeFactoryBody',
    'governancePolicy', 'ingressPrefixForVariant', 'extractConstStringArray',
    'machine and webhook ingress prefixes must remain distinct',
    'mounted route factories + their Router::route literals',
    'mounted HTTP route is not covered by ingress API policy',
    "'GET /metrics'", '/metrics must remain a governed authenticated API surface',
    'MACHINE_MIGRATION', 'machineVersion(machineMigration)',
    'ORDER_V2_ROUTE', 'orderV2Pagination(orderV2)',
    'P2_CLOSURE', 'compatibilityLifecycle(p2)',
    'ORDER_USERS_COMPATIBILITY_HOLD', 'ORDER_USERS_REPLACEMENT_PATH', 'DEVICE_SERIAL_COMPATIBILITY_HOLD',
    'handMaintainedEndpointCopy: false', 'deprecationDate: null', 'sunsetDate: null',
    'genericCursorContract: null', 'webSocketPublicSurface: false',
    'ssePublicSurface: false', "process.argv.includes('--check')",
  ])
  if (generator.includes("currentMachineVersion: 'v1'")) {
    failures.push(`${paths.generator}: machine API version is hand-maintained instead of derived`)
  }
  if (generator.includes('handMaintainedEndpointCopy: true')) {
    failures.push(`${paths.generator}: generated contract claims a hand-maintained endpoint copy`)
  }
  if (generator.includes('const API_PREFIXES =')) {
    failures.push(`${paths.generator}: generator duplicates ingress API prefixes instead of deriving governance policy`)
  }
  if (generator.includes('fs.readdirSync(ROUTES_DIR)')) {
    failures.push(`${paths.generator}: generator scans unmounted route modules and can publish ghost endpoints`)
  }

  requireTokens(paths.generatorTest, [
    'governancePolicy(governanceFixture)',
    "assert.equal(policy.machinePrefix, '/api/machine/')",
    "assert.equal(policy.webhookPrefix, '/api/integrations/webhooks/')",
    'routeFactoryBody(moduleFixture, \'machine_routes\')',
    '/api/test-only-ghost',
    "['.merge(', '.nest(', '.nest_service(', '.route_service(']",
    'const actualContract = contract()',
    "endpoint.path === '/webhooks/sf-express'",
  ])

  requireTokens(paths.capabilities, [
    "schema: 'talos.public-api-capabilities/v1'",
    'globalRequestQuota: false',
    'universalReplayGuarantee: false',
    'globalHardLimitBytes: null',
    "key: 'ResolvedClientIp + normalized username'",
    "key: 'endpoint-token SHA-256 digest'",
    'readMachineRpmRange', 'readIdempotencyBounds',
    'fixture webhook limiter still keys on reverse-proxy peer address',
  ])
  requireTokens(paths.capabilitiesTest, [
    "assert.equal(result.rateQuota.machineBearer.preAuthIngress.limit, 30)",
    "assert.equal(result.rateQuota.machineBearer.postAuthClient.rpmMax, 600)",
    "assert.equal(result.rateQuota.providerWebhook.preBodyIngress.limit, 60)",
    "assert.equal(result.responseSize.globalHardLimitBytes, null)",
  ])

  requireTokens(paths.packageJson, [
    '"quality:r4-public-api:test"', 'check-r4-public-api-governance.test.mjs',
    'generate-r4-public-api-contract.test.mjs',
    '"quality:r4-public-api"', 'generate-r4-public-api-contract.mjs --check',
  ])

  const workflow = requireTokens(paths.workflow, [
    'Validate R4 public Web/API governance',
    'pnpm quality:r4-public-api:test', 'pnpm quality:r4-public-api',
  ])
  if (!workflow.includes('github.event.pull_request.draft == false')) {
    failures.push(`${paths.workflow}: exact-head draft guard missing`)
  }

  const frontendIndex = requireTokens(paths.frontendIndex, ['/theme-bootstrap.js'])
  if (/<script>(?:.|\n)*?<\/script>/m.test(frontendIndex)) {
    failures.push(`${paths.frontendIndex}: inline script conflicts with strict script-src 'self' CSP`)
  }
  requireTokens(paths.themeBootstrap, ["localStorage.getItem('talos-theme')", 'data-theme', 'light'])

  return failures
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const failures = check(file => {
    const resolved = path.resolve(file)
    return fs.existsSync(resolved) ? fs.readFileSync(resolved, 'utf8') : undefined
  })
  if (!failures.length) {
    try {
      const capabilities = capabilityContract()
      validateCapabilities(capabilities)
    } catch (error) {
      failures.push(`R4-P4 capability baseline: ${error instanceof Error ? error.message : String(error)}`)
    }
  }
  if (failures.length) {
    console.error(failures.join('\n'))
    process.exitCode = 1
  } else {
    console.log('R4-P4 public Web/API governance boundary: PASS')
  }
}
