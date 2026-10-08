#!/usr/bin/env node
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const P = {
  app:'backend/src/application/pricing_compatibility.rs',
  repos:'backend/src/repositories/mod.rs',
  provider:'backend/src/repositories/contracts/provider.rs',
  sqlite:'backend/src/repositories/pricing.rs',
  dispatch:'backend/src/repositories/pricing_dispatch.rs',
  pgMutation:'backend/src/repositories/pricing_postgres_mutation.rs',
  pgRead:'backend/src/repositories/pricing_postgres_read.rs',
  factory:'backend/src/registry/factory.rs',
  descriptors:'backend/src/registry/descriptors.rs',
  pgMod:'backend/src/repositories/postgres/mod.rs',
  pgTest:'backend/src/repositories/postgres/pricing_qualification_tests.rs',
  workflow:'.github/workflows/exact-head-qualification.yml',
}
const read = p => readFileSync(path.join(ROOT,p),'utf8')
const compact = s => s.replace(/\s+/g,'')

export function collectPricingAuthoritySnapshot() {
  return Object.fromEntries(Object.entries(P).map(([k,p])=>[k,read(p)]))
}

export function validatePricingAuthoritySnapshot(s) {
  const e=[]
  const app=compact(s.app), factory=compact(s.factory), desc=compact(s.descriptors)
  for (const t of ['self.repository_provider.bind(ctx)','.pricing()','FeaturePricing::new().metadata()','resolve_warehouse','estimate_shipping','estimate_return']) {
    if (!app.includes(compact(t))) e.push('app:'+t)
  }
  for (const t of ['mod pricing;','mod pricing_dispatch;','mod pricing_postgres;','mod pricing_postgres_mutation;','mod pricing_postgres_read;','pub use pricing_dispatch::ScopedPricingRepository;']) {
    if (!s.repos.includes(t)) e.push('wiring:'+t)
  }
  if (!s.provider.includes("pub fn pricing(&self) -> ScopedPricingRepository<'_>")) e.push('provider:pricing')
  if (!s.dispatch.includes('PostgresPricingRepository') || !s.dispatch.includes('SqlitePricingRepository')) e.push('dispatch')
  for (const t of ['write_immediate','WHERE tenant_id=?1','DELETE FROM dynamic_daily_prices','INSERT INTO dynamic_daily_prices','sqlite_pricing_authority_preserves_scope_config_dynamic_and_model_prices']) {
    if (!s.sqlite.includes(t)) e.push('sqlite:'+t)
  }
  for (const t of ['pg_write_serializable_repository','WHERE tenant_id=$1','ON CONFLICT (tenant_id) DO NOTHING','DELETE FROM dynamic_daily_prices','INSERT INTO dynamic_daily_prices']) {
    if (!s.pgMutation.includes(t)) e.push('pg-mutation:'+t)
  }
  if (!s.pgRead.includes('WHERE tenant_id=$1') || !s.pgRead.includes('AND modelid=$2')) e.push('pg-read-scope')
  if (!factory.includes('PricingCompatibilityModule::new(repository_provider.clone(),Some(handles.logistics.clone()),Some(handles.warehouse_routing.clone()),)')) e.push('registry-provider')
  if (s.factory.includes('with_pool!(FeaturePricing)')) e.push('registry-sqlite')
  if (!desc.includes('constPRICING:&[ModuleRequirement]=&[ModuleRequirement::Module("logistics"),ModuleRequirement::Module("warehouse_routing"),];')) e.push('descriptor-deps')
  if (desc.includes('constPRICING:&[ModuleRequirement]=&[ModuleRequirement::SqlitePool')) e.push('descriptor-sqlite')
  if (!factory.includes('QuoteModule::new(repository_provider.clone(),pricing.clone(),)')) e.push('quote-pricing')
  if (factory.includes('legacy_order_concrete.pricing_module=Mutex::new(Some(pricing.clone()))')) e.push('order-pricing-legacy-handle')
  if (!s.pgMod.includes('mod pricing_qualification_tests;')) e.push('pg-test-module')
  for (const t of ['live_pg18_pricing_authority_preserves_scope_atomic_config_and_recomposition','list_dynamic_prices()?[0].price, 111.0','list_dynamic_prices()?[0].price, 222.0','get_model_base_price("model-b")?','persisted.base_weekday_price']) {
    if (!s.pgTest.includes(t)) e.push('pg-proof:'+t)
  }
  for (const t of ['node scripts/check-r4-p8-pricing-authority.test.mjs','node scripts/check-r4-p8-pricing-authority.mjs','live_pg18_pricing_authority_preserves_scope_atomic_config_and_recomposition']) {
    if (!s.workflow.includes(t)) e.push('workflow:'+t)
  }
  return e
}

function main() {
  const errors=validatePricingAuthoritySnapshot(collectPricingAuthoritySnapshot())
  if (errors.length) {
    console.error('R4-P8 Pricing authority gate failed:')
    for (const error of errors) console.error('- '+error)
    process.exitCode=1
  } else {
    console.log('R4-P8 Pricing authority gate passed.')
  }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
