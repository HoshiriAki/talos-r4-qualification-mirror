import assert from 'node:assert/strict'
import fs from 'node:fs'
import { check, paths } from './check-r4-public-api-governance.mjs'
import {
  contract as capabilityContract,
  validate as validateCapabilities,
} from './generate-r4-public-api-capabilities.mjs'

const baseline = new Map(
  Object.values(paths).map(file => [file, fs.readFileSync(file, 'utf8')]),
)

function mutated(file, replaceFrom, replaceTo = '') {
  const files = new Map(baseline)
  const source = files.get(file)
  assert(source.includes(replaceFrom), `mutation token missing from ${file}: ${replaceFrom}`)
  files.set(file, source.replace(replaceFrom, replaceTo))
  return fileName => files.get(fileName)
}

function mutatedAll(file, replaceFrom, replaceTo = '') {
  const files = new Map(baseline)
  const source = files.get(file)
  assert(source.includes(replaceFrom), `mutation token missing from ${file}: ${replaceFrom}`)
  files.set(file, source.replaceAll(replaceFrom, replaceTo))
  return fileName => files.get(fileName)
}

assert.deepEqual(check(file => baseline.get(file)), [])
assert(
  baseline.get(paths.config).includes('if raw == "*"'),
  'credential CORS must keep an explicit wildcard guard',
)

const capabilities = capabilityContract()
validateCapabilities(capabilities)
assert.equal(capabilities.rateQuota.machineBearer.preAuthIngress.limit, 30)
assert.equal(capabilities.rateQuota.machineBearer.postAuthClient.rpmMax, 600)
assert.equal(capabilities.rateQuota.providerWebhook.preBodyIngress.limit, 60)
assert.equal(capabilities.responseSize.globalHardLimitBytes, null)
assert.equal(capabilities.idempotency.machineExecute.universalReplayGuarantee, false)

{
  const failures = check(mutated(paths.routes, '.allow_headers([', '.allow_headers(Any) // '))
  assert(failures.some(failure => failure.includes('wildcard credential CORS remains')))
}

{
  const failures = check(mutated(paths.routes, '.layer(cors)', '.layer(axum::middleware::from_fn(|req, next| async move { next.run(req).await }))'))
  assert(failures.some(failure => failure.includes('CORS must be source-composed before governance')))
}

{
  const failures = check(mutated(
    paths.csrf,
    'if IngressSurface::classify(req.uri().path()).csrf_exempt() {',
    'if req.uri().path().starts_with("/api/machine/") {',
  ))
  assert(failures.some(failure => failure.includes('machine CSRF exemption bypasses ingress contract')))
}

{
  const failures = check(mutated(
    paths.csrf,
    'require_https && parsed.scheme() != "https"',
    'false',
  ))
  assert(failures.some(failure => failure.includes('require_https && parsed.scheme() != "https"')))
}

{
  const failures = check(mutatedAll(paths.proxy, 'TRUSTED_PROXY_CIDRS', 'REMOVED_PROXY_ALLOWLIST'))
  assert(failures.some(failure => failure.includes('TRUSTED_PROXY_CIDRS')))
}

{
  const failures = check(mutated(
    paths.authRoutes,
    'use crate::middleware::trusted_proxy::ResolvedClientIp;',
    '',
  ))
  assert(failures.some(failure => failure.includes('ResolvedClientIp')))
}

{
  const failures = check(mutated(
    paths.authRoutes,
    'client_ip: Option<Extension<ResolvedClientIp>>',
    '/* TRUSTED_PROXY_COUNT */ client_ip: Option<Extension<ResolvedClientIp>>',
  ))
  assert(failures.some(failure => failure.includes('auth rate limiting reinterprets raw proxy input')))
}

{
  const failures = check(mutated(paths.config, 'let auth_cookie_secure = if is_production', 'let auth_cookie_secure = if false'))
  assert(failures.some(failure => failure.includes('let auth_cookie_secure = if is_production')))
}

{
  const failures = check(mutated(paths.config, 'address.is_loopback()', 'true'))
  assert(failures.some(failure => failure.includes('address.is_loopback()')))
}

{
  const failures = check(mutatedAll(
    paths.config,
    'wildcard origin is forbidden with credentials',
    'wildcard credential origin accepted',
  ))
  assert(failures.some(failure => failure.includes('wildcard origin is forbidden with credentials')))
}

{
  const failures = check(mutatedAll(paths.error, 'HTTP_BAD_REQUEST', 'HTTP_400_REMOVED'))
  assert(failures.some(failure => failure.includes('HTTP_BAD_REQUEST')))
}

{
  const failures = check(mutatedAll(paths.governance, '"/metrics"', '"/metrics-removed"'))
  assert(failures.some(failure => failure.includes('"/metrics"')))
}

{
  const failures = check(mutated(paths.governance, 'DefaultBodyLimit::max', 'DefaultBodyLimit::disable'))
  assert(failures.some(failure => failure.includes('DefaultBodyLimit::max')))
}

{
  const failures = check(mutated(
    paths.governance,
    'fn hard_timeout_allowed(method: &Method) -> bool {\n    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)\n}',
    'fn hard_timeout_allowed(_method: &Method) -> bool { true }',
  ))
  assert(failures.some(failure => failure.includes('Method::GET | Method::HEAD | Method::OPTIONS')))
}

{
  const failures = check(mutated(
    paths.governance,
    '} else {\n        // Do not drop state-changing handler futures at an arbitrary outer\n        // deadline. Registry/idempotency/provider-specific code owns outcome\n        // semantics for mutations.\n        next.run(request).await\n    };',
    '} else { timeout_response() };',
  ))
  assert(failures.some(failure => failure.includes('next.run(request).await')))
}

{
  const failures = check(mutated(paths.governance, 'frame-ancestors \'self\'', 'frame-ancestors *'))
  assert(failures.some(failure => failure.includes("frame-ancestors 'self'")))
}

{
  const failures = check(mutatedAll(paths.governance, 'HTTP_UNSUPPORTED_MEDIA_TYPE', 'HTTP_415_REMOVED'))
  assert(failures.some(failure => failure.includes('HTTP_UNSUPPORTED_MEDIA_TYPE')))
}

{
  const failures = check(mutated(paths.main, 'CORS_ALLOWED_ORIGIN is required in production', 'cors optional'))
  assert(failures.some(failure => failure.includes('CORS_ALLOWED_ORIGIN is required in production')))
}

{
  const failures = check(mutated(paths.main, 'routes::create_router(state, metrics)', 'routes::create_router(state, metrics).layer(cors)'))
  assert(failures.some(failure => failure.includes('forbidden public API composition bypass')))
}

{
  const failures = check(mutated(paths.generator, 'handMaintainedEndpointCopy: false', 'handMaintainedEndpointCopy: true'))
  assert(failures.some(failure => failure.includes('handMaintainedEndpointCopy: false')))
}

{
  const failures = check(mutated(
    paths.generator,
    'const mountedFactories = mountedRouteFactories(routesMod)',
    'const mountedFactories = fs.readdirSync(ROUTES_DIR)',
  ))
  assert(failures.some(failure => failure.includes('ghost endpoints')))
}

{
  const failures = check(mutated(paths.generator, 'const policy = governancePolicy(governance)', "const API_PREFIXES = ['/api']; const policy = governancePolicy(governance)"))
  assert(failures.some(failure => failure.includes('duplicates ingress API prefixes')))
}

{
  const failures = check(mutated(paths.generator, 'machineVersion(machineMigration)', "'v1'"))
  assert(failures.some(failure => failure.includes('machineVersion(machineMigration)')))
}

{
  const failures = check(mutatedAll(paths.generator, 'deprecationDate: null', 'deprecationDate: 0'))
  assert(failures.some(failure => failure.includes('deprecationDate: null')))
}

{
  const failures = check(mutatedAll(paths.workflow, 'pnpm quality:r4-public-api:test', 'echo skipped-r4-p4-test'))
  assert(failures.some(failure => failure.includes('pnpm quality:r4-public-api:test')))
}

{
  const failures = check(mutated(
    paths.frontendIndex,
    '<script src="/theme-bootstrap.js"></script>',
    '<script>localStorage.getItem(\'talos-theme\')</script>',
  ))
  assert(failures.some(failure => failure.includes('inline script conflicts')))
}

console.log('R4-P4 public Web/API governance mutation/capability tests: PASS')
