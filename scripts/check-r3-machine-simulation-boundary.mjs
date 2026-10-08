import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export const paths = [
  'backend/src/services/machine_api.rs',
  'backend/src/registry/mod.rs',
  'backend/src/routes/machine_api.rs',
  'backend/src/routes/tenant_memberships.rs',
  'backend/src/routes/api_keys.rs',
  'backend/src/db/migrations/066_r3_machine_api.sql',
  'backend/src/db/migrations/postgres/066_r3_machine_api.sql',
  'backend/src/integration/module.rs',
  'frontend/src/router/index.ts',
  'frontend/src/constants/sidebarGroups.ts',
  'frontend/src/utils/settings.ts',
  'backend/src/repositories/machine_authority.rs',
  'backend/src/repositories/machine_authority_sqlite.rs',
  'backend/src/repositories/machine_authority_postgres.rs',
  'backend/src/repositories/machine_authority_dispatch.rs',
  'backend/src/repositories/tenant_membership_authority.rs',
  'backend/src/repositories/tenant_membership_authority_postgres.rs',
  'backend/src/application/tenant_membership_compatibility.rs',
]
export const retiredPaths = [
  'frontend/src/api/api-keys.ts',
  'frontend/src/pages/ApiKeysPage.vue',
]

const SERVICE = 0
const REGISTRY = 1
const MACHINE_ROUTE = 2
const TENANT_MEMBERSHIPS = 3
const LEGACY_API_KEYS = 4
const SQLITE_MIGRATION = 5
const POSTGRES_MIGRATION = 6
const INTEGRATION_MODULE = 7
const MACHINE_SHARED = 11
const MACHINE_SQLITE = 12
const MACHINE_POSTGRES = 13
const MACHINE_DISPATCH = 14
const TENANT_MEMBERSHIP_SQLITE = 15
const TENANT_MEMBERSHIP_POSTGRES = 16
const TENANT_MEMBERSHIP_COMPAT = 17

export function check(read) {
  const failures = []
  const require = (file, tokens) => {
    const source = read(file)?.replaceAll('\r\n', '\n') ?? ''
    for (const token of tokens) if (!source.includes(token)) failures.push(file + ': missing ' + token)
    return source
  }
  const requireMachineMarkerOwnerGuard = (file, source) => {
    const start = source.indexOf('machine_marker_validate')
    const end = source.indexOf('machine_marker_immutable', start)
    const markerValidation = start >= 0 && end > start ? source.slice(start, end) : ''
    const ownerMembershipCheck = /EXISTS\s*\(\s*SELECT\s+1\s+FROM\s+tenant_memberships\s+WHERE\s+identity_id\s*=\s*NEW\.identity_id\s+AND\s+role\s*=\s*'owner'\s*\)/
    if (!ownerMembershipCheck.test(markerValidation)) {
      failures.push(file + ': machine marker validation must reject an existing owner membership')
    }
  }
  const requirePgMachineIdentityLock = (source, functionName) => {
    const functionStart = `CREATE OR REPLACE FUNCTION ${functionName}()`
    const start = source.indexOf(functionStart)
    const end = source.indexOf('END; $$;', start)
    const functionBody = start >= 0 && end > start ? source.slice(start, end) : ''
    if (!functionBody.includes('PERFORM machine_identity_guard_lock(NEW.identity_id);')) {
      failures.push('backend/src/db/migrations/postgres/066_r3_machine_api.sql: ' + functionName + ' must use the shared final-identity lock')
    }
  }

  const service = require(paths[SERVICE], [
    'MachineAuthorityRepository::new',
    'provision_with_repository',
    'authorize_with_repository',
    'lifecycle_with_repository',
    'list_with_repository',
    'credential_lifecycle_with_repository',
    'machine_scope_allowed',
    'FUTURE_CANONICAL_MIGRATION',
    'AccessRequirement::Authenticated',
    'AccessRequirement::TenantAdmin',
  ])
  for (const forbidden of ['rusqlite', 'TransactionBehavior', 'INSERT INTO api_', 'UPDATE api_', 'api_usage_windows']) {
    if (service.includes(forbidden)) {
      failures.push(paths[SERVICE] + ': policy facade must not own persistence token ' + forbidden)
    }
  }

  require(paths[MACHINE_SHARED], [
    'MachineAdminContext',
    'MachineAuthorization',
    'MachineIssuedCredential',
    'MachineScopeRecord',
    'machine_scope_allowed',
    '"two_fa" | "consent" | "deletion" | "user_settings"',
  ])

  const sqliteRepository = require(paths[MACHINE_SQLITE], [
    'verify_live_admin',
    'ct_eq(expected_hash.as_bytes())',
    'membership_status == "active"',
    'identity_status == "active"',
    'client_status == "active"',
    'credential_status == "active"',
    'tenant_id != requested_tenant',
    'version != api_version',
    '!scope_allowed',
    'used >= rate_limit_rpm',
    'TransactionBehavior::Immediate',
    'api_usage_windows',
    'api_scope_grants',
    'append_machine_audit',
    'machine_scope_allowed',
  ])
  const sqliteAuthorize = sqliteRepository.slice(
    sqliteRepository.indexOf('pub(crate) fn authorize('),
    sqliteRepository.indexOf('pub(crate) fn lifecycle('),
  )
  if (sqliteAuthorize.includes('created_by')) {
    failures.push(paths[MACHINE_SQLITE] + ': machine execution must not inherit provisioner authority')
  }

  const postgresRepository = require(paths[MACHINE_POSTGRES], [
    'verify_live_admin',
    'ct_eq(expected_hash.as_bytes())',
    'membership_status == "active"',
    'identity_status == "active"',
    'client_status == "active"',
    'credential_status == "active"',
    'tenant_id != requested_tenant',
    'version != api_version',
    '!scope_allowed',
    'used >= rate_limit_rpm',
    'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
    'FOR UPDATE OF k,c,m,i',
    'FROM api_usage_windows',
    'FOR UPDATE',
    'GREATEST(api_usage_windows.window_start,EXCLUDED.window_start)',
    'api_scope_grants',
    'append_machine_audit',
    'machine_scope_allowed',
    'SYS_MACHINE_PERSISTENCE',
  ])
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(postgresRepository)) {
    failures.push(paths[MACHINE_POSTGRES] + ': PostgreSQL machine authority must not depend on SQLite runtime types')
  }
  const postgresAuthorize = postgresRepository.slice(
    postgresRepository.indexOf('pub(crate) fn authorize('),
    postgresRepository.indexOf('pub(crate) fn lifecycle('),
  )
  if (postgresAuthorize.includes('created_by')) {
    failures.push(paths[MACHINE_POSTGRES] + ': machine execution must not inherit provisioner authority')
  }

  require(paths[MACHINE_DISPATCH], [
    'enum MachineAuthorityBackend',
    'Sqlite(SqliteMachineAuthorityRepository)',
    'Postgres(PostgresMachineAuthorityRepository)',
    'pub(crate) fn postgres(pool: sqlx::PgPool)',
  ])

  const registry = require(paths[REGISTRY], [
    'pub fn execute_machine(',
    'machine_api::authorize_with_repository(',
    'ActorIdentity::with_authority(auth.identity_id, auth.authority)',
  ])
  const machine = registry.slice(registry.indexOf('pub fn execute_machine('), registry.indexOf('pub fn new('))
  for (const token of ['ExecutionMode::Normal', 'self.execute(', '&ctx']) {
    if (!machine.includes(token)) failures.push('machine Registry entry missing ' + token)
  }

  const machineRoute = require(paths[MACHINE_ROUTE], [
    'deny_unknown_fields',
    'header::COOKIE',
    'x-talos-execution-mode',
    'execute_machine_with_repository(',
    'no-store',
    'MachineIngressRateLimiter',
    'ConnectInfo(peer_addr)',
    'MACHINE_INGRESS_RATE_LIMITED',
  ])
  if (!/post\(execute\)\.route_layer\([\s\S]{0,240}machine_ingress_rate_limit/s.test(machineRoute)) {
    failures.push(paths[MACHINE_ROUTE] + ': ingress limiter must wrap execute before Registry authorization')
  }

  const tenantMembershipRoute = require(paths[TENANT_MEMBERSHIPS], [
    'MACHINE_OWNER_FORBIDDEN',
  ])
  const transferStart = tenantMembershipRoute.indexOf('async fn transfer_ownership(')
  const transferEnd = tenantMembershipRoute.indexOf('\nasync fn list_users(', transferStart)
  const ownershipTransferRoute =
    transferStart >= 0 && transferEnd > transferStart
      ? tenantMembershipRoute.slice(transferStart, transferEnd)
      : ''
  for (const token of ['.registry', '.execute(', '"staff"', '"transfer_ownership"']) {
    if (!ownershipTransferRoute.includes(token)) {
      failures.push(paths[TENANT_MEMBERSHIPS] + ': ownership transfer must cross Registry staff boundary: missing ' + token)
    }
  }

  require(paths[TENANT_MEMBERSHIP_SQLITE], [
    'pub(crate) fn transfer_ownership(',
    'if target.is_machine',
    'TenantMembershipAuthorityError::MachineOwnerForbidden',
    'machine_identities',
  ])
  const tenantMembershipPostgres = require(paths[TENANT_MEMBERSHIP_POSTGRES], [
    'pub(crate) fn transfer_ownership(',
    'if target.is_machine',
    'TenantMembershipAuthorityError::MachineOwnerForbidden',
    'machine_identities',
  ])
  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(tenantMembershipPostgres)) {
    failures.push(paths[TENANT_MEMBERSHIP_POSTGRES] + ': PostgreSQL tenant membership authority must not depend on SQLite runtime types')
  }
  require(paths[TENANT_MEMBERSHIP_COMPAT], [
    '"transfer_ownership"',
    '.transfer_ownership(&actor',
    'SimulationSupport::Blocked',
  ])

  require(paths[LEGACY_API_KEYS], ['API_LEGACY_UNBOUND', 'AdminUser'])

  for (const file of [paths[SQLITE_MIGRATION], paths[POSTGRES_MIGRATION]]) {
    const migration = require(file, [
      'REFERENCES tenant_memberships(id,identity_id,tenant_id)',
      'machine_no_session_insert',
      'machine_no_session_update',
      'machine_no_platform_insert',
      'machine_no_platform_update',
      'machine_no_owner_membership_insert',
      'machine_no_owner_membership_update',
      "NEW.role='owner'",
      'machine_identities WHERE identity_id=NEW.identity_id',
      'machine_marker_no_delete',
      'machine_password_locked',
      "NEW.password_hash <> '!non-interactive'",
      'identity_id=OLD.id',
      'NEW.identity_id<>OLD.identity_id',
      'NEW.tenant_id<>OLD.tenant_id',
      "OLD.status='revoked' AND NEW.status<>'revoked'",
      'api_credential_no_delete',
      'api_client_no_delete',
    ])
    requireMachineMarkerOwnerGuard(file, migration)
    if (file === paths[POSTGRES_MIGRATION]) {
      const helperStart = migration.indexOf('CREATE OR REPLACE FUNCTION machine_identity_guard_lock(machine_identity_id TEXT)')
      const helperEnd = migration.indexOf('END; $$;', helperStart)
      const helper = helperStart >= 0 && helperEnd > helperStart ? migration.slice(helperStart, helperEnd) : ''
      if (!helper.includes('pg_advisory_xact_lock(hashtextextended(machine_identity_id, 0))')) {
        failures.push(file + ': shared machine identity lock must use PostgreSQL transaction-scoped deterministic hashing')
      }
      for (const functionName of [
        'machine_marker_validate_fn',
        'machine_no_owner_membership_insert_fn',
        'machine_no_owner_membership_update_fn',
      ]) requirePgMachineIdentityLock(migration, functionName)
    }
  }

  require(paths[INTEGRATION_MODULE], [
    'r3_simulation_effects_never_enter_production_queue_or_credentials',
    'TrapKeyStore',
    'TrapDispatcher',
    'reference_config.record_effect',
    'EXEC_SIMULATION_UNSUPPORTED',
    'RunOnceResult::Idle',
    'assert_eq!(first, replay)',
  ])

  for (const file of paths.slice(8, 11)) {
    const source = require(file, [])
    for (const token of ['/app/settings/api-keys', 'ApiKeysPage', 'api-keys', 'apiKeys']) {
      if (source.includes(token)) failures.push(file + ': legacy API-key UI remains reachable: ' + token)
    }
  }
  for (const file of retiredPaths) {
    if (read(file) !== undefined) failures.push(file + ': legacy API-key source remains')
  }
  return failures
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const failures = check(file => {
    const resolved = path.resolve(file)
    return fs.existsSync(resolved) ? fs.readFileSync(resolved, 'utf8') : undefined
  })
  if (failures.length) {
    console.error(failures.join('\n'))
    process.exitCode = 1
  } else {
    console.log('R3-C machine authority / simulation boundary: PASS')
  }
}
