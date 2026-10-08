#!/usr/bin/env node

import assert from 'node:assert/strict';
import {
  collectProductionPersistenceSnapshot,
  validateProductionPersistenceSnapshot,
} from './check-r4-p8-production-persistence.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectProductionPersistenceSnapshot());
  mutate(snapshot);
  const errors = validateProductionPersistenceSnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baseline = validateProductionPersistenceSnapshot(
  collectProductionPersistenceSnapshot(),
);
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before P8 mutations: ${baseline.join('; ')}`,
);

expectFailure(
  'production default falls back to sqlite',
  (snapshot) => {
    snapshot.config = snapshot.config.replace(
      'let default_backend = if is_production { "postgres" } else { "sqlite" }',
      'let default_backend = "sqlite"',
    );
  },
  'database profile contract missing',
);

expectFailure(
  'production sqlite profile becomes accepted',
  (snapshot) => {
    snapshot.config = snapshot.config.replace(
      'return Err("production requires the PostgreSQL 18 database profile");',
      'return Ok(profile);',
    );
  },
  'database profile contract missing',
);

expectFailure(
  'composition root reads DB_BACKEND independently',
  (snapshot) => {
    snapshot.rawBackendReaders.push('backend/src/main.rs');
  },
  'DB_BACKEND authority escaped config.rs',
);

expectFailure(
  'defensive backend reader stops rejecting postgres',
  (snapshot) => {
    const [file] = Object.keys(snapshot.defensiveBackendReaders);
    assert.ok(file, 'expected at least one transitional defensive backend reader');
    snapshot.defensiveBackendReaders[file] = snapshot.defensiveBackendReaders[file].replace(
      'SYS_BACKEND_UNSUPPORTED',
      'SYS_BACKEND_ALLOWED',
    );
  },
  'defensive DB_BACKEND reader no longer fails closed',
);

expectFailure(
  'transitional postgres barrier is restored',
  (snapshot) => {
    snapshot.main +=
      '\nfn forbidden_barrier() { let _ = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback"; }\n';
  },
  'transitional PostgreSQL admission barrier must be removed',
);

expectFailure(
  'sqlite pool opens before postgres authority',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'let mut config = AppConfig::from_env();',
      'let mut config = AppConfig::from_env();\n    let _early = create_pool(&config.db_path)?;',
    );
  },
  'exactly one SQLite pool creation site',
);

expectFailure(
  'production postgres authority guard disappears',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    );
  },
  'trusted composition root missing P8 admission token',
);

expectFailure(
  'postgres absence manufactures sqlite fallback',
  (snapshot) => {
    snapshot.main +=
      '\nfn forbidden_fallback() { let _ = pg_pool.is_none().then(|| create_pool(&config.db_path)); }\n';
  },
  'must not infer SQLite fallback',
);

expectFailure(
  'new sqlite consumer is not classified',
  (snapshot) => {
    snapshot.candidates.push({
      file: 'backend/new-production-surface.rs',
      ruleId: null,
      category: null,
      status: null,
    });
  },
  'unclassified SQLite persistence consumers',
);

expectFailure(
  'active inventory rule disappears from evidence',
  (snapshot) => {
    const active = snapshot.candidates.find((candidate) => candidate.ruleId);
    assert.ok(active, 'expected at least one active SQLite persistence candidate');
    snapshot.inventory = snapshot.inventory.replaceAll(`\`${active.ruleId}\``, '`removed-rule`');
  },
  'inventory does not document active classification rule',
);

console.log('R4-P8 production persistence mutation tests passed.');
