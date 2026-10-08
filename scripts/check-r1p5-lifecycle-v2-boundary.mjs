#!/usr/bin/env node

import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'

const root = path.resolve(process.env.TALOS_R1P5_ROOT || process.cwd())
const failures = []

function read(rel) {
  const file = path.join(root, rel)
  if (!fs.existsSync(file)) {
    failures.push(`missing ${rel}`)
    return ''
  }
  return fs.readFileSync(file, 'utf8').replaceAll('\r\n', '\n')
}
function requireTokens(rel, tokens) {
  const text = read(rel)
  for (const token of tokens) if (!text.includes(token)) failures.push(`${rel}: missing ${JSON.stringify(token)}`)
  return text
}
function forbidTokens(rel, tokens) {
  const text = read(rel)
  for (const token of tokens) if (text.includes(token)) failures.push(`${rel}: forbidden ${JSON.stringify(token)}`)
  return text
}

requireTokens('backend/src/db/migrations/056_order_lifecycle_v2.sql', [
  'CREATE TABLE IF NOT EXISTS order_lifecycle', 'commercial_status', 'contract_status',
  'financial_status', 'fulfilment_status', 'risk_status', 'version INTEGER NOT NULL DEFAULT 1',
  'CREATE TABLE IF NOT EXISTS order_lifecycle_history',
  'CREATE TABLE IF NOT EXISTS lifecycle_migration_exceptions',
  'legacy single-axis order status cannot reconstruct',
  'CREATE TRIGGER IF NOT EXISTS trg_orders_create_lifecycle_v2',
])
requireTokens('backend/src/db/migrations/postgres/056_order_lifecycle_v2.sql', [
  'CREATE TABLE IF NOT EXISTS order_lifecycle', 'order_lifecycle_history',
  'lifecycle_migration_exceptions', 'ensure_order_lifecycle_v2',
])
requireTokens('backend/src/db/migrations.rs', [
  '056_order_lifecycle_v2', 'migrations/056_order_lifecycle_v2.sql', 'Some("066_r3_machine_api")',
])
requireTokens('backend/src/db/migrations_pg.rs', [
  '056_order_lifecycle_v2', 'migrations/postgres/056_order_lifecycle_v2.sql', 'ids.len(), 63',
])

const repository = requireTokens('backend/src/repositories/lifecycle.rs', [
  'pub struct ScopedOrderLifecycleRepository', 'write_immediate', 'expected_version',
  'optimistic lifecycle version conflict', 'order_lifecycle_history', 'can_mark_ready_to_ship',
  'confirmed_reservation_exists', 'allocations_complete', 'blocking_risk',
])
if (repository.includes('extra_checks')) failures.push('lifecycle repository must not use caller-supplied bool guards')
requireTokens('backend/src/repositories/contracts/provider.rs', [
  'pub fn lifecycles(&self)', 'ScopedOrderLifecycleRepository::new(self)',
])
requireTokens('backend/src/application/order_lifecycle_v2.rs', [
  'const MODULE_NAME: &str = "order_lifecycle_v2"', '"mark_ready_to_ship"', '"close_order"',
  'expected_version',
])
forbidTokens('backend/src/application/order_lifecycle_v2.rs', ['write_command("transition")', 'write_command("update_status")'])

const compatibility = requireTokens('backend/src/application/order_lifecycle_compatibility.rs', [
  'pub struct OrderLifecycleCompatibilityModule', 'if command == "transition"',
  'self.lifecycle.execute', 'mark_awaiting_payment', 'mark_return_pending', 'expected_version',
])
if (compatibility.includes('guard_transition')) {
  failures.push('legacy transition compatibility must delegate to Lifecycle V2 named actions, not the old graph')
}

const query = requireTokens('backend/src/application/order_query_v2.rs', [
  'lifecycles().operational_view', 'LifecycleAllowedAction',
  'commercial_status', 'contract_status', 'financial_status', 'fulfilment_status', 'risk_status',
  'version: operational.lifecycle.version', 'allowed_actions: operational.allowed_actions',
])
if (query.includes('state_machine::allowed_next')) {
  failures.push('Order Query V2 must not derive actions from the retired single-axis transition graph')
}

requireTokens('backend/src/routes/order_lifecycle_v2.rs', [
  '/api/v2/orders/{id}/lifecycle', '/api/v2/orders/{id}/lifecycle/actions/{action}',
  'TrustedTenantUser', 'order_lifecycle_v2',
])
requireTokens('backend/src/routes/orders.rs', ['/users/transition', '.execute("order", "transition"'])
requireTokens('backend/src/registry/factory.rs', [
  'OrderLifecycleCompatibilityModule', '(OrderLifecycleCompatibilityModule, Order, "order")',
  'OrderLifecycleCompatibilityModule::new(', 'OrderLifecycleV2Module', '"order_lifecycle_v2"',
])
requireTokens('backend/official/order/src/state_machine.rs', ['BIZ_LEGACY_ORDER_TRANSITION_RETIRED'])

requireTokens('frontend/src/api/orders.ts', [
  'commercialStatus', 'contractStatus', 'financialStatus', 'fulfilmentStatus', 'riskStatus',
  'expectedVersion', 'applyLifecycleAction', '/lifecycle/actions/',
])
forbidTokens('frontend/src/api/orders.ts', ['STATUS_TRANSITIONS'])
requireTokens('frontend/src/components/order/OrderActions.vue', ['action.action', 'action.expectedVersion'])
requireTokens('backend/src/application/mod.rs', ['mod lifecycle_tests;', 'OrderLifecycleV2Module', 'OrderLifecycleCompatibilityModule'])
requireTokens('backend/src/application/lifecycle_tests.rs', [
  'optimistic_version_conflict_is_fail_closed',
  'ready_to_ship_named_guard_uses_payment_reservation_allocation_and_risk',
  'preview_reads_lifecycle_but_cannot_write_and_simulation_fails_closed',
])
requireTokens('backend/src/registry/descriptors.rs', ['OrderLifecycleV2', '"order_lifecycle_v2"'])
requireTokens('scripts/check-sp08-scoped-repository-boundary-with-r1-delegation.mjs', [
  'check-r1p5-lifecycle-v2-boundary.mjs', 'backend/src/repositories/lifecycle.rs', 'TALOS-OPS-031',
])
requireTokens('package.json', ['quality:talos-ops:lifecycle-v2:test', 'quality:talos-ops:lifecycle-v2'])

if (failures.length) {
  process.stderr.write('TALOS-OPS-031 R1-P5 lifecycle boundary check failed:\n')
  for (const failure of failures) process.stderr.write(`- ${failure}\n`)
  process.exit(1)
}
process.stdout.write('TALOS-OPS-031 R1-P5 lifecycle boundary passed.\n')
