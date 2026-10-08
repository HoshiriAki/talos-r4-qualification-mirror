#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PATHS = {
  application: 'backend/src/application/notify_compatibility.rs',
  repositories: 'backend/src/repositories/mod.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  sqlite: 'backend/src/repositories/notification.rs',
  dispatch: 'backend/src/repositories/notification_dispatch.rs',
  pgRead: 'backend/src/repositories/notification_postgres_read.rs',
  pgMutation: 'backend/src/repositories/notification_postgres_mutation.rs',
  factory: 'backend/src/registry/factory.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  pgTest: 'backend/src/repositories/postgres/notification_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectNotifyAuthoritySnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateNotifyAuthoritySnapshot(snapshot) {
  const errors = []
  const app = compact(snapshot.application)
  const factory = compact(snapshot.factory)
  const descriptors = compact(snapshot.descriptors)

  for (const token of [
    'NotifyCompatibilityModule',
    'self.repository_provider.bind(ctx)',
    '.notifications()',
    '.template(',
    '.insert_log(',
    '.list_in_app(',
    '.mark_read(',
    '.list_templates(',
    '.upsert_template(',
    '.enabled_channels(',
    '.order_context(',
    'FeatureNotification::new().metadata()',
    'FeatureNotification::new().commands()',
    'FeatureNotification::new().schema()',
  ]) {
    if (!app.includes(compact(token))) {
      errors.push('Notify compatibility authority missing: ' + token)
    }
  }

  for (const token of [
    'mod notification;',
    'mod notification_dispatch;',
    'mod notification_postgres;',
    'mod notification_postgres_mutation;',
    'mod notification_postgres_read;',
    'pub use notification_dispatch::ScopedNotificationRepository;',
  ]) {
    if (!snapshot.repositories.includes(token)) {
      errors.push('Notification repository wiring missing: ' + token)
    }
  }

  if (!snapshot.provider.includes("pub fn notifications(&self) -> ScopedNotificationRepository<'_>")) {
    errors.push('ScopedRepositories must expose Notification authority')
  }
  if (!snapshot.dispatch.includes('PostgresNotificationRepository')
      || !snapshot.dispatch.includes('SqliteNotificationRepository')) {
    errors.push('Notification backend dispatch is incomplete')
  }

  for (const token of [
    'WHERE tenant_id=?1 AND event_type=?2 AND channel=?3',
    "WHERE tenant_id=?1 AND channel='in_app' AND recipient=?2",
    'WHERE tenant_id=?2 AND id=?3 AND recipient=?4',
    'FROM orders o',
    'LEFT JOIN customers c',
    "COALESCE(c.display_name,'')",
    'o.endDate',
    'WHERE o.tenant_id=?1 AND o.id=?2',
    'sqlite_notification_authority_preserves_scope_templates_messages_and_order_hydration',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite Notification authority invariant missing: ' + token)
    }
  }

  for (const token of [
    'tenant_id=$1 AND event_type=$2 AND channel=$3',
    "tenant_id=$1 AND channel='in_app' AND recipient=$2",
    'FROM orders o',
    'LEFT JOIN customers c',
    "COALESCE(c.display_name,'') AS customer_name",
    'o.enddate AS due_date',
    'o.tenant_id=$1 AND o.id=$2',
    '(is_enabled <> 0) AS is_enabled',
  ]) {
    if (!snapshot.pgRead.includes(token)) {
      errors.push('PostgreSQL Notification read invariant missing: ' + token)
    }
  }

  for (const token of [
    'pg_write_serializable_repository',
    'INSERT INTO notification_log',
    'UPDATE notification_log SET read_at=$1',
    'UPDATE notification_templates',
    'INSERT INTO notification_templates',
    'tenant_id',
  ]) {
    if (!snapshot.pgMutation.includes(token)) {
      errors.push('PostgreSQL Notification write invariant missing: ' + token)
    }
  }

  if (!factory.includes('NotifyCompatibilityModule::new(repository_provider.clone())')) {
    errors.push('Registry Notify provider composition missing')
  }
  if (snapshot.factory.includes('with_pool!(FeatureNotification)')) {
    errors.push('Registry Notify restored direct SQLite construction')
  }
  if (!descriptors.includes('descriptor!("notify",Business,ModuleActivation::Always,NONE,Notify)')) {
    errors.push('Notify Registry descriptor must not require SQLite')
  }

  for (const stale of ['recipientName', 'estimatedReturnDate', 'recipientname', 'estimatedreturndate']) {
    if (snapshot.sqlite.includes(stale) || snapshot.pgRead.includes(stale) || snapshot.pgTest.includes(stale)) {
      errors.push('Notification authority restored nonexistent legacy order hydration column: ' + stale)
    }
  }

  if (!snapshot.postgresMod.includes('mod notification_qualification_tests;')) {
    errors.push('PostgreSQL Notification qualification module is not registered')
  }
  for (const token of [
    'live_pg18_notification_authority_preserves_scope_templates_messages_order_hydration_and_recomposition',
    'Err(NotificationMutationError::TemplateNotFound)',
    'persisted.total, 1',
    'persisted.unread_count, 0',
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 Notification authority evidence missing: ' + token)
    }
  }
  if (!compact(snapshot.pgTest).includes('.order_context("notify-order-b")?.is_none()')) {
    errors.push('PG18 Notification authority evidence missing: cross-tenant order hydration rejection')
  }

  for (const token of [
    'node scripts/check-r4-p8-notify-authority.test.mjs',
    'node scripts/check-r4-p8-notify-authority.mjs',
    'live_pg18_notification_authority_preserves_scope_templates_messages_order_hydration_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Notification qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateNotifyAuthoritySnapshot(collectNotifyAuthoritySnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 Notify authority gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Notify authority gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
