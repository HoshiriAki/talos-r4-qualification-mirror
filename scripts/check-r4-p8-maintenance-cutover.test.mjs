#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectP8MaintenanceSnapshot,
  validateP8MaintenanceSnapshot,
} from './check-r4-p8-maintenance-cutover.mjs'

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8MaintenanceSnapshot())
  mutate(snapshot)
  const errors = validateP8MaintenanceSnapshot(snapshot)
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`)
  assert.ok(
    errors.some(error => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  )
}

const baseline = validateP8MaintenanceSnapshot(collectP8MaintenanceSnapshot())
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Maintenance mutations: ${baseline.join('; ')}`,
)

expectFailure(
  'Production maintenance composition falls back to SQLite',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'MaintenanceCompatibilityRepository::postgres(pg)',
      'MaintenanceCompatibilityRepository::new(pool.clone())',
    )
  },
  'MaintenanceCompatibilityRepository::postgres(pg)',
)

expectFailure(
  'one local Maintenance path bypasses explicit sqlite capability',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      /require_sqlite_pool\(\s*"maintenance compatibility"/,
      'legacy_sqlite_pool("maintenance compatibility"',
    )
  },
  'exactly both compile paths',
)

expectFailure(
  'Maintenance keeps capability token but severs repository binding',
  snapshot => {
    const source = snapshot.main
    snapshot.main = snapshot.main.replace(
      'MaintenanceCompatibilityRepository::new(require_sqlite_pool(',
      'MaintenanceCompatibilityRepository::new(legacy_pool.clone());\n        let _ = require_sqlite_pool(',
    )
    assert.notEqual(snapshot.main, source, 'maintenance binding mutation anchor missing')
  },
  'bind the explicit SQLite capability directly',
)

expectFailure(
  'Maintenance restores implicit shared SQLite pool',
  snapshot => {
    snapshot.main += '\n// MaintenanceCompatibilityRepository::new(pool.clone())\n'
  },
  'implicit shared SQLite pool fallback',
)

expectFailure(
  'Legacy maintenance adapter regains a SQLite pool',
  snapshot => {
    snapshot.workers = snapshot.workers.replace(
      'maintenance_repository: MaintenanceCompatibilityRepository,',
      'pool: Pool<SqliteConnectionManager>,',
    )
  },
  'must not own a SQLite persistence pool',
)

expectFailure(
  'PostgreSQL overdue seeding loses idempotent insert',
  snapshot => {
    snapshot.postgresRepository = snapshot.postgresRepository.replace(
      'ON CONFLICT (id) DO NOTHING',
      'RETURNING id',
    )
  },
  'ON CONFLICT (id) DO NOTHING',
)

expectFailure(
  'PostgreSQL overdue discovery loses tenant-aligned device join',
  snapshot => {
    snapshot.postgresRepository = snapshot.postgresRepository.replace(
      'o.tenant_id=od.tenant_id',
      'TRUE',
    )
  },
  'o.tenant_id=od.tenant_id',
)

expectFailure(
  'Live proof stops checking tenant-a work-task ownership',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replace(
      'assert_eq!(row_a.get::<String, _>(1), "tenant-a");',
      'assert!(true);',
    )
  },
  'tenant-a',
)

expectFailure(
  'Live proof stops checking recomposed idempotency',
  snapshot => {
    snapshot.liveQualification = snapshot.liveQualification.replace(
      'let recomposed = MaintenanceCompatibilityRepository::postgres',
      'let recomposed_removed = MaintenanceCompatibilityRepository::postgres',
    )
  },
  'let recomposed =',
)

expectFailure(
  'Exact-head stops running maintenance PostgreSQL qualification',
  snapshot => {
    snapshot.exactHead = snapshot.exactHead.replace(
      'live_pg18_maintenance_compatibility_preserves_status_sync_overdue_tenant_and_idempotency',
      'removed_maintenance_pg18_qualification',
    )
  },
  'Exact-head qualification',
)

expectFailure(
  'Maintenance production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Maintenance gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 Maintenance compatibility cutover mutation tests passed.')
