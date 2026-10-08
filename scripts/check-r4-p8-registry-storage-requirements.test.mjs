#!/usr/bin/env node

import assert from 'node:assert/strict'
import {
  collectRegistryStorageRequirementSnapshot,
  validateRegistryStorageRequirementSnapshot,
} from './check-r4-p8-registry-storage-requirements.mjs'

function replaceRequired(source, needle, replacement, name) {
  assert.ok(source.includes(needle), name + ': mutation anchor missing: ' + JSON.stringify(needle))
  const mutated = source.replace(needle, replacement)
  assert.notEqual(mutated, source, name + ': mutation did not change source')
  return mutated
}

function expectFailure(name, mutate, needle) {
  const snapshot = structuredClone(collectRegistryStorageRequirementSnapshot())
  mutate(snapshot)
  const errors = validateRegistryStorageRequirementSnapshot(snapshot)
  assert.ok(errors.length > 0, name + ': mutation unexpectedly passed')
  assert.ok(
    errors.some(error => error.includes(needle)),
    name + ': expected ' + JSON.stringify(needle) + ', got ' + JSON.stringify(errors),
  )
}

const baseline = validateRegistryStorageRequirementSnapshot(
  collectRegistryStorageRequirementSnapshot(),
)
assert.deepEqual(baseline, [], 'baseline must pass: ' + baseline.join('; '))

expectFailure(
  'integration descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      '        NONE,\n        Integration',
      '        SQLITE,\n        Integration',
      'integration descriptor restores SQLite storage',
    )
  },
  'integration',
)

expectFailure(
  'pricing dependency list restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'const PRICING: &[ModuleRequirement] = &[\n    ModuleRequirement::Module("logistics"),\n    ModuleRequirement::Module("warehouse_routing"),\n];',
      'const PRICING: &[ModuleRequirement] = &[\n    ModuleRequirement::SqlitePool,\n    ModuleRequirement::Module("logistics"),\n    ModuleRequirement::Module("warehouse_routing"),\n];',
      'pricing dependency list restores SQLite storage',
    )
  },
  'constPRICING',
)

expectFailure(
  'pricing runtime restores direct SQLite construction',
  snapshot => {
    snapshot.factory += '\n// with_pool!(FeaturePricing)\n'
  },
  'FeaturePricing',
)

expectFailure(
  'quote descriptor restores direct SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'const QUOTE: &[ModuleRequirement] = &[ModuleRequirement::Module("pricing")];',
      'const QUOTE: &[ModuleRequirement] = &[ModuleRequirement::SqlitePool, ModuleRequirement::Module("pricing")];',
      'quote descriptor restores direct SQLite storage',
    )
  },
  'constQUOTE',
)

expectFailure(
  'integration loses PostgreSQL composition proof',
  snapshot => {
    snapshot.main = replaceRequired(
      snapshot.main,
      'IntegrationModule::new_with_postgres_persistence(',
      'IntegrationModule::new(',
      'integration loses PostgreSQL composition proof',
    )
  },
  'PostgreSQL composition proof',
)

expectFailure(
  'quote runtime restores direct SQLite construction',
  snapshot => {
    snapshot.factory += '\n// with_pool!(FeatureQuote)\n'
  },
  'quote descriptor truth restored direct SQLite runtime construction',
)

expectFailure(
  'audit descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("audit" => "feature-audit", Core, ModuleActivation::Always, NONE, Audit)',
      'descriptor!("audit" => "feature-audit", Core, ModuleActivation::Always, SQLITE, Audit)',
      'audit descriptor restores SQLite storage',
    )
  },
  'audit',
)

for (const [name, before, after] of [
  [
    'report',
    'descriptor!("report", Business, ModuleActivation::Always, NONE, Report)',
    'descriptor!("report", Business, ModuleActivation::Always, SQLITE, Report)',
  ],
  [
    'staff',
    'descriptor!("staff", Business, ModuleActivation::Always, NONE, Staff)',
    'descriptor!("staff", Business, ModuleActivation::Always, SQLITE, Staff)',
  ],
  [
    'two_fa',
    'descriptor!("two_fa", Business, ModuleActivation::Always, NONE, TwoFa)',
    'descriptor!("two_fa", Business, ModuleActivation::Always, SQLITE, TwoFa)',
  ],
  [
    'consent',
    'descriptor!("consent", Business, ModuleActivation::Always, NONE, Consent)',
    'descriptor!("consent", Business, ModuleActivation::Always, SQLITE, Consent)',
  ],
  [
    'deletion',
    '        NONE,\n        Deletion',
    '        SQLITE,\n        Deletion',
  ],
]) {
  expectFailure(
    name + ' descriptor restores SQLite storage',
    snapshot => {
      snapshot.descriptors = replaceRequired(
        snapshot.descriptors,
        before,
        after,
        name + ' descriptor restores SQLite storage',
      )
    },
    name,
  )
}

expectFailure(
  'device descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("device" => "feature-device", Business, ModuleActivation::Always, NONE, Device)',
      'descriptor!("device" => "feature-device", Business, ModuleActivation::Always, SQLITE, Device)',
      'device descriptor restores SQLite storage',
    )
  },
  'device',
)

expectFailure(
  'model descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("model", Business, ModuleActivation::Always, NONE, Model)',
      'descriptor!("model", Business, ModuleActivation::Always, SQLITE, Model)',
      'model descriptor restores SQLite storage',
    )
  },
  'model',
)

expectFailure(
  'procurement descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!(\n        "procurement",\n        Business,\n        ModuleActivation::Always,\n        NONE,\n        Procurement\n    )',
      'descriptor!(\n        "procurement",\n        Business,\n        ModuleActivation::Always,\n        SQLITE,\n        Procurement\n    )',
      'procurement descriptor restores SQLite storage',
    )
  },
  'procurement',
)

expectFailure(
  'damage descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("damage", Business, ModuleActivation::Always, NONE, Damage)',
      'descriptor!("damage", Business, ModuleActivation::Always, SQLITE, Damage)',
      'damage descriptor restores SQLite storage',
    )
  },
  'damage',
)

expectFailure(
  'depreciation descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!(\n        "depreciation",\n        Business,\n        ModuleActivation::Always,\n        NONE,\n        Depreciation\n    )',
      'descriptor!(\n        "depreciation",\n        Business,\n        ModuleActivation::Always,\n        SQLITE,\n        Depreciation\n    )',
      'depreciation descriptor restores SQLite storage',
    )
  },
  'depreciation',
)

expectFailure(
  'deposit descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("deposit", Business, ModuleActivation::Always, NONE, Deposit)',
      'descriptor!("deposit", Business, ModuleActivation::Always, SQLITE, Deposit)',
      'deposit descriptor restores SQLite storage',
    )
  },
  'deposit',
)

expectFailure(
  'refund descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("refund", Business, ModuleActivation::Always, NONE, Refund)',
      'descriptor!("refund", Business, ModuleActivation::Always, SQLITE, Refund)',
      'refund descriptor restores SQLite storage',
    )
  },
  'refund',
)

expectFailure(
  'repair descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("repair", Business, ModuleActivation::Always, NONE, Repair)',
      'descriptor!("repair", Business, ModuleActivation::Always, SQLITE, Repair)',
      'repair descriptor restores SQLite storage',
    )
  },
  'repair',
)

expectFailure(
  'invoice descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("invoice", Business, ModuleActivation::Always, NONE, Invoice)',
      'descriptor!("invoice", Business, ModuleActivation::Always, SQLITE, Invoice)',
      'invoice descriptor restores SQLite storage',
    )
  },
  'invoice',
)

expectFailure(
  'settlement descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!(\n        "settlement",\n        Business,\n        ModuleActivation::Always,\n        NONE,\n        Settlement\n    )',
      'descriptor!(\n        "settlement",\n        Business,\n        ModuleActivation::Always,\n        SQLITE,\n        Settlement\n    )',
      'settlement descriptor restores SQLite storage',
    )
  },
  'settlement',
)

expectFailure(
  'tax descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("tax", Business, ModuleActivation::Always, NONE, Tax)',
      'descriptor!("tax", Business, ModuleActivation::Always, SQLITE, Tax)',
      'tax descriptor restores SQLite storage',
    )
  },
  'tax',
)

expectFailure(
  'credit descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("credit", Business, ModuleActivation::Always, NONE, Credit)',
      'descriptor!("credit", Business, ModuleActivation::Always, SQLITE, Credit)',
      'credit descriptor restores SQLite storage',
    )
  },
  'credit',
)

expectFailure(
  'overdue descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("overdue", Business, ModuleActivation::Always, NONE, Overdue)',
      'descriptor!("overdue", Business, ModuleActivation::Always, SQLITE, Overdue)',
      'overdue descriptor restores SQLite storage',
    )
  },
  'overdue',
)

expectFailure(
  'optical SOP descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!(\n        "optical_sop",\n        Business,\n        ModuleActivation::Always,\n        NONE,\n        OpticalSop\n    )',
      'descriptor!(\n        "optical_sop",\n        Business,\n        ModuleActivation::Always,\n        SQLITE,\n        OpticalSop\n    )',
      'optical SOP descriptor restores SQLite storage',
    )
  },
  'optical_sop',
)

expectFailure(
  'booking descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("booking", Business, ModuleActivation::Always, NONE, Booking)',
      'descriptor!("booking", Business, ModuleActivation::Always, SQLITE, Booking)',
      'booking descriptor restores SQLite storage',
    )
  },
  'booking',
)

expectFailure(
  'order descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("order", Business, ModuleActivation::Always, NONE, Order)',
      'descriptor!("order", Business, ModuleActivation::Always, SQLITE, Order)',
      'order descriptor restores SQLite storage',
    )
  },
  'order',
)

expectFailure(
  'notify descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("notify", Business, ModuleActivation::Always, NONE, Notify)',
      'descriptor!("notify", Business, ModuleActivation::Always, SQLITE, Notify)',
      'notify descriptor restores SQLite storage',
    )
  },
  'notify',
)

expectFailure(
  'ROA descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("roa", Business, ModuleActivation::Always, NONE, Roa)',
      'descriptor!("roa", Business, ModuleActivation::Always, SQLITE, Roa)',
      'ROA descriptor restores SQLite storage',
    )
  },
  'roa',
)

expectFailure(
  'contract descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!(\n        "contract",\n        Business,\n        ModuleActivation::Always,\n        NONE,\n        Contract\n    )',
      'descriptor!(\n        "contract",\n        Business,\n        ModuleActivation::Always,\n        SQLITE,\n        Contract\n    )',
      'contract descriptor restores SQLite storage',
    )
  },
  'contract',
)

expectFailure(
  'barcode descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("barcode", Business, ModuleActivation::Always, NONE, Barcode)',
      'descriptor!("barcode", Business, ModuleActivation::Always, SQLITE, Barcode)',
      'barcode descriptor restores SQLite storage',
    )
  },
  'barcode',
)

expectFailure(
  'reservation descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!(\n        "reservation",\n        Business,\n        ModuleActivation::Always,\n        NONE,\n        Reservation\n    )',
      'descriptor!(\n        "reservation",\n        Business,\n        ModuleActivation::Always,\n        SQLITE,\n        Reservation\n    )',
      'reservation descriptor restores SQLite storage',
    )
  },
  'reservation',
)

expectFailure(
  'tenant governance descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("tenant_governance" => "feature-tenant-governance", Core, ModuleActivation::Always, NONE, TenantGovernance)',
      'descriptor!("tenant_governance" => "feature-tenant-governance", Core, ModuleActivation::Always, SQLITE, TenantGovernance)',
      'tenant governance descriptor restores SQLite storage',
    )
  },
  'tenant_governance',
)

expectFailure(
  'tenant preview descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("tenant_preview" => "feature-tenant-preview", Core, ModuleActivation::Always, NONE, TenantPreview)',
      'descriptor!("tenant_preview" => "feature-tenant-preview", Core, ModuleActivation::Always, SQLITE, TenantPreview)',
      'tenant preview descriptor restores SQLite storage',
    )
  },
  'tenant_preview',
)

expectFailure(
  'tenant preview runtime restores direct SQLite construction',
  snapshot => {
    snapshot.factory += '\n// FeatureTenantPreview::with_pool(self.pool.clone())\n'
  },
  'FeatureTenantPreview::with_pool',
)

expectFailure(
  'tenant simulation descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!("tenant_simulation" => "feature-tenant-simulation", Core, ModuleActivation::Always, NONE, TenantSimulation)',
      'descriptor!("tenant_simulation" => "feature-tenant-simulation", Core, ModuleActivation::Always, SQLITE, TenantSimulation)',
      'tenant simulation descriptor restores SQLite storage',
    )
  },
  'tenant_simulation',
)

expectFailure(
  'tenant simulation runtime restores direct SQLite factory construction',
  snapshot => {
    snapshot.factory += '\n// FeatureTenantSimulation::with_pool(self.pool.clone())\n'
  },
  'FeatureTenantSimulation::with_pool',
)

expectFailure(
  'warehouse descriptor restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'descriptor!(\n        "warehouse",\n        Business,\n        ModuleActivation::Always,\n        NONE,\n        Warehouse\n    )',
      'descriptor!(\n        "warehouse",\n        Business,\n        ModuleActivation::Always,\n        SQLITE,\n        Warehouse\n    )',
      'warehouse descriptor restores SQLite storage',
    )
  },
  'warehouse',
)

expectFailure(
  'Excel import dependency list restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'const DEVICE_MODULE: &[ModuleRequirement] = &[ModuleRequirement::Module("device")];',
      'const DEVICE_MODULE: &[ModuleRequirement] = &[ModuleRequirement::SqlitePool, ModuleRequirement::Module("device")];',
      'Excel import dependency list restores SQLite storage',
    )
  },
  'DEVICE_MODULE',
)

expectFailure(
  'warehouse advanced dependency list restores SQLite storage',
  snapshot => {
    snapshot.descriptors = replaceRequired(
      snapshot.descriptors,
      'const WAREHOUSE_DEVICE_MODULES: &[ModuleRequirement] = &[\n    ModuleRequirement::Module("warehouse"),',
      'const WAREHOUSE_DEVICE_MODULES: &[ModuleRequirement] = &[\n    ModuleRequirement::SqlitePool,\n    ModuleRequirement::Module("warehouse"),',
      'warehouse advanced dependency list restores SQLite storage',
    )
  },
  'WAREHOUSE_DEVICE_MODULES',
)

expectFailure(
  'audit runtime restores legacy FeatureAudit construction',
  snapshot => {
    snapshot.factory += '\n// with_pool!(FeatureAudit)\n'
  },
  'with_pool!(FeatureAudit)',
)

for (const feature of [
  'FeatureConsent',
  'FeatureDeletion',
  'FeatureReport',
  'FeatureStaff',
  'FeatureTwoFa',
]) {
  expectFailure(
    feature + ' runtime restores SQLite construction',
    snapshot => {
      snapshot.factory += '\n// with_pool!(' + feature + ')\n'
    },
    'with_pool!(' + feature + ')',
  )
}

expectFailure(
  'procurement runtime restores SQLite construction',
  snapshot => {
    snapshot.factory += '\n// with_pool!(FeatureProcurement)\n'
  },
  'with_pool!(FeatureProcurement)',
)

expectFailure(
  'ROA runtime restores SQLite construction',
  snapshot => {
    snapshot.factory += '\n// with_pool!(FeatureRoa)\n'
  },
  'with_pool!(FeatureRoa)',
)

expectFailure(
  'contract runtime restores SQLite construction',
  snapshot => {
    snapshot.factory += '\n// with_pool!(FeatureContract)\n'
  },
  'with_pool!(FeatureContract)',
)

expectFailure(
  'barcode runtime restores SQLite construction',
  snapshot => {
    snapshot.factory += '\n// with_pool!(FeatureBarcode)\n'
  },
  'with_pool!(FeatureBarcode)',
)

expectFailure(
  'reservation runtime restores SQLite construction',
  snapshot => {
    snapshot.factory += '\n// with_pool!(FeatureReservation)\n'
  },
  'with_pool!(FeatureReservation)',
)

expectFailure(
  'tenant governance runtime restores direct SQLite construction',
  snapshot => {
    snapshot.factory += '\n// FeatureTenantGovernance::with_pool(self.pool.clone())\n'
  },
  'FeatureTenantGovernance::with_pool',
)

expectFailure(
  'device runtime restores SQLite construction',
  snapshot => {
    snapshot.factory += '\n// with_pool!(FeatureDevice)\n'
  },
  'with_pool!(FeatureDevice)',
)

console.log('R4-P8 Registry storage-requirement normalization mutation tests passed.')
