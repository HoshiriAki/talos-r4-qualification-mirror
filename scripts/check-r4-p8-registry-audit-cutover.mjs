#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const PATHS = {
  registryMod: 'backend/src/registry/mod.rs',
  sqliteSink: 'backend/src/registry/audit_sink.rs',
  postgresSink: 'backend/src/registry/audit_sink_postgres.rs',
  liveQualification:
    'backend/src/registry/audit_sink_postgres_qualification_tests.rs',
  migration052:
    'backend/src/db/migrations/postgres/052_identity_authority_foundation.sql',
  assembler: 'backend/src/registry/assembler.rs',
  main: 'backend/src/main.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  auditModule: 'backend/src/application/audit_compatibility.rs',
  auditCompatibilityQualification:
    'backend/src/repositories/postgres/audit_compatibility_qualification_tests.rs',
};

function read(relative) {
  return fs.readFileSync(path.join(ROOT, relative), 'utf8');
}

export function collectP8RegistryAuditSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  );
}

export function validateP8RegistryAuditSnapshot(snapshot) {
  const errors = [];

  for (const token of [
    'mod audit_sink_postgres;',
    'mod audit_sink_postgres_qualification_tests;',
  ]) {
    if (!snapshot.registryMod.includes(token)) {
      errors.push(`Registry PostgreSQL audit wiring missing: ${token}`);
    }
  }

  if (!snapshot.sqliteSink.includes('pub struct SqliteAuditSink')) {
    errors.push('SQLite audit sink must remain explicit for local compatibility');
  }

  for (const token of [
    'pub struct PostgresAuditSink',
    "SELECT to_regclass('audit_events') IS NOT NULL",
    'INSERT INTO audit_events',
    '$7::jsonb',
    '$8::jsonb',
    '$12::jsonb',
    'tenant_membership_id',
    'platform_membership_id',
    'ALL_PLATFORM_CAPABILITIES',
    'audit append failed',
    'block_in_place',
  ]) {
    if (!snapshot.postgresSink.includes(token)) {
      errors.push(`Registry PostgreSQL audit invariant missing: ${token}`);
    }
  }

  if (/\brusqlite\b|SqliteConnectionManager|r2d2::/.test(snapshot.postgresSink)) {
    errors.push('Registry PostgreSQL audit sink must not depend on SQLite runtime types');
  }

  for (const token of [
    'CREATE TABLE IF NOT EXISTS audit_events',
    'roles_snapshot JSONB',
    'capabilities_snapshot JSONB',
    'detail_json JSONB',
    'tenant_membership_id',
    'platform_membership_id',
  ]) {
    if (!snapshot.migration052.includes(token)) {
      errors.push(`Canonical PostgreSQL audit schema invariant missing: ${token}`);
    }
  }

  for (const token of [
    'live_pg18_registry_audit_is_append_only_tenant_scoped_and_restart_durable',
    'PostgresAuditSink::new',
    'assert_eq!(stored.0, "tenant");',
    'assert_eq!(stored.1, "tenant-a");',
    'assert_eq!(stored.2, "membership-a");',
    'assert!(stored.3.contains("owner"));',
    'sink.append(event(',
    '.is_err()',
    'assert_eq!(original_action, "device.list");',
    'let recomposed =',
    'recomposed\n        .append(event(',
    'assert_eq!(count, 2);',
  ]) {
    if (!snapshot.liveQualification.includes(token)) {
      errors.push(`Registry PostgreSQL audit live proof missing: ${token}`);
    }
  }

  const compactAssembler = snapshot.assembler.replace(/\s+/g, '');
  for (const token of [
    'assemble_with_metrics_audit_sink_and_integration_module(',
    'audit_sink:Arc<dynAuditSink>',
    'Self::new_with_audit_sink_and_metrics(built.into_modules(),audit_sink,metrics)',
    'SqliteAuditSink::new(pool.clone())',
  ]) {
    if (!compactAssembler.includes(token)) {
      errors.push(`Registry audit composition seam missing: ${token}`);
    }
  }

  for (const token of [
    'crate::registry::audit_sink_postgres::PostgresAuditSink::new(pg)',
    'crate::registry::audit_sink::SqliteAuditSink::new(require_sqlite_pool(',
    '"registry audit sink"',
    'ModuleRegistry::assemble_with_metrics_audit_sink_integration_and_staff_module(',
    'registry_audit_sink',
  ]) {
    if (!snapshot.main.includes(token)) {
      errors.push(`Registry audit production composition missing: ${token}`);
    }
  }

  const localAuditCapabilityUses = snapshot.main.split(
    'crate::registry::audit_sink::SqliteAuditSink::new(require_sqlite_pool(',
  ).length - 1;
  if (localAuditCapabilityUses !== 2) {
    errors.push(
      `Registry audit local fallback must remain explicitly SQLite-capability gated in exactly both compile paths (found ${localAuditCapabilityUses}, expected 2)`,
    );
  }
  if (snapshot.main.includes(
    'crate::registry::audit_sink::SqliteAuditSink::new(pool.clone())',
  )) {
    errors.push(
      'Registry audit production composition must not restore ambient SQLite pool fallback',
    );
  }

  if (!snapshot.main.includes('if config.is_production && pg_pool.is_none()')) {
    errors.push('Registry Audit production cutover must retain the fail-closed PostgreSQL authority guard')
  }
  if (snapshot.main.includes(
    'R4-P8 PostgreSQL runtime cutover incomplete: refusing SQLite business fallback',
  )) {
    errors.push('Registry Audit cutover must not restore the retired transitional production barrier')
  }

  const compactFactory = snapshot.factory.replace(/\s+/g, '');
  if (compactFactory.includes('with_pool!(FeatureAudit)')) {
    errors.push('Registry audit module must not restore the legacy SQLite FeatureAudit runtime');
  }
  for (const token of [
    'AuditCompatibilityModule::new(',
    'AuditCompatibilityRepository::new(self.require_sqlite_pool("audit compatibility")?)',
    'with_audit_module',
  ]) {
    if (!compactFactory.includes(token.replace(/\s+/g, ''))) {
      errors.push(`Registry audit module composition missing: ${token}`);
    }
  }

  const compactDescriptors = snapshot.descriptors.replace(/\s+/g, '');
  if (!compactDescriptors.includes(
    'descriptor!("audit"=>"feature-audit",Core,ModuleActivation::Always,NONE,Audit)',
  )) {
    errors.push('Registry audit descriptor must not require the ambient SQLite pool');
  }

  if (!/\bsafe_audit_detail_json\(&detail\)/.test(snapshot.auditModule)) {
    errors.push('Registry audit compatibility module invariant missing: safe_audit_detail_json(&detail)');
  }
  for (const token of [
    '.append_audit_log(&entry)',
    '.list_audit_logs(ctx.data_scope().tenant_id().as_str(), &input)',
  ]) {
    if (!snapshot.auditModule.includes(token)) {
      errors.push(`Registry audit compatibility module invariant missing: ${token}`);
    }
  }

  for (const token of [
    'live_pg18_audit_compatibility_preserves_legacy_view_and_canonical_store',
    'assert_eq!(foreign.total, 0);',
    'assert!(foreign.logs.is_empty());',
    'FROM audit_events WHERE id = \'audit-compat-1\'',
  ]) {
    if (!snapshot.auditCompatibilityQualification.includes(token)) {
      errors.push(`Registry audit compatibility live proof missing: ${token}`);
    }
  }

  return errors;
}

function main() {
  const errors = validateP8RegistryAuditSnapshot(
    collectP8RegistryAuditSnapshot(),
  );
  if (errors.length > 0) {
    console.error('R4-P8 Registry Audit cutover gate failed:');
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log('R4-P8 Registry Audit PostgreSQL parity gate passed.');
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main();
}
