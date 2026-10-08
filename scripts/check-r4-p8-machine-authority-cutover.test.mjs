import assert from 'node:assert/strict'
import {
  collectP8MachineAuthoritySnapshot,
  validateP8MachineAuthoritySnapshot,
} from './check-r4-p8-machine-authority-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8MachineAuthoritySnapshot())
  mutate(snapshot)
  const errors = validateP8MachineAuthoritySnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP8MachineAuthoritySnapshot(
  collectP8MachineAuthoritySnapshot(),
)
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Machine Authority mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'Machine dispatch loses PostgreSQL backend',
  snapshot => {
    snapshot.dispatch = snapshot.dispatch.replaceAll(
      'Postgres(PostgresMachineAuthorityRepository)',
      'REMOVED_POSTGRES_MACHINE_BACKEND',
    )
  },
  'Postgres(PostgresMachineAuthorityRepository)',
)

expectFailure(
  'Machine policy facade regains SQL ownership',
  snapshot => {
    snapshot.service += '\n// INSERT INTO api_clients\n'
  },
  'must not own persistence token',
)

expectFailure(
  'PostgreSQL machine writes lose serialization',
  snapshot => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'SET TRANSACTION ISOLATION LEVEL SERIALIZABLE',
      'SET TRANSACTION ISOLATION LEVEL READ COMMITTED',
    )
  },
  'SERIALIZABLE',
)

expectFailure(
  'PostgreSQL authorize loses credential/client row locks',
  snapshot => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'FOR UPDATE OF k,c,m,i',
      'REMOVED_MACHINE_AUTH_LOCKS',
    )
  },
  'FOR UPDATE OF k,c,m,i',
)

expectFailure(
  'PostgreSQL authorize loses durable RPM state',
  snapshot => {
    snapshot.postgresRepository = snapshot.postgresRepository.replaceAll(
      'api_usage_windows',
      'REMOVED_USAGE_WINDOWS',
    )
  },
  'api_usage_windows',
)

expectFailure(
  'PostgreSQL machine repository gains SQLite coupling',
  snapshot => {
    snapshot.postgresRepository += '\nuse rusqlite::Connection;\n'
  },
  'must not depend on SQLite',
)

expectFailure(
  'Live machine proof stops checking machine session exclusion',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert!(forbidden_session.is_err());',
      'assert!(true);',
    )
  },
  'forbidden_session',
)

expectFailure(
  'Live machine proof stops checking restart-safe rate budget',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'let recomposed =',
      'let removed_recomposition =',
    )
  },
  'let recomposed =',
)

expectFailure(
  'Live machine proof stops checking exact machine audit role',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert!(!admitted_roles.contains("admin"));',
      'assert!(true);',
    )
  },
  'admitted_roles',
)

expectFailure(
  'Inherited R3 machine gate stops checking PostgreSQL repository',
  snapshot => {
    snapshot.r3Gate = snapshot.r3Gate.replaceAll(
      'paths[MACHINE_POSTGRES]',
      'paths[MACHINE_SQLITE]',
    )
  },
  'Inherited R3 machine boundary',
)

expectFailure(
  'SQLite compatibility Registry machine entry disappears',
  snapshot => {
    snapshot.registry = snapshot.registry.replaceAll(
      'pool: &r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>',
      'pool: &REMOVED_SQLITE_MACHINE_POOL',
    )
  },
  'SQLite compatibility Registry machine entry',
)

expectFailure(
  'Machine route bypasses composition-selected repository',
  snapshot => {
    snapshot.route = snapshot.route.replace(
      'state.registry.execute_machine_with_repository(',
      'state.registry.execute_machine(',
    )
  },
  'execute_machine_with_repository',
)

expectFailure(
  'Machine AppState repository handle becomes public',
  snapshot => {
    snapshot.state = snapshot.state.replace(
      'machine_authority_repository: MachineAuthorityRepository',
      'pub machine_authority_repository: MachineAuthorityRepository',
    )
  },
  'must remain private',
)

expectFailure(
  'Machine AppState bundle stops consuming selected authority',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      /AppStateRepositories::new\(\s*audit_compatibility_repository,\s*auth_security_repository,\s*identity_authority_repository,\s*machine_authority_repository,/,
      'AppStateRepositories::new(audit_compatibility_repository, auth_security_repository, identity_authority_repository, MachineAuthorityRepository::new(pool.clone()),',
    )
  },
  'AppState bundle must consume the selected Machine repository',
)

expectFailure(
  'Production machine composition falls back to SQLite',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'MachineAuthorityRepository::postgres(pg.clone())',
      'MachineAuthorityRepository::new(pool.clone())',
    )
  },
  'MachineAuthorityRepository::postgres(pg.clone())',
)

expectFailure(
  'Machine production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Machine gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 Machine Authority cutover mutation tests passed.')
