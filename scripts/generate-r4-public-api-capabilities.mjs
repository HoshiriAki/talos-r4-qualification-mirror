import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const GOVERNANCE = path.join(ROOT, 'backend/src/middleware/api_governance.rs')
const MACHINE_ROUTE = path.join(ROOT, 'backend/src/routes/machine_api.rs')
const MACHINE_SERVICE = path.join(ROOT, 'backend/src/services/machine_api.rs')
const MACHINE_SQLITE_REPOSITORY = path.join(ROOT, 'backend/src/repositories/machine_authority_sqlite.rs')
const MACHINE_POSTGRES_REPOSITORY = path.join(ROOT, 'backend/src/repositories/machine_authority_postgres.rs')
const REGISTRY = path.join(ROOT, 'backend/src/registry/mod.rs')
const AUTH_ROUTE = path.join(ROOT, 'backend/src/routes/auth.rs')
const AUTH_RATE_SERVICE = path.join(ROOT, 'backend/src/services/auth_rate_limit.rs')
const AUTH_SECURITY_REPOSITORY = path.join(ROOT, 'backend/src/repositories/auth_security.rs')
const CONFIG = path.join(ROOT, 'backend/src/config.rs')
const INTEGRATION_ROUTE = path.join(ROOT, 'backend/src/routes/integrations.rs')
const STATE = path.join(ROOT, 'backend/src/state.rs')

function read(file) {
  return fs.readFileSync(file, 'utf8')
}

function requireTokens(source, tokens, label) {
  for (const token of tokens) {
    if (!source.includes(token)) throw new Error(`${label}: missing ${token}`)
  }
}

function parseInteger(raw) {
  return Number(raw.replaceAll('_', ''))
}

function readIntegerConst(source, name) {
  const match = source.match(
    new RegExp(`(?:pub\\(crate\\)\\s+)?const\\s+${name}:\\s*[A-Za-z0-9_]+\\s*=\\s*([0-9_]+);`),
  )
  if (!match) throw new Error(`missing integer constant ${name}`)
  return parseInteger(match[1])
}

function readDurationSeconds(source, name) {
  const match = source.match(
    new RegExp(`const\\s+${name}:\\s*Duration\\s*=\\s*Duration::from_secs\\(([0-9_]+)\\);`),
  )
  if (!match) throw new Error(`missing duration constant ${name}`)
  return parseInteger(match[1])
}

function readMachineRpmRange(source) {
  const match = source.match(/!\(([0-9_]+)\.\.\=([0-9_]+)\)\.contains\(&input\.rate_limit_rpm\)/)
  if (!match) throw new Error('machine client RPM range is not derivable')
  return { min: parseInteger(match[1]), max: parseInteger(match[2]) }
}

function readIdempotencyBounds(source) {
  const match = source.match(/key\.len\(\) < ([0-9_]+) \|\| key\.len\(\) > ([0-9_]+) \|\| !key\.is_ascii\(\)/)
  if (!match) throw new Error('machine Idempotency-Key bounds are not derivable')
  return { minLength: parseInteger(match[1]), maxLength: parseInteger(match[2]) }
}

export function contract() {
  const governance = read(GOVERNANCE)
  const machineRoute = read(MACHINE_ROUTE)
  const machineService = read(MACHINE_SERVICE)
  const machineSqliteRepository = read(MACHINE_SQLITE_REPOSITORY)
  const machinePostgresRepository = read(MACHINE_POSTGRES_REPOSITORY)
  const registry = read(REGISTRY)
  const authRoute = read(AUTH_ROUTE)
  const authRateService = read(AUTH_RATE_SERVICE)
  const authSecurityRepository = read(AUTH_SECURITY_REPOSITORY)
  const config = read(CONFIG)
  const integrationRoute = read(INTEGRATION_ROUTE)
  const state = read(STATE)

  requireTokens(governance, ['x-request-id', 'RequestId::generate()', 'ApiDeadline'], 'governance')
  requireTokens(machineRoute, [
    'ResolvedClientIp',
    '.extensions()',
    '.get::<ResolvedClientIp>()',
    'MACHINE_INGRESS_RATE_LIMITED',
    'idempotency-key',
    'x-correlation-id',
  ], 'machine route')
  requireTokens(machineService, ['rate_limit_rpm'], 'machine service')
  requireTokens(machineSqliteRepository, ['api_usage_windows'], 'machine SQLite repository')
  requireTokens(machinePostgresRepository, ['api_usage_windows'], 'machine PostgreSQL repository')
  requireTokens(registry, [
    'idempotency_key: Option<String>',
    'RequestId::new(correlation)',
    'idempotency_key,',
  ], 'registry machine entry')
  requireTokens(authRoute, [
    'ResolvedClientIp',
    'AuthRateLimiter',
    'auth_rate_limiter.check_login',
    'auth_rate_limiter.check_custom',
    'login_rate_key_uses_only_resolved_client_ip_extension',
  ], 'auth route')
  for (const forbidden of ['TRUSTED_PROXY_COUNT', 'x-forwarded-for', 'x-real-ip']) {
    if (authRoute.toLowerCase().includes(forbidden.toLowerCase())) {
      throw new Error(`auth route reinterprets raw proxy input: ${forbidden}`)
    }
  }
  requireTokens(authRateService, [
    'AuthSecurityRepository',
    'rate_key_digest',
    'login_key_hashes',
    'LOGIN_ACCOUNT_ATTEMPT_FACTOR',
    'LOGIN_SOURCE_ATTEMPT_FACTOR',
  ], 'durable auth rate service')
  requireTokens(authSecurityRepository, [
    'auth_rate_limit_state',
    'TransactionBehavior::Immediate',
    'mutate_rate_states',
  ], 'auth rate repository')
  for (const configToken of [
    'AUTH_LOGIN_RATE_WINDOW_MS',
    'AUTH_LOGIN_RATE_MAX_ATTEMPTS',
    'AUTH_LOGIN_RATE_BLOCK_MS',
  ]) {
    requireTokens(config, [configToken], 'auth rate config')
  }
  requireTokens(integrationRoute, [
    'WEBHOOK_RATE_LIMITED',
    'x-integration-event-id',
    'receipt.duplicate',
    'MAX_FIXTURE_WEBHOOK_BODY_BYTES',
  ], 'integration webhook route')
  requireTokens(state, [
    'FIXTURE_WEBHOOK_RATE_LIMIT',
    'FIXTURE_WEBHOOK_RATE_WINDOW',
    'FIXTURE_WEBHOOK_RATE_BUCKETS',
    'let key = hex::encode(Sha256::digest(endpoint_token.as_bytes()));',
    'fixture_webhook_rate_limit_is_scoped_to_endpoint_token_digest',
  ], 'fixture webhook limiter')
  if (state.includes('format!("{peer_ip}:{token_digest}")')) {
    throw new Error('fixture webhook limiter still keys on reverse-proxy peer address')
  }

  const rpm = readMachineRpmRange(machineService)
  const idempotency = readIdempotencyBounds(registry)

  return {
    schema: 'talos.public-api-capabilities/v1',
    requestIdentity: {
      requestId: {
        header: 'x-request-id',
        serverGenerated: true,
        callerValueTrusted: false,
        scope: 'all governed HTTP requests',
      },
      machineCorrelation: {
        header: 'x-correlation-id',
        serverGenerated: true,
        scope: '/api/machine/{version}/tenants/{tenant}/execute',
      },
      publicTraceHeader: null,
      note: 'P4 does not claim a public trace-id propagation header; internal tracing remains implementation detail.',
    },
    idempotency: {
      machineExecute: {
        requestHeader: 'idempotency-key',
        optional: true,
        asciiOnly: true,
        ...idempotency,
        propagation: 'validated header -> ExecutionContext.idempotency_key',
        universalReplayGuarantee: false,
        note: 'P4 guarantees transport validation and propagation only; command/domain idempotency semantics remain downstream.',
      },
      browserSession: {
        genericIdempotencyHeaderContract: false,
        note: 'Existing browser/domain commands retain their own concurrency/idempotency semantics; P4 does not invent a universal browser replay contract.',
      },
      providerWebhook: {
        idempotencyHeaderContract: false,
        providerEventIdHeader: 'x-integration-event-id',
        duplicateResultField: 'duplicate',
        note: 'Current fixture ingress performs provider-event dedup in the integration ingress; this is not an Idempotency-Key contract.',
      },
    },
    rateQuota: {
      browserSession: {
        globalRequestQuota: false,
        loginFailureLimiter: {
          key: 'ResolvedClientIp + normalized username',
          additionalDimensions: ['normalized username', 'ResolvedClientIp'],
          keyPersistence: 'SHA-256 digests only',
          durability: 'durable auth_rate_limit_state',
          configuredBy: [
            'AUTH_LOGIN_RATE_WINDOW_MS',
            'AUTH_LOGIN_RATE_MAX_ATTEMPTS',
            'AUTH_LOGIN_RATE_BLOCK_MS',
          ],
        },
        changePasswordFailureLimiter: {
          key: 'ResolvedClientIp + identity',
          keyPersistence: 'SHA-256 digest only',
          durability: 'durable auth_rate_limit_state',
          configuredBy: [
            'AUTH_LOGIN_RATE_WINDOW_MS',
            'AUTH_LOGIN_RATE_MAX_ATTEMPTS',
            'AUTH_LOGIN_RATE_BLOCK_MS',
          ],
        },
      },
      machineBearer: {
        preAuthIngress: {
          key: 'ResolvedClientIp; socket peer fallback only if governance extension is unavailable',
          limit: readIntegerConst(machineRoute, 'MACHINE_INGRESS_RATE_LIMIT'),
          windowSeconds: readDurationSeconds(machineRoute, 'MACHINE_INGRESS_RATE_WINDOW'),
          maxBuckets: readIntegerConst(machineRoute, 'MACHINE_INGRESS_RATE_BUCKETS'),
          durability: 'process-local',
        },
        postAuthClient: {
          key: 'machine client',
          rpmMin: rpm.min,
          rpmMax: rpm.max,
          durability: 'durable api_usage_windows',
          recheckedOnEveryRequest: true,
        },
      },
      providerWebhook: {
        preBodyIngress: {
          key: 'endpoint-token SHA-256 digest',
          limit: readIntegerConst(state, 'FIXTURE_WEBHOOK_RATE_LIMIT'),
          windowSeconds: readDurationSeconds(state, 'FIXTURE_WEBHOOK_RATE_WINDOW'),
          maxBuckets: readIntegerConst(state, 'FIXTURE_WEBHOOK_RATE_BUCKETS'),
          durability: 'process-local',
          fixtureOnly: true,
        },
      },
    },
    responseSize: {
      globalHardLimitBytes: null,
      explicitBinaryExportRoutes: ['/users/export', '/devices/export'],
      policy: 'P4 does not claim a universal response-byte cap. Existing paginated/domain-bounded JSON responses and explicit Extended binary exports retain their endpoint contracts.',
      newSurfaceRule: 'Any new streaming or intentionally unbounded public response requires an explicit response-size/duration/inflight policy before mount.',
    },
    streamingInflight: {
      webSocketPublicSurface: false,
      ssePublicSurface: false,
      globalPublicInflightLimit: null,
      note: 'No current public streaming route exists. P4 therefore freezes streaming as absent rather than inventing unused limits.',
    },
  }
}

export function validate(result) {
  if (result.rateQuota.machineBearer.preAuthIngress.limit <= 0) {
    throw new Error('machine ingress rate limit must be positive')
  }
  if (result.rateQuota.machineBearer.postAuthClient.rpmMin < 1) {
    throw new Error('machine client RPM minimum must be positive')
  }
  if (result.rateQuota.machineBearer.postAuthClient.rpmMax < result.rateQuota.machineBearer.postAuthClient.rpmMin) {
    throw new Error('machine client RPM range is invalid')
  }
  if (result.rateQuota.providerWebhook.preBodyIngress.limit <= 0) {
    throw new Error('fixture webhook ingress rate limit must be positive')
  }
  if (result.rateQuota.browserSession.loginFailureLimiter.durability !== 'durable auth_rate_limit_state') {
    throw new Error('browser login throttling must remain durable')
  }
  if (result.rateQuota.browserSession.changePasswordFailureLimiter.durability !== 'durable auth_rate_limit_state') {
    throw new Error('change-password throttling must remain durable')
  }
  if (result.idempotency.machineExecute.minLength < 1 || result.idempotency.machineExecute.maxLength < result.idempotency.machineExecute.minLength) {
    throw new Error('machine Idempotency-Key range is invalid')
  }
  if (result.responseSize.globalHardLimitBytes !== null) {
    throw new Error('P4 response-size baseline changed; update the explicit capability contract and review scope')
  }
}

function emit() {
  const result = contract()
  validate(result)
  const json = `${JSON.stringify(result, null, 2)}\n`
  const digest = crypto.createHash('sha256').update(json).digest('hex')
  if (process.argv.includes('--check')) {
    console.log(`R4-P4 generated public API capabilities: PASS (sha256:${digest})`)
  } else {
    process.stdout.write(json)
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  emit()
}
