#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  route: 'backend/src/routes/tasks.rs',
  application: 'backend/src/application/work_task_compatibility.rs',
  repository: 'backend/src/repositories/work_task.rs',
  dispatch: 'backend/src/repositories/work_task_dispatch.rs',
  postgres: 'backend/src/repositories/work_task_postgres.rs',
  provider: 'backend/src/repositories/contracts/provider.rs',
  descriptors: 'backend/src/registry/descriptors.rs',
  factory: 'backend/src/registry/factory.rs',
  postgresMod: 'backend/src/repositories/postgres/mod.rs',
  error: 'backend/src/error.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectWorkTaskSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateWorkTaskSnapshot(snapshot) {
  const errors = []

  for (const token of [
    'TrustedTenantUser',
    '.registry',
    '"work_task"',
    '"list_tasks"',
    '"task_action"',
    '"send_to_pc"',
    'tenant_user.context()',
  ]) {
    if (!snapshot.route.includes(token)) errors.push('Work task HTTP cutover missing: ' + token)
  }
  for (const forbidden of ['state.pool', 'rusqlite', 'AuthUser', 'FROM work_tasks', 'UPDATE work_tasks']) {
    if (snapshot.route.includes(forbidden)) {
      errors.push('Work task route must not retain direct persistence/legacy authority: ' + forbidden)
    }
  }

  for (const token of [
    'pub(crate) struct WorkTaskCompatibilityModule',
    'repository_provider: Arc<dyn RepositoryProvider>',
    '.bind(ctx)',
    '.work_tasks()',
    'AccessRequirement::Authenticated',
    'SimulationSupport::Blocked',
  ]) {
    if (!snapshot.application.includes(token)) errors.push('Work task Registry authority missing: ' + token)
  }

  for (const token of [
    'WHERE tenant_id = ?',
    'AND id = ?',
    'AND version = ?',
    'version = version + 1',
  ]) {
    if (!snapshot.repository.includes(token)) errors.push('SQLite work task scope/CAS missing: ' + token)
  }
  for (const token of [
    'FROM work_tasks WHERE tenant_id = ',
    'WHERE tenant_id = $1 AND id = $2',
    'WHERE tenant_id = $3 AND id = $4 AND version = $5',
    'WHERE tenant_id = $2 AND id = $3 AND version = $4',
    'RETURNING id, version::BIGINT AS version',
  ]) {
    if (!snapshot.postgres.includes(token)) errors.push('PostgreSQL work task scope/CAS missing: ' + token)
  }

  for (const token of [
    'ScopedWorkTaskRepository',
    'PostgresWorkTaskRepository',
    'SqliteWorkTaskRepository',
  ]) {
    if (!snapshot.dispatch.includes(token)) errors.push('Work task backend dispatch missing: ' + token)
  }
  if (!snapshot.provider.includes('pub fn work_tasks(&self) -> ScopedWorkTaskRepository')) {
    errors.push('ScopedRepositories must expose work_tasks through the bound repository session')
  }

  for (const token of ['WorkTask', '"work_task"']) {
    if (!snapshot.descriptors.includes(token)) errors.push('Work task descriptor missing: ' + token)
  }
  for (const token of [
    '(WorkTaskCompatibilityModule, WorkTask, "work_task")',
    'WorkTaskCompatibilityModule::new(repository_provider.clone())',
    'insert(work_task_compatibility_built)',
  ]) {
    if (!snapshot.factory.includes(token)) errors.push('Work task composition missing: ' + token)
  }

  if (!snapshot.postgresMod.includes('mod work_task_qualification_tests;')) {
    errors.push('PostgreSQL work task qualification module is not registered')
  }

  for (const token of [
    '"BIZ_WORK_TASK_NOT_FOUND" => AppError::NotFound(message)',
    '"BIZ_WORK_TASK_CONFLICT" => AppError::Conflict(message)',
    '"AUTH_WORK_TASK_FORBIDDEN" => AppError::Forbidden',
    'AppError::BadRequest(message)',
  ]) {
    if (!snapshot.error.includes(token)) errors.push('Work task HTTP compatibility mapping missing: ' + token)
  }
  for (const coded of [
    '"BIZ_WORK_TASK_NOT_FOUND" => AppError::CodedNotFound',
    '"BIZ_WORK_TASK_CONFLICT" => AppError::CodedConflict',
    '"AUTH_WORK_TASK_FORBIDDEN" => AppError::CodedForbidden',
  ]) {
    if (snapshot.error.includes(coded)) errors.push('Work task cutover must preserve legacy public HTTP error codes: ' + coded)
  }
  for (const token of [
    'node scripts/check-r4-p8-work-task-cutover.test.mjs',
    'node scripts/check-r4-p8-work-task-cutover.mjs',
    'live_pg18_work_task_cutover_preserves_tenant_scope_version_and_recomposition',
  ]) {
    if (!snapshot.workflow.includes(token)) errors.push('Exact-head work task qualification missing: ' + token)
  }

  return errors
}

function main() {
  const errors = validateWorkTaskSnapshot(collectWorkTaskSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 work task cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 work task cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
