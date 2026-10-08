import fs from 'node:fs'
import assert from 'node:assert/strict'
import test from 'node:test'
import { check, paths, retiredPaths } from './check-r3-machine-simulation-boundary.mjs'

const sources = new Map(
  [...paths, ...retiredPaths].map(p => [
    p,
    fs.existsSync(p) ? fs.readFileSync(p, 'utf8').replaceAll('\r\n', '\n') : undefined,
  ]),
)

test('current source satisfies R3-C boundary', () =>
  assert.deepEqual(check(p => sources.get(p)), []))

const SERVICE = 0
const REGISTRY = 1
const MACHINE_ROUTE = 2
const SQLITE_MIGRATION = 5
const POSTGRES_MIGRATION = 6
const MACHINE_SHARED = 11
const MACHINE_SQLITE = 12
const MACHINE_POSTGRES = 13
const MACHINE_DISPATCH = 14
const TENANT_MEMBERSHIP_SQLITE = 15
const TENANT_MEMBERSHIP_POSTGRES = 16
const TENANT_MEMBERSHIP_COMPAT = 17

for (const [file, token] of [
  [paths[SERVICE], 'authorize_with_repository'],
  [paths[SERVICE], 'credential_lifecycle_with_repository'],
  [paths[MACHINE_SHARED], '"two_fa" | "consent" | "deletion" | "user_settings"'],
  [paths[MACHINE_SQLITE], 'ct_eq(expected_hash.as_bytes())'],
  [paths[MACHINE_SQLITE], 'membership_status == "active"'],
  [paths[MACHINE_SQLITE], 'tenant_id != requested_tenant'],
  [paths[MACHINE_SQLITE], '!scope_allowed'],
  [paths[MACHINE_SQLITE], 'used >= rate_limit_rpm'],
  [paths[MACHINE_SQLITE], 'TransactionBehavior::Immediate'],
  [paths[MACHINE_POSTGRES], 'ct_eq(expected_hash.as_bytes())'],
  [paths[MACHINE_POSTGRES], 'membership_status == "active"'],
  [paths[MACHINE_POSTGRES], 'tenant_id != requested_tenant'],
  [paths[MACHINE_POSTGRES], '!scope_allowed'],
  [paths[MACHINE_POSTGRES], 'used >= rate_limit_rpm'],
  [paths[MACHINE_POSTGRES], 'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE'],
  [paths[MACHINE_POSTGRES], 'FOR UPDATE OF k,c,m,i'],
  [paths[MACHINE_POSTGRES], 'GREATEST(api_usage_windows.window_start,EXCLUDED.window_start)'],
  [paths[MACHINE_DISPATCH], 'Postgres(PostgresMachineAuthorityRepository)'],
  [paths[TENANT_MEMBERSHIP_SQLITE], 'TenantMembershipAuthorityError::MachineOwnerForbidden'],
  [paths[TENANT_MEMBERSHIP_SQLITE], 'if target.is_machine'],
  [paths[TENANT_MEMBERSHIP_POSTGRES], 'TenantMembershipAuthorityError::MachineOwnerForbidden'],
  [paths[TENANT_MEMBERSHIP_POSTGRES], 'if target.is_machine'],
  [paths[TENANT_MEMBERSHIP_COMPAT], '"transfer_ownership"'],
  [paths[TENANT_MEMBERSHIP_COMPAT], 'SimulationSupport::Blocked'],
  [paths[REGISTRY], 'ActorIdentity::with_authority(auth.identity_id, auth.authority)'],
  [paths[MACHINE_ROUTE], 'header::COOKIE'],
  [paths[MACHINE_ROUTE], 'post(execute).route_layer'],
  [paths[3], 'MACHINE_OWNER_FORBIDDEN'],
  [paths[3], '.registry'],
  [paths[3], '"transfer_ownership"'],
  [paths[SQLITE_MIGRATION], 'NEW.tenant_id<>OLD.tenant_id'],
  [paths[POSTGRES_MIGRATION], 'NEW.tenant_id<>OLD.tenant_id'],
  [paths[SQLITE_MIGRATION], 'machine_no_owner_membership_update'],
  [paths[POSTGRES_MIGRATION], 'machine_no_owner_membership_update'],
  [
    paths[SQLITE_MIGRATION],
    "EXISTS (SELECT 1 FROM tenant_memberships WHERE identity_id=NEW.identity_id AND role='owner')",
  ],
  [
    paths[POSTGRES_MIGRATION],
    "EXISTS (SELECT 1 FROM tenant_memberships WHERE identity_id=NEW.identity_id AND role='owner')",
  ],
  [paths[7], 'RunOnceResult::Idle'],
]) {
  test('reject removal: ' + file + ' ' + token, () => {
    assert.ok(sources.get(file).includes(token), 'mutation must hit live source')
    const mutated = new Map(sources)
    mutated.set(file, mutated.get(file).replaceAll(token, 'MUTATED'))
    assert.ok(check(p => mutated.get(p)).length > 0)
  })
}

test('reject SQL persistence returning to machine policy facade', () => {
  const mutated = new Map(sources)
  mutated.set(
    paths[SERVICE],
    mutated.get(paths[SERVICE]) + '\n// INSERT INTO api_clients\n',
  )
  assert.ok(check(p => mutated.get(p)).length > 0)
})

test('reject SQLite coupling entering PostgreSQL tenant membership authority', () => {
  const mutated = new Map(sources)
  mutated.set(
    paths[TENANT_MEMBERSHIP_POSTGRES],
    mutated.get(paths[TENANT_MEMBERSHIP_POSTGRES]) + '\nuse rusqlite::Connection;\n',
  )
  assert.ok(check(p => mutated.get(p)).length > 0)
})

test('reject SQLite coupling entering PostgreSQL machine authority', () => {
  const mutated = new Map(sources)
  mutated.set(
    paths[MACHINE_POSTGRES],
    mutated.get(paths[MACHINE_POSTGRES]) + '\nuse rusqlite::Connection;\n',
  )
  assert.ok(check(p => mutated.get(p)).length > 0)
})

for (const file of retiredPaths) {
  test('reject restoration: ' + file, () => {
    const mutated = new Map(sources)
    mutated.set(file, 'legacy API-key source')
    assert.ok(check(p => mutated.get(p)).length > 0)
  })
}

for (const file of paths.slice(8, 11)) {
  test('reject legacy API-key reachability: ' + file, () => {
    const mutated = new Map(sources)
    mutated.set(file, mutated.get(file) + "\n'/app/settings/api-keys'\n")
    assert.ok(check(p => mutated.get(p)).length > 0)
  })
}

function replaceInPgFunction(source, functionName, needle, replacement) {
  const start = source.indexOf('CREATE OR REPLACE FUNCTION ' + functionName + '()')
  assert.notEqual(start, -1, 'mutation must find ' + functionName)
  const end = source.indexOf('END; $$;', start)
  assert.notEqual(end, -1, 'mutation must find end of ' + functionName)
  const body = source.slice(start, end)
  assert.ok(body.includes(needle), 'mutation must hit ' + functionName)
  return source.slice(0, start) + body.replace(needle, replacement) + source.slice(end)
}

for (const functionName of [
  'machine_marker_validate_fn',
  'machine_no_owner_membership_insert_fn',
  'machine_no_owner_membership_update_fn',
]) {
  test('reject removal of shared PostgreSQL identity lock: ' + functionName, () => {
    const mutated = new Map(sources)
    mutated.set(
      paths[POSTGRES_MIGRATION],
      replaceInPgFunction(
        mutated.get(paths[POSTGRES_MIGRATION]),
        functionName,
        'PERFORM machine_identity_guard_lock(NEW.identity_id);',
        '/* MUTATED shared identity lock removed */',
      ),
    )
    assert.ok(check(p => mutated.get(p)).length > 0)
  })
}

test('reject PostgreSQL authority path using a different identity lock helper', () => {
  const mutated = new Map(sources)
  mutated.set(
    paths[POSTGRES_MIGRATION],
    replaceInPgFunction(
      mutated.get(paths[POSTGRES_MIGRATION]),
      'machine_no_owner_membership_update_fn',
      'machine_identity_guard_lock(NEW.identity_id)',
      'different_machine_identity_guard_lock(NEW.identity_id)',
    ),
  )
  assert.ok(check(p => mutated.get(p)).length > 0)
})

test('reject ingress limiter moved after Registry execution', () => {
  const mutated = new Map(sources)
  const route = mutated.get(paths[MACHINE_ROUTE])
  mutated.set(
    paths[MACHINE_ROUTE],
    route
      .replace('post(execute).route_layer', 'post(execute)')
      .replace(
        'let result = state.registry.execute_machine_with_repository(',
        '/* moved after Registry */ let result = state.registry.execute_machine_with_repository(',
      ),
  )
  assert.ok(check(p => mutated.get(p)).length > 0)
})
