#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(SCRIPT_DIR, '..')

const PATHS = {
  route: 'backend/src/routes/pricing.rs',
  authority: 'backend/src/application/warehouse_authority.rs',
  sqlite: 'backend/src/repositories/warehouse.rs',
  postgres: 'backend/src/repositories/warehouse_postgres.rs',
  dispatch: 'backend/src/repositories/warehouse_dispatch.rs',
  pgTest: 'backend/src/repositories/postgres/warehouse_qualification_tests.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

export function collectWarehouseRoutingSnapshot() {
  return Object.fromEntries(Object.entries(PATHS).map(([key, value]) => [key, read(value)]))
}

export function validateWarehouseRoutingSnapshot(snapshot) {
  const errors = []

  for (const forbidden of [
    'resolve_warehouse_route(&state.pool',
    'warehouse_routing_service::resolve_warehouse_route',
    'state.pool',
  ]) {
    if (snapshot.route.includes(forbidden)) {
      errors.push('pricing warehouse-route retains SQLite authority: ' + forbidden)
    }
  }

  for (const token of [
    '.application_services()',
    '.warehouse_authority()',
    '.resolve_route(&ctx, province)',
  ]) {
    if (!snapshot.route.includes(token)) {
      errors.push('pricing warehouse-route application cutover missing: ' + token)
    }
  }

  for (const token of [
    'pub fn resolve_route(',
    '.routing_rules_for_province(province)',
    'fn select_route(',
    'warehouse_type == "owned"',
  ]) {
    if (!snapshot.authority.includes(token)) {
      errors.push('warehouse routing application authority missing: ' + token)
    }
  }

  for (const token of [
    'w.tenant_id=?1 AND r.province=?2 AND w.enabled=1',
    'ORDER BY r.shippingDays ASC,r.returnDays ASC,r.warehouseId ASC',
  ]) {
    if (!snapshot.sqlite.includes(token)) {
      errors.push('SQLite warehouse routing scope missing: ' + token)
    }
  }

  for (const token of [
    'w.tenant_id=$1 AND r.province=$2 AND w.enabled=true',
    'ORDER BY r.shippingdays ASC,r.returndays ASC,r.warehouseid ASC',
  ]) {
    if (!snapshot.postgres.includes(token)) {
      errors.push('PostgreSQL warehouse routing scope missing: ' + token)
    }
  }

  if (!snapshot.dispatch.includes('pub fn routing_rules_for_province(')) {
    errors.push('scoped warehouse repository does not dispatch routing rules')
  }

  for (const token of [
    'routing_rules_for_province("Shanghai")?',
    'assert_eq!(routing_rules[0].warehouse_id, "warehouse-a")',
    "assert!(\n            scoped_b\n                .warehouses()\n                .routing_rules_for_province(\"Shanghai\")?\n                .is_empty()\n        );",
  ]) {
    if (!snapshot.pgTest.includes(token)) {
      errors.push('PG18 warehouse routing evidence missing: ' + token)
    }
  }

  for (const token of [
    'node scripts/check-r4-p8-warehouse-routing-cutover.test.mjs',
    'node scripts/check-r4-p8-warehouse-routing-cutover.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head P8-V qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateWarehouseRoutingSnapshot(collectWarehouseRoutingSnapshot())
  if (errors.length > 0) {
    console.error('R4-P8 warehouse routing cutover gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 warehouse routing cutover gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
