#!/usr/bin/env node

import assert from 'node:assert/strict';
import {
  collectP8RegistryAuditSnapshot,
  validateP8RegistryAuditSnapshot,
} from './check-r4-p8-registry-audit-cutover.mjs';

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectP8RegistryAuditSnapshot());
  mutate(snapshot);
  const errors = validateP8RegistryAuditSnapshot(snapshot);
  assert.ok(errors.length > 0, `${name}: mutation unexpectedly passed`);
  assert.ok(
    errors.some((error) => error.includes(needle)),
    `${name}: expected ${JSON.stringify(needle)}, got ${JSON.stringify(errors)}`,
  );
}

const baseline = validateP8RegistryAuditSnapshot(
  collectP8RegistryAuditSnapshot(),
);
assert.deepEqual(
  baseline,
  [],
  `baseline must pass before Registry Audit mutations: ${baseline.join('; ')}`,
);

expectFailure(
  'PostgreSQL audit sink loses JSONB role snapshot',
  (snapshot) => {
    snapshot.postgresSink = snapshot.postgresSink.replaceAll(
      '$7::jsonb',
      '$7',
    );
  },
  '$7::jsonb',
);

expectFailure(
  'PostgreSQL audit sink loses tenant membership projection',
  (snapshot) => {
    snapshot.postgresSink = snapshot.postgresSink.replaceAll(
      'tenant_membership_id',
      'REMOVED_TENANT_MEMBERSHIP',
    );
  },
  'tenant_membership_id',
);

expectFailure(
  'PostgreSQL audit sink gains SQLite coupling',
  (snapshot) => {
    snapshot.postgresSink += '\nuse rusqlite::Connection;\n';
  },
  'must not depend on SQLite',
);

expectFailure(
  'Live audit proof stops asserting tenant isolation',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(stored.1, "tenant-a");',
      'assert!(true);',
    );
  },
  'stored.1',
);

expectFailure(
  'Live audit proof stops checking canonical role snapshot',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert!(stored.3.contains("owner"));',
      'assert!(true);',
    );
  },
  'stored.3',
);

expectFailure(
  'Live audit proof stops asserting append-only duplicate rejection',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'assert_eq!(original_action, "device.list");',
      'assert!(true);',
    );
  },
  'original_action',
);

expectFailure(
  'Live audit proof stops asserting restart durability',
  (snapshot) => {
    snapshot.liveQualification = snapshot.liveQualification.replaceAll(
      'let recomposed =',
      'let removed_recomposition =',
    );
  },
  'let recomposed =',
);

expectFailure(
  'Registry assembly loses injected audit sink seam',
  (snapshot) => {
    snapshot.assembler = snapshot.assembler.replaceAll(
      'assemble_with_metrics_audit_sink_and_integration_module(',
      'removed_audit_sink_composition(',
    );
  },
  'assemble_with_metrics_audit_sink_and_integration_module',
);

expectFailure(
  'Production Registry audit composition falls back to SQLite',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'crate::registry::audit_sink_postgres::PostgresAuditSink::new(pg)',
      'crate::registry::audit_sink::SqliteAuditSink::new(pool.clone())',
    );
  },
  'PostgresAuditSink::new(pg)',
);

expectFailure(
  'one Registry audit local path bypasses explicit SQLite capability',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'crate::registry::audit_sink::SqliteAuditSink::new(require_sqlite_pool(',
      'crate::registry::audit_sink::SqliteAuditSink::new(legacy_sqlite_pool(',
    );
  },
  'exactly both compile paths',
);

expectFailure(
  'Production Registry audit restores ambient SQLite pool fallback',
  (snapshot) => {
    snapshot.main +=
      '\n// crate::registry::audit_sink::SqliteAuditSink::new(pool.clone())\n';
  },
  'must not restore ambient SQLite pool fallback',
);

expectFailure(
  'Production Registry bypasses injected audit sink assembly',
  (snapshot) => {
    snapshot.main = snapshot.main.replace(
      'ModuleRegistry::assemble_with_metrics_audit_sink_integration_and_staff_module(',
      'ModuleRegistry::assemble_with_metrics(',
    );
  },
  'assemble_with_metrics_audit_sink_integration_and_staff_module',
);

expectFailure(
  'Registry audit module restores legacy FeatureAudit runtime',
  (snapshot) => {
    snapshot.factory += '\n// with_pool!(FeatureAudit)\n';
  },
  'legacy SQLite FeatureAudit runtime',
);

expectFailure(
  'Registry audit compatibility loses secret redaction',
  (snapshot) => {
    snapshot.auditModule = snapshot.auditModule.replaceAll(
      'safe_audit_detail_json(&detail)',
      'redaction_removed(&detail)',
    );
  },
  'safe_audit_detail_json',
);

expectFailure(
  'Registry audit descriptor restores SQLite requirement',
  (snapshot) => {
    snapshot.descriptors = snapshot.descriptors.replace(
      'descriptor!("audit" => "feature-audit", Core, ModuleActivation::Always, NONE, Audit)',
      'descriptor!("audit" => "feature-audit", Core, ModuleActivation::Always, SQLITE, Audit)',
    );
  },
  'descriptor must not require',
);

expectFailure(
  'Registry Audit production guard disappears',
  snapshot => {
    snapshot.main = snapshot.main.replace(
      'if config.is_production && pg_pool.is_none()',
      'if false',
    )
  },
  'fail-closed PostgreSQL authority guard',
)

expectFailure(
  'Registry Audit gate restores transitional production barrier',
  snapshot => {
    snapshot.main +=
      '\nconst RETIRED_P8_BARRIER: &str = "R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback";\n'
  },
  'must not restore the retired transitional production barrier',
)

console.log('R4-P8 Registry Audit cutover mutation tests passed.');
