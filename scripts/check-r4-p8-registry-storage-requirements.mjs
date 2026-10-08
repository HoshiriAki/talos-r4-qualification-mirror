#!/usr/bin/env node

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const PATHS = {
  descriptors: 'backend/src/registry/descriptors.rs',
  factory: 'backend/src/registry/factory.rs',
  main: 'backend/src/main.rs',
  workflow: '.github/workflows/exact-head-qualification.yml',
}

function read(relative) {
  return readFileSync(path.join(ROOT, relative), 'utf8')
}

function compact(source) {
  return source.replace(/\s+/g, '')
}

export function collectRegistryStorageRequirementSnapshot() {
  return Object.fromEntries(
    Object.entries(PATHS).map(([key, relative]) => [key, read(relative)]),
  )
}

export function validateRegistryStorageRequirementSnapshot(snapshot) {
  const errors = []
  const descriptors = compact(snapshot.descriptors)

  for (const token of [
    'constDEVICE_MODULE:&[ModuleRequirement]=&[ModuleRequirement::Module("device")];',
    'constWAREHOUSE_DEVICE_MODULES:&[ModuleRequirement]=&[ModuleRequirement::Module("warehouse"),ModuleRequirement::Module("device"),];',
    'descriptor!("audit"=>"feature-audit",Core,ModuleActivation::Always,NONE,Audit)',
    'descriptor!("consent",Business,ModuleActivation::Always,NONE,Consent)',
    'descriptor!("deletion",Business,ModuleActivation::Always,NONE,Deletion)',
    'descriptor!("device"=>"feature-device",Business,ModuleActivation::Always,NONE,Device)',
    'descriptor!("report",Business,ModuleActivation::Always,NONE,Report)',
    'descriptor!("staff",Business,ModuleActivation::Always,NONE,Staff)',
    'descriptor!("two_fa",Business,ModuleActivation::Always,NONE,TwoFa)',
    'descriptor!("excel_import"=>"feature-excel-import",Business,ModuleActivation::Always,DEVICE_MODULE,ExcelImport)',
    'descriptor!("model",Business,ModuleActivation::Always,NONE,Model)',
    'descriptor!("procurement",Business,ModuleActivation::Always,NONE,Procurement)',
    'descriptor!("damage",Business,ModuleActivation::Always,NONE,Damage)',
    'descriptor!("depreciation",Business,ModuleActivation::Always,NONE,Depreciation)',
    'descriptor!("deposit",Business,ModuleActivation::Always,NONE,Deposit)',
    'descriptor!("refund",Business,ModuleActivation::Always,NONE,Refund)',
    'descriptor!("repair",Business,ModuleActivation::Always,NONE,Repair)',
    'descriptor!("invoice",Business,ModuleActivation::Always,NONE,Invoice)',
    'descriptor!("settlement",Business,ModuleActivation::Always,NONE,Settlement)',
    'descriptor!("tax",Business,ModuleActivation::Always,NONE,Tax)',
    'descriptor!("credit",Business,ModuleActivation::Always,NONE,Credit)',
    'descriptor!("overdue",Business,ModuleActivation::Always,NONE,Overdue)',
    'descriptor!("order",Business,ModuleActivation::Always,NONE,Order)',
    'descriptor!("notify",Business,ModuleActivation::Always,NONE,Notify)',
    'descriptor!("barcode",Business,ModuleActivation::Always,NONE,Barcode)',
    'descriptor!("optical_sop",Business,ModuleActivation::Always,NONE,OpticalSop)',
    'descriptor!("booking",Business,ModuleActivation::Always,NONE,Booking)',
    'descriptor!("reservation",Business,ModuleActivation::Always,NONE,Reservation)',
    'descriptor!("tenant_governance"=>"feature-tenant-governance",Core,ModuleActivation::Always,NONE,TenantGovernance)',
    'descriptor!("tenant_preview"=>"feature-tenant-preview",Core,ModuleActivation::Always,NONE,TenantPreview)',
    'descriptor!("tenant_simulation"=>"feature-tenant-simulation",Core,ModuleActivation::Always,NONE,TenantSimulation)',
    'descriptor!("roa",Business,ModuleActivation::Always,NONE,Roa)',
    'descriptor!("integration",Core,ModuleActivation::Always,NONE,Integration)',
    'constPRICING:&[ModuleRequirement]=&[ModuleRequirement::Module("logistics"),ModuleRequirement::Module("warehouse_routing"),];',
    'constQUOTE:&[ModuleRequirement]=&[ModuleRequirement::Module("pricing")];',
    'descriptor!("quote",Business,ModuleActivation::Always,QUOTE,Quote)',
    'descriptor!("warehouse",Business,ModuleActivation::Always,NONE,Warehouse)',
    'descriptor!("warehouse_advanced",Business,ModuleActivation::Always,WAREHOUSE_DEVICE_MODULES,WarehouseAdvanced)',
  ]) {
    if (!descriptors.includes(token)) {
      errors.push('migrated Registry descriptor requirement missing: ' + token)
    }
  }

  for (const forbidden of [
    'constDEVICE_MODULE:&[ModuleRequirement]=&[ModuleRequirement::SqlitePool',
    'constWAREHOUSE_DEVICE_MODULES:&[ModuleRequirement]=&[ModuleRequirement::SqlitePool',
    'descriptor!("audit"=>"feature-audit",Core,ModuleActivation::Always,SQLITE,Audit)',
    'descriptor!("consent",Business,ModuleActivation::Always,SQLITE,Consent)',
    'descriptor!("deletion",Business,ModuleActivation::Always,SQLITE,Deletion)',
    'descriptor!("device"=>"feature-device",Business,ModuleActivation::Always,SQLITE,Device)',
    'descriptor!("report",Business,ModuleActivation::Always,SQLITE,Report)',
    'descriptor!("staff",Business,ModuleActivation::Always,SQLITE,Staff)',
    'descriptor!("two_fa",Business,ModuleActivation::Always,SQLITE,TwoFa)',
    'descriptor!("model",Business,ModuleActivation::Always,SQLITE,Model)',
    'descriptor!("procurement",Business,ModuleActivation::Always,SQLITE,Procurement)',
    'descriptor!("damage",Business,ModuleActivation::Always,SQLITE,Damage)',
    'descriptor!("depreciation",Business,ModuleActivation::Always,SQLITE,Depreciation)',
    'descriptor!("deposit",Business,ModuleActivation::Always,SQLITE,Deposit)',
    'descriptor!("refund",Business,ModuleActivation::Always,SQLITE,Refund)',
    'descriptor!("repair",Business,ModuleActivation::Always,SQLITE,Repair)',
    'descriptor!("invoice",Business,ModuleActivation::Always,SQLITE,Invoice)',
    'descriptor!("settlement",Business,ModuleActivation::Always,SQLITE,Settlement)',
    'descriptor!("tax",Business,ModuleActivation::Always,SQLITE,Tax)',
    'descriptor!("credit",Business,ModuleActivation::Always,SQLITE,Credit)',
    'descriptor!("overdue",Business,ModuleActivation::Always,SQLITE,Overdue)',
    'descriptor!("order",Business,ModuleActivation::Always,SQLITE,Order)',
    'descriptor!("notify",Business,ModuleActivation::Always,SQLITE,Notify)',
    'descriptor!("contract",Business,ModuleActivation::Always,SQLITE,Contract)',
    'descriptor!("barcode",Business,ModuleActivation::Always,SQLITE,Barcode)',
    'descriptor!("optical_sop",Business,ModuleActivation::Always,SQLITE,OpticalSop)',
    'descriptor!("booking",Business,ModuleActivation::Always,SQLITE,Booking)',
    'descriptor!("reservation",Business,ModuleActivation::Always,SQLITE,Reservation)',
    'descriptor!("tenant_governance"=>"feature-tenant-governance",Core,ModuleActivation::Always,SQLITE,TenantGovernance)',
    'descriptor!("tenant_preview"=>"feature-tenant-preview",Core,ModuleActivation::Always,SQLITE,TenantPreview)',
    'descriptor!("tenant_simulation"=>"feature-tenant-simulation",Core,ModuleActivation::Always,SQLITE,TenantSimulation)',
    'descriptor!("roa",Business,ModuleActivation::Always,SQLITE,Roa)',
    'descriptor!("integration",Core,ModuleActivation::Always,SQLITE,Integration)',
    'constPRICING:&[ModuleRequirement]=&[ModuleRequirement::SqlitePool',
    'constQUOTE:&[ModuleRequirement]=&[ModuleRequirement::SqlitePool',
    'descriptor!("warehouse",Business,ModuleActivation::Always,SQLITE,Warehouse)',
  ]) {
    if (descriptors.includes(forbidden)) {
      errors.push('migrated Registry descriptor retains SQLite storage requirement: ' + forbidden)
    }
  }

  for (const token of [
    'AuditCompatibilityModule::new(',
    'self.require_sqlite_pool("audit compatibility")?',
    'ConsentCompatibilityModule::new(',
    'self.require_sqlite_pool("consent compatibility")?',
    'DeletionCompatibilityModule::new(',
    'self.require_sqlite_pool("deletion compatibility")?',
    'DeviceCompatibilityModule::new(repository_provider.clone())',
    'ReportCompatibilityModule::new(',
    'self.require_sqlite_pool("report compatibility")?',
    'TenantMembershipCompatibilityModule::new(',
    'self.require_sqlite_pool("staff membership compatibility")?',
    'TwoFaCompatibilityModule::new(',
    'AuthSecurityRepository::new(',
    'ModelCompatibilityModule::new(repository_provider.clone())',
    'ProcurementCompatibilityModule::new(repository_provider.clone())',
    'DamageCompatibilityModule::new(repository_provider.clone())',
    'DepreciationCompatibilityModule::new(repository_provider.clone())',
    'DepositCompatibilityModule::new(repository_provider.clone())',
    'RefundCompatibilityModule::new(repository_provider.clone())',
    'RepairCompatibilityModule::new(repository_provider.clone())',
    'InvoiceCompatibilityModule::new(repository_provider.clone())',
    'SettlementCompatibilityModule::new(repository_provider.clone())',
    'TaxCompatibilityModule::new(repository_provider.clone())',
    'CreditCompatibilityModule::new(repository_provider.clone())',
    'OverdueCompatibilityModule::new(repository_provider.clone())',
    'PricingCompatibilityModule::new(',
    'OrderLifecycleCompatibilityModule::new(',
    'NotifyCompatibilityModule::new(repository_provider.clone())',
    'ContractCompatibilityModule::new(repository_provider.clone())',
    'BarcodeCompatibilityModule::new(repository_provider.clone())',
    'OpticalSopCompatibilityModule::new(repository_provider.clone())',
    'BookingCompatibilityModule::new(repository_provider.clone())',
    'ReservationCompatibilityModule::new(repository_provider.clone())',
    'TenantGovernanceCompatibilityModule::new(',
    'TenantPreviewCompatibilityModule::new(',
    'with_tenant_simulation_module',
    'RoaCompatibilityModule::new(repository_provider.clone())',
    'WarehouseCompatibilityModule::new(repository_provider.clone())',
    'WarehouseAdvancedCompatibilityModule::new(repository_provider.clone())',
    'ExcelImportCompatibilityModule::new(repository_provider.clone())',
  ]) {
    if (!snapshot.factory.includes(token)) {
      errors.push('normalized descriptor lacks backend-neutral runtime composition proof: ' + token)
    }
  }

  for (const forbidden of [
    'with_pool!(FeatureAudit)',
    'with_pool!(FeatureConsent)',
    'with_pool!(FeatureDeletion)',
    'with_pool!(FeatureDevice)',
    'with_pool!(FeatureReport)',
    'with_pool!(FeatureStaff)',
    'with_pool!(FeatureTwoFa)',
    'with_pool!(FeatureModel)',
    'with_pool!(FeatureProcurement)',
    'with_pool!(FeatureDamage)',
    'with_pool!(FeatureDepreciation)',
    'with_pool!(FeatureDeposit)',
    'with_pool!(FeatureRefund)',
    'with_pool!(FeatureRepair)',
    'with_pool!(FeatureInvoice)',
    'with_pool!(FeatureSettlement)',
    'with_pool!(FeatureTax)',
    'with_pool!(FeatureCredit)',
    'with_pool!(FeatureOverdue)',
    'with_pool!(FeaturePricing)',
    'with_pool!(FeatureOrder)',
    'with_pool!(FeatureNotification)',
    'with_pool!(FeatureContract)',
    'with_pool!(FeatureBarcode)',
    'with_pool!(FeatureOpticalSop)',
    'with_pool!(FeatureBooking)',
    'with_pool!(FeatureReservation)',
    'FeatureTenantGovernance::with_pool',
    'FeatureTenantPreview::with_pool',
    'FeatureTenantSimulation::with_pool',
    'with_pool!(FeatureRoa)',
    'with_pool!(FeatureWarehouse)',
    'with_pool!(FeatureWarehouseAdvanced)',
    'with_pool!(FeatureExcelImport)',
  ]) {
    if (snapshot.factory.includes(forbidden)) {
      errors.push('normalized Registry key restored SQLite runtime construction: ' + forbidden)
    }
  }

  const factory = compact(snapshot.factory)
  const main = compact(snapshot.main)

  for (const token of [
    'letintegration_concrete=matchself.integration_module.clone(){',
    'Some(module)=>module',
    'letpricing_built=constructed(PricingCompatibilityModule::new(repository_provider.clone(),Some(handles.logistics.clone()),Some(handles.warehouse_routing.clone()),));',
    'letquote_built=constructed(QuoteModule::new(repository_provider.clone(),pricing.clone(),));',
  ]) {
    if (!factory.includes(token)) {
      errors.push('descriptor truth lacks runtime composition proof: ' + token)
    }
  }

  if (!main.includes('IntegrationModule::new_with_postgres_persistence(')) {
    errors.push('integration descriptor truth lacks PostgreSQL composition proof')
  }

  if (snapshot.factory.includes('with_pool!(FeatureQuote)')) {
    errors.push('quote descriptor truth restored direct SQLite runtime construction')
  }

  for (const token of [
    'node scripts/check-r4-p8-registry-storage-requirements.test.mjs',
    'node scripts/check-r4-p8-registry-storage-requirements.mjs',
  ]) {
    if (!snapshot.workflow.includes(token)) {
      errors.push('Exact-head Registry storage-requirement qualification missing: ' + token)
    }
  }

  return errors
}

function main() {
  const errors = validateRegistryStorageRequirementSnapshot(
    collectRegistryStorageRequirementSnapshot(),
  )
  if (errors.length) {
    console.error('R4-P8 Registry storage-requirement normalization gate failed:')
    for (const error of errors) console.error('- ' + error)
    process.exitCode = 1
    return
  }
  console.log('R4-P8 Registry storage-requirement normalization gate passed.')
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
