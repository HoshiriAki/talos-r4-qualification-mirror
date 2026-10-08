#!/usr/bin/env node

import assert from 'node:assert/strict'

import {
  collectHostileBoundarySources,
  validateHostileBoundarySources,
} from './check-r4-hostile-security-boundaries.mjs'

const baseline = collectHostileBoundarySources()
assert.deepEqual(validateHostileBoundarySources(baseline), [])

const cases = [
  {
    name: 'browser cookie must not authorize machine execution',
    mutate: (value) => ({ ...value, machineRoute: value.machineRoute.replace('headers.contains_key(header::COOKIE)', 'false') }),
  },
  {
    name: 'machine execution must remain bearer authenticated',
    mutate: (value) => ({ ...value, machineRoute: value.machineRoute.replace('.get(header::AUTHORIZATION)', '.get(header::ACCEPT)') }),
  },
  {
    name: 'machine execute must call the browser-authority rejection guard',
    mutate: (value) => ({ ...value, machineRoute: value.machineRoute.replace('require_machine_bearer_lane(&headers)?;', '// machine bearer guard removed') }),
  },
  {
    name: 'machine browser-lane negative runtime proof is mandatory',
    mutate: (value) => ({ ...value, machineRoute: value.machineRoute.replace('fn machine_lane_rejects_browser_and_caller_selected_authority_headers()', 'fn removed_machine_lane_negative_test()') }),
  },
  {
    name: 'machine cannot gain browser or platform authority',
    mutate: (value) => ({ ...value, machineTests: value.machineTests.replace('fn machine_cannot_login_get_password_or_platform_membership_or_retarget()', 'fn removed_machine_authority_test()') }),
  },
  {
    name: 'machine provisioning cannot grant interactive identity controls',
    mutate: (value) => ({
      ...value,
      machinePolicy: value.machinePolicy.replace(
        'if !machine_scope_allowed(scope)',
        'if false',
      ),
    }),
  },
  {
    name: 'SQLite machine runtime must recheck legacy persisted interactive scopes',
    mutate: (value) => ({
      ...value,
      machineSqliteRepository: value.machineSqliteRepository.replace(
        '|| !machine_scope_allowed(scope)',
        '|| false',
      ),
    }),
  },
  {
    name: 'PostgreSQL machine runtime must recheck legacy persisted interactive scopes',
    mutate: (value) => ({
      ...value,
      machinePostgresRepository: value.machinePostgresRepository.replace(
        '|| !machine_scope_allowed(&scope)',
        '|| false',
      ),
    }),
  },
  {
    name: 'machine interactive module denylist cannot lose deletion governance',
    mutate: (value) => ({
      ...value,
      machineShared: value.machineShared.replace(
        '"deletion" |',
        '"deletion_removed" |',
      ),
    }),
  },
  {
    name: 'machine interactive-control runtime proof is mandatory',
    mutate: (value) => ({ ...value, machinePolicyTests: value.machinePolicyTests.replace('fn machine_scopes_cannot_become_interactive_identity_or_compliance_authority()', 'fn removed_machine_interactive_control_test()') }),
  },
  {
    name: 'damage assessment cannot fall back to ordinary authenticated authority',
    mutate: (value) => ({ ...value, damageModule: value.damageModule.replace('"assess",\n                AccessRequirement::TenantAdmin', '"assess",\n                AccessRequirement::Authenticated') }),
  },
  {
    name: 'damage report must remain available to authenticated frontline workflow',
    mutate: (value) => ({ ...value, damageModule: value.damageModule.replace('"report",\n                AccessRequirement::Authenticated', '"report",\n                AccessRequirement::TenantAdmin') }),
  },
  {
    name: 'repair return-to-stock cannot fall back to ordinary authenticated authority',
    mutate: (value) => ({ ...value, repairModule: value.repairModule.replace('"return_to_stock",\n                AccessRequirement::TenantAdmin', '"return_to_stock",\n                AccessRequirement::Authenticated') }),
  },
  {
    name: 'legacy repair cannot reclaim deposit settlement authority',
    mutate: (value) => ({ ...value, repairModule: value.repairModule.replace('"settlementAuthority": "r3_settlement"', 'execute("forfeit"') }),
  },
  {
    name: 'legacy repair cannot reintroduce a module-level deposit side effect',
    mutate: (value) => ({ ...value, repairModule: value.repairModule.replace('let now = shanghai_now_iso();', 'let now = shanghai_now_iso();\n        let _ = self.deposit_module.lock();') }),
  },
  {
    name: 'repair return-to-stock must own its immediate transaction',
    mutate: (value) => {
      const start = value.repairModule.indexOf('fn do_return_to_stock(')
      const end = value.repairModule.indexOf('fn do_get(', start)
      const block = value.repairModule.slice(start, end).replace('conn.execute_batch("BEGIN IMMEDIATE")', 'conn.execute_batch("BEGIN DEFERRED")')
      return { ...value, repairModule: value.repairModule.slice(0, start) + block + value.repairModule.slice(end) }
    },
  },
  {
    name: 'repair return-to-stock must keep the device update in its atomic local transaction',
    mutate: (value) => ({ ...value, repairModule: value.repairModule.replace("UPDATE devices SET rentalStatus = 'available' WHERE tenant_id = ?1 AND serialNo = ?2", "UPDATE devices SET rentalStatus = 'repairing' WHERE tenant_id = ?1 AND serialNo = ?2") }),
  },
  {
    name: 'repair return-to-stock rollback runtime proof is mandatory',
    mutate: (value) => ({ ...value, repairModule: value.repairModule.replace('fn return_to_stock_rolls_back_until_device_inventory_update_can_commit()', 'fn removed_return_to_stock_atomicity_test()') }),
  },
  {
    name: 'refund request cannot fall back to ordinary authenticated authority',
    mutate: (value) => ({ ...value, refundModule: value.refundModule.replace('"request",\n                AccessRequirement::TenantAdmin', '"request",\n                AccessRequirement::Authenticated') }),
  },
  {
    name: 'overdue apply route must not drift back to admin-only',
    mutate: (value) => ({ ...value, overdueRoute: value.overdueRoute.replace('async fn overdue_apply(\n    State(state): State<Arc<AppState>>,\n    auth: AuthUser,', 'async fn overdue_apply(\n    State(state): State<Arc<AppState>>,\n    auth: AdminUser,') }),
  },
  {
    name: 'overdue apply canonical module access must stay authenticated',
    mutate: (value) => ({ ...value, overdueModule: value.overdueModule.replace('"overdue_apply",\n                system_core::AccessRequirement::Authenticated', '"overdue_apply",\n                system_core::AccessRequirement::TenantAdmin') }),
  },
  {
    name: 'finance routes cannot reintroduce caller-selected tenant payload authority',
    mutate: (value) => ({ ...value, financeTaxRoute: value.financeTaxRoute.replace('struct InvoiceIssueBody {', 'struct InvoiceIssueBody {\n    tenant_id: Option<String>,') }),
  },
  {
    name: 'diagnostics must remain read-only execution mode',
    mutate: (value) => {
      const start = value.tenantPreview.indexOf('"diagnostics" => (')
      const end = value.tenantPreview.indexOf('"simulation" =>', start)
      const before = value.tenantPreview.slice(0, start)
      const block = value.tenantPreview.slice(start, end).replace(/ExecutionMode::ReadOnlyPreview\([\s\S]*?\),\n\s*\),/, 'ExecutionMode::Normal,\n        ),')
      return { ...value, tenantPreview: before + block + value.tenantPreview.slice(end) }
    },
  },
  {
    name: 'preview must remain read-only execution mode',
    mutate: (value) => {
      const startToken = '"preview" => ('
      const endToken = '"diagnostics" =>'
      const start = value.tenantPreview.indexOf(startToken)
      const end = value.tenantPreview.indexOf(endToken, start + startToken.length)
      assert.ok(start >= 0 && end > start, 'preview execution branch must exist in fixture')
      const block = value.tenantPreview.slice(start, end)
      const mutatedBlock = block.replace(
        'ExecutionMode::ReadOnlyPreview(',
        'ExecutionMode::Normal /* removed preview mode */(',
      )
      assert.notEqual(mutatedBlock, block, 'preview read-only mutation target must exist')
      return {
        ...value,
        tenantPreview: value.tenantPreview.slice(0, start) + mutatedBlock + value.tenantPreview.slice(end),
      }
    },
  },
  {
    name: 'simulation must remain isolated namespace',
    mutate: (value) => ({ ...value, tenantPreview: value.tenantPreview.replace('Namespace::Simulation(simulation_id.clone())', 'Namespace::Production') }),
  },
  {
    name: 'tenant preview route cannot reclaim simulation-session SQL',
    mutate: (value) => ({
      ...value,
      tenantPreview: value.tenantPreview + '\n// SELECT 1 FROM simulation_sessions\n',
    }),
  },
  {
    name: 'SQLite preview repository must bind simulation ownership to actor and tenant',
    mutate: (value) => ({
      ...value,
      tenantPreviewSqliteRepository: value.tenantPreviewSqliteRepository.replace(
        'WHERE id=?1 AND actor_id=?2 AND target_tenant_id=?3',
        'WHERE id=?1',
      ),
    }),
  },
  {
    name: 'PostgreSQL preview repository must bind simulation ownership to actor and tenant',
    mutate: (value) => ({
      ...value,
      tenantPreviewPostgresRepository: value.tenantPreviewPostgresRepository.replace(
        'WHERE id=$1 AND actor_id=$2 AND target_tenant_id=$3',
        'WHERE id=$1',
      ),
    }),
  },
  {
    name: 'PostgreSQL preview repository must keep the simulation ownership lock',
    mutate: (value) => {
      const start = value.tenantPreviewPostgresRepository.indexOf('"SELECT 1 FROM simulation_sessions')
      const end = value.tenantPreviewPostgresRepository.indexOf('.bind(simulation_id)', start)
      assert.ok(start >= 0 && end > start, 'PostgreSQL simulation ownership query must exist in fixture')
      const block = value.tenantPreviewPostgresRepository.slice(start, end)
      const mutatedBlock = block.replace('FOR SHARE', '/* lock removed */')
      assert.notEqual(mutatedBlock, block, 'simulation ownership lock mutation target must exist')
      return {
        ...value,
        tenantPreviewPostgresRepository:
          value.tenantPreviewPostgresRepository.slice(0, start) +
          mutatedBlock +
          value.tenantPreviewPostgresRepository.slice(end),
      }
    },
  },
  {
    name: 'resolved workspace must precede tenant context construction',
    mutate: (value) => {
      const startToken = 'fn preview_execute('
      const endToken = 'fn preview_platform_ctx('
      const start = value.tenantPreview.indexOf(startToken)
      const end = value.tenantPreview.indexOf(endToken, start + startToken.length)
      assert.ok(start >= 0 && end > start, 'preview_execute function must exist in fixture')
      const block = value.tenantPreview.slice(start, end)
      const mutatedBlock = block.replace(
        '    let resolved = state\n',
        '    let ctx = workspace_data_ctx( /* synthetic early context */\n    let resolved = state\n',
      )
      assert.notEqual(mutatedBlock, block, 'preview resolve-order mutation target must exist')
      return {
        ...value,
        tenantPreview: value.tenantPreview.slice(0, start) + mutatedBlock + value.tenantPreview.slice(end),
      }
    },
  },
  {
    name: 'tenant resolution inherited gate is mandatory',
    mutate: (value) => ({ ...value, inheritedBoundaryChecks: value.inheritedBoundaryChecks.filter((item) => item !== 'scripts/check-sp03c-trusted-tenant-resolution-boundary.mjs') }),
  },
  {
    name: 'scoped repository inherited gate is mandatory',
    mutate: (value) => ({ ...value, inheritedBoundaryChecks: value.inheritedBoundaryChecks.filter((item) => item !== 'scripts/check-sp08-scoped-repository-boundary-with-r1-delegation.mjs') }),
  },
  {
    name: 'simulation inherited gate is mandatory',
    mutate: (value) => ({ ...value, inheritedBoundaryChecks: value.inheritedBoundaryChecks.filter((item) => item !== 'scripts/check-r3-machine-simulation-boundary.mjs') }),
  },
  {
    name: 'machine/browser P4 inherited gate is mandatory',
    mutate: (value) => ({ ...value, inheritedBoundaryChecks: value.inheritedBoundaryChecks.filter((item) => item !== 'scripts/check-r4-public-api-governance.mjs') }),
  },
  {
    name: 'plugin raw-authority inherited gate is mandatory',
    mutate: (value) => ({ ...value, inheritedBoundaryChecks: value.inheritedBoundaryChecks.filter((item) => item !== 'scripts/check-r4-plugin-quality.mjs') }),
  },
]

for (const testCase of cases) {
  const mutated = testCase.mutate({
    ...baseline,
    inheritedBoundaryChecks: [...baseline.inheritedBoundaryChecks],
  })
  const errors = validateHostileBoundarySources(mutated)
  assert.ok(errors.length > 0, `${testCase.name}: expected hostile-boundary failure`)
}

console.log(`R4-P7 hostile boundary fixtures passed: ${cases.length} mutations`)
