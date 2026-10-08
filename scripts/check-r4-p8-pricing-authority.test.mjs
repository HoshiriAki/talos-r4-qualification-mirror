#!/usr/bin/env node
import assert from 'node:assert/strict'
import {
  collectPricingAuthoritySnapshot,
  validatePricingAuthoritySnapshot,
} from './check-r4-p8-pricing-authority.mjs'

function expectFailure(name, mutate, needle) {
  const s=structuredClone(collectPricingAuthoritySnapshot())
  mutate(s)
  const errors=validatePricingAuthoritySnapshot(s)
  assert.ok(errors.length>0, name+': mutation unexpectedly passed')
  assert.ok(errors.some(error=>error.includes(needle)), name+': '+JSON.stringify(errors))
}

assert.deepEqual(validatePricingAuthoritySnapshot(collectPricingAuthoritySnapshot()),[])

expectFailure('restore direct SQLite',s=>{
  s.factory+='\n// with_pool!(FeaturePricing)\n'
},'registry-sqlite')

expectFailure('restore descriptor SQLite',s=>{
  s.descriptors=s.descriptors.replace(
    'const PRICING: &[ModuleRequirement] = &[\n    ModuleRequirement::Module("logistics"),',
    'const PRICING: &[ModuleRequirement] = &[\n    ModuleRequirement::SqlitePool,\n    ModuleRequirement::Module("logistics"),'
  )
},'descriptor-sqlite')

expectFailure('lose serialization',s=>{
  s.pgMutation=s.pgMutation.replaceAll('pg_write_serializable_repository','pg_write')
},'pg-mutation:pg_write_serializable_repository')

expectFailure('lose tenant scope',s=>{
  s.pgRead=s.pgRead.replaceAll('WHERE tenant_id=$1','WHERE tenant_removed=$1')
},'pg-read-scope')

expectFailure('disconnect quote',s=>{
  s.factory=s.factory.replace(
    'QuoteModule::new(\n            repository_provider.clone(),\n            pricing.clone(),\n        )',
    'QuoteModule::new(repository_provider.clone(), detached_pricing.clone())'
  )
},'quote-pricing')

expectFailure('restore legacy order pricing handle',s=>{
  s.factory+='\n// legacy_order_concrete.pricing_module = Mutex::new(Some(pricing.clone()));\n'
},'order-pricing-legacy-handle')

console.log('R4-P8 Pricing authority mutation tests passed.')
