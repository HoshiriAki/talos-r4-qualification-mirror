#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export const inheritedBoundaryChecks = [
  'scripts/check-sp03c-trusted-tenant-resolution-boundary.test.mjs',
  'scripts/check-sp03c-trusted-tenant-resolution-boundary.mjs',
  'scripts/check-sp08-scoped-repository-boundary.test.mjs',
  'scripts/check-sp08-scoped-repository-boundary-with-r1-delegation.mjs',
  'scripts/check-r3-machine-simulation-boundary.test.mjs',
  'scripts/check-r3-machine-simulation-boundary.mjs',
  'scripts/check-r4-public-api-governance.test.mjs',
  'scripts/check-r4-public-api-governance.mjs',
  'scripts/check-r4-plugin-quality.test.mjs',
  'scripts/check-r4-plugin-quality.mjs',
]

function read(root, relativePath) {
  return fs.readFileSync(path.join(root, relativePath), 'utf8').replace(/\r\n?/g, '\n')
}

export function collectHostileBoundarySources(root = ROOT) {
  return {
    machineRoute: read(root, 'backend/src/routes/machine_api.rs'),
    machinePolicy: read(root, 'backend/src/services/machine_api.rs'),
    machineShared: read(root, 'backend/src/repositories/machine_authority.rs'),
    machineSqliteRepository: read(root, 'backend/src/repositories/machine_authority_sqlite.rs'),
    machinePostgresRepository: read(root, 'backend/src/repositories/machine_authority_postgres.rs'),
    machineTests: read(root, 'backend/src/services/machine_api/tests.rs'),
    machinePolicyTests: read(root, 'backend/src/services/machine_api_security_tests.rs'),
    tenantPreview: read(root, 'backend/src/routes/tenant_preview.rs'),
    tenantPreviewSqliteRepository: read(root, 'backend/src/repositories/tenant_preview.rs'),
    tenantPreviewPostgresRepository: read(root, 'backend/src/repositories/tenant_preview_postgres.rs'),
    damageModule: read(root, 'backend/official/device/src/damage.rs'),
    repairModule: read(root, 'backend/official/device/src/repair.rs'),
    refundModule: read(root, 'backend/official/finance/src/refund.rs'),
    overdueRoute: read(root, 'backend/src/routes/overdue.rs'),
    overdueModule: read(root, 'backend/system/admin/src/overdue.rs'),
    financeTaxRoute: read(root, 'backend/src/routes/finance_tax.rs'),
    inheritedBoundaryChecks: [...inheritedBoundaryChecks],
  }
}

function requireToken(errors, source, token, label) {
  if (!source.includes(token)) errors.push(`${label}: missing ${JSON.stringify(token)}`)
}

function forbidToken(errors, source, token, label) {
  if (source.includes(token)) errors.push(`${label}: forbidden ${JSON.stringify(token)}`)
}

function between(source, startToken, endToken) {
  const start = source.indexOf(startToken)
  const end = source.indexOf(endToken, start + startToken.length)
  if (start < 0 || end < 0) return ''
  return source.slice(start, end)
}

function requireCommandAccess(errors, source, command, access, label) {
  const pattern = new RegExp(
    `CommandMetadata::new\\(\\s*"${command}",\\s*(?:system_core::)?AccessRequirement::${access}`,
  )
  if (!pattern.test(source)) {
    errors.push(`${label}: ${command} must require ${access}`)
  }
}

export function validateHostileBoundarySources(snapshot) {
  const errors = []

  for (const token of [
    'fn require_machine_bearer_lane(',
    'headers.contains_key(header::COOKIE)',
    'headers.contains_key("x-talos-authority")',
    'headers.contains_key("x-talos-execution-mode")',
    'require_machine_bearer_lane(&headers)?;',
    '.get(header::AUTHORIZATION)',
    '.strip_prefix("Bearer ")',
    'execute_machine_with_repository(',
    'fn machine_lane_rejects_browser_and_caller_selected_authority_headers()',
  ]) {
    requireToken(errors, snapshot.machineRoute, token, 'machine/browser authority boundary')
  }
  for (const token of [
    'fn unauthorized_wrong_tenant_out_of_scope_and_version_fail_closed()',
    'fn machine_cannot_login_get_password_or_platform_membership_or_retarget()',
    'fn direct_sql_machine_owner_promotion_is_rejected_but_human_transfer_is_legal()',
  ]) {
    requireToken(errors, snapshot.machineTests, token, 'machine principal authority boundary')
  }

  for (const module of ['"two_fa"', '"consent"', '"deletion"', '"user_settings"']) {
    requireToken(
      errors,
      snapshot.machineShared,
      module,
      'machine principal interactive-control boundary',
    )
  }
  requireToken(
    errors,
    snapshot.machineShared,
    'pub(crate) fn machine_scope_allowed',
    'machine principal authority boundary',
  )

  const machineProvision = between(
    snapshot.machinePolicy,
    'pub(crate) fn provision_with_repository',
    '/// Called only by Registry',
  )
  requireToken(
    errors,
    machineProvision,
    'if !machine_scope_allowed(scope)',
    'machine principal provision boundary',
  )

  const sqliteMachineAuthorize = between(
    snapshot.machineSqliteRepository,
    'pub(crate) fn authorize(',
    'pub(crate) fn lifecycle(',
  )
  requireToken(
    errors,
    sqliteMachineAuthorize,
    '|| !machine_scope_allowed(scope)',
    'machine principal SQLite runtime boundary',
  )

  const postgresMachineAuthorize = between(
    snapshot.machinePostgresRepository,
    'pub(crate) fn authorize(',
    'pub(crate) fn lifecycle(',
  )
  requireToken(
    errors,
    postgresMachineAuthorize,
    '|| !machine_scope_allowed(&scope)',
    'machine principal PostgreSQL runtime boundary',
  )
  requireToken(
    errors,
    snapshot.machinePolicyTests,
    'fn machine_scopes_cannot_become_interactive_identity_or_compliance_authority()',
    'machine principal interactive-control runtime proof',
  )

  requireCommandAccess(errors, snapshot.damageModule, 'report', 'Authenticated', 'damage registry authority boundary')
  for (const command of ['assess', 'adjudicate']) {
    requireCommandAccess(errors, snapshot.damageModule, command, 'TenantAdmin', 'damage registry authority boundary')
  }
  for (const command of ['create_repair_order', 'start_repair', 'complete_repair', 'return_to_stock']) {
    requireCommandAccess(errors, snapshot.repairModule, command, 'TenantAdmin', 'repair registry authority boundary')
  }

  requireToken(errors, snapshot.repairModule, '"settlementAuthority": "r3_settlement"', 'repair settlement authority boundary')
  requireToken(
    errors,
    snapshot.repairModule,
    'fn return_to_stock_rolls_back_until_device_inventory_update_can_commit()',
    'repair atomic effect runtime proof',
  )
  const returnToStock = between(snapshot.repairModule, 'fn do_return_to_stock(', 'fn do_get(')
  if (!returnToStock) {
    errors.push('repair atomic effect boundary: do_return_to_stock is missing')
  } else {
    for (const token of [
      'conn.execute_batch("BEGIN IMMEDIATE")',
      "UPDATE devices SET rentalStatus = 'available' WHERE tenant_id = ?1 AND serialNo = ?2",
      "UPDATE repair_orders SET status = 'returned'",
      'execute_batch("COMMIT")',
      'conn.execute_batch("ROLLBACK")',
    ]) {
      requireToken(errors, returnToStock, token, 'repair atomic effect boundary')
    }
  }
  for (const token of [
    'execute("forfeit"',
    '"update_status"',
    'deposit_result.is_some()',
    'deposit_module.lock()',
    'device_module.lock()',
  ]) {
    forbidToken(errors, snapshot.repairModule, token, 'repair retired cross-module effect boundary')
  }

  requireCommandAccess(errors, snapshot.refundModule, 'request', 'TenantAdmin', 'refund registry authority boundary')
  requireCommandAccess(errors, snapshot.overdueModule, 'overdue_apply', 'Authenticated', 'overdue registry authority boundary')
  const overdueApplyRoute = between(snapshot.overdueRoute, 'async fn overdue_apply(', '// ── Waive')
  if (!overdueApplyRoute) {
    errors.push('overdue route authority boundary: overdue_apply handler is missing')
  } else {
    requireToken(errors, overdueApplyRoute, 'auth: AuthUser', 'overdue route authority boundary')
    requireToken(errors, overdueApplyRoute, 'make_ctx(&auth.0', 'overdue route authority boundary')
    forbidToken(errors, overdueApplyRoute, 'AdminUser', 'overdue route authority boundary')
  }

  for (const token of ['tenant_id:', '"tenantId"']) {
    forbidToken(errors, snapshot.financeTaxRoute, token, 'finance trusted tenant boundary')
  }

  for (const token of [
    'PlatformUser(admin): PlatformUser',
    'session.resolve',
    'require_resolved_workspace_capability',
    'require_workspace_mode_capability',
    'TenantScope::tenant(tenant_id.clone())',
    'ExecutionMode::Simulation(simulation_id)',
    "Namespace::Simulation(simulation_id.clone())",
  ]) {
    requireToken(errors, snapshot.tenantPreview, token, 'platform/workspace execution boundary')
  }
  forbidToken(
    errors,
    snapshot.tenantPreview,
    'simulation_sessions',
    'platform/workspace route must not own simulation-session SQL',
  )
  requireToken(
    errors,
    snapshot.tenantPreviewSqliteRepository,
    'WHERE id=?1 AND actor_id=?2 AND target_tenant_id=?3',
    'platform/workspace SQLite simulation ownership boundary',
  )
  requireToken(
    errors,
    snapshot.tenantPreviewSqliteRepository,
    "AND status='active'",
    'platform/workspace SQLite simulation status boundary',
  )
  const pgSimulationOwnership = between(
    snapshot.tenantPreviewPostgresRepository,
    '"SELECT 1 FROM simulation_sessions',
    '.bind(simulation_id)',
  )
  if (!pgSimulationOwnership) {
    errors.push('platform/workspace PostgreSQL simulation ownership query is missing')
  } else {
    requireToken(
      errors,
      pgSimulationOwnership,
      'WHERE id=$1 AND actor_id=$2 AND target_tenant_id=$3',
      'platform/workspace PostgreSQL simulation ownership boundary',
    )
    requireToken(
      errors,
      pgSimulationOwnership,
      "AND status='active'",
      'platform/workspace PostgreSQL simulation status boundary',
    )
    requireToken(
      errors,
      pgSimulationOwnership,
      'FOR SHARE',
      'platform/workspace PostgreSQL simulation lock boundary',
    )
  }

  const preview = between(snapshot.tenantPreview, '"preview" => (', '"diagnostics" =>')
  if (!preview) {
    errors.push('platform/workspace execution boundary: preview execution branch is missing')
  } else {
    requireToken(errors, preview, 'ExecutionMode::ReadOnlyPreview(', 'platform/workspace preview boundary')
    forbidToken(errors, preview, 'ExecutionMode::Normal', 'platform/workspace preview boundary')
  }

  const diagnostics = between(snapshot.tenantPreview, '"diagnostics" => (', '"simulation" =>')
  if (!diagnostics) {
    errors.push('platform/workspace execution boundary: diagnostics execution branch is missing')
  } else {
    requireToken(errors, diagnostics, 'ExecutionMode::ReadOnlyPreview(', 'platform/workspace diagnostics boundary')
    forbidToken(errors, diagnostics, 'ExecutionMode::Normal', 'platform/workspace diagnostics boundary')
  }

  const previewExecute = between(
    snapshot.tenantPreview,
    'fn preview_execute(',
    'fn preview_platform_ctx(',
  )
  if (!previewExecute) {
    errors.push('platform/workspace execution boundary: preview_execute handler is missing')
  } else {
    const resolveIndex = previewExecute.indexOf('"session.resolve"')
    const dataContextIndex = previewExecute.indexOf('let ctx = workspace_data_ctx(')
    if (resolveIndex < 0 || dataContextIndex < 0 || resolveIndex >= dataContextIndex) {
      errors.push('platform/workspace execution boundary: server-side session.resolve must precede tenant data-context construction')
    }
  }

  for (const required of [
    'scripts/check-sp03c-trusted-tenant-resolution-boundary.mjs',
    'scripts/check-sp08-scoped-repository-boundary-with-r1-delegation.mjs',
    'scripts/check-r3-machine-simulation-boundary.mjs',
    'scripts/check-r4-public-api-governance.mjs',
    'scripts/check-r4-plugin-quality.mjs',
  ]) {
    if (!snapshot.inheritedBoundaryChecks.includes(required)) {
      errors.push(`hostile boundary aggregate: missing inherited authority check ${required}`)
    }
  }

  return errors
}

export function runInheritedBoundaryChecks(root = ROOT) {
  const failures = []
  for (const relativePath of inheritedBoundaryChecks) {
    const result = spawnSync(process.execPath, [path.join(root, relativePath)], {
      cwd: root,
      encoding: 'utf8',
      stdio: 'pipe',
    })
    if (result.status !== 0) {
      failures.push(`${relativePath}: failed\n${result.stdout ?? ''}${result.stderr ?? ''}`.trim())
    }
  }
  return failures
}

export function run(root = ROOT) {
  const failures = [
    ...validateHostileBoundarySources(collectHostileBoundarySources(root)),
    ...runInheritedBoundaryChecks(root),
  ]
  if (failures.length > 0) {
    console.error('R4-P7 hostile cross-authority boundaries FAILED')
    for (const failure of failures) console.error(`- ${failure}`)
    return false
  }
  console.log('R4-P7 hostile cross-authority boundaries PASS')
  return true
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!run()) process.exitCode = 1
}
