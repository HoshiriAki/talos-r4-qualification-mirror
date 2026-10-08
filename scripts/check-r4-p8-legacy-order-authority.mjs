#!/usr/bin/env node
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
const ROOT=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const P={app:'backend/src/application/order_lifecycle_compatibility.rs',factory:'backend/src/registry/factory.rs',desc:'backend/src/registry/descriptors.rs',workflow:'.github/workflows/exact-head-qualification.yml'}
const read=p=>readFileSync(path.join(ROOT,p),'utf8')
const compact=s=>s.replace(/\s+/g,'')
export function collectLegacyOrderSnapshot(){return Object.fromEntries(Object.entries(P).map(([k,p])=>[k,read(p)]))}
export function validateLegacyOrderSnapshot(s){
 const e=[], app=compact(s.app), fac=compact(s.factory), desc=compact(s.desc)
 for(const t of [
  'OrderLifecycleCompatibilityModule','RepositoryProvider','self.repositories.bind(ctx)',
  '.order_commands().update_draft(','.order_commands().create_imported_draft(',
  '.reservations().find_by_order(','.reservations().allocate_device(',
  '.reservations().allocate_devices_batch(','.reservations().release_allocation(',
  'self.lifecycle.execute(','self.reads.execute(',
  '"create_order"|"update_order"|"delete_order"=>Err('
 ]) if(!app.includes(compact(t))) e.push('Legacy Order compatibility invariant missing: '+t)
 if(!fac.includes('OrderLifecycleCompatibilityModule::new(repository_provider.clone(),)')) e.push('Legacy Order Registry provider composition missing')
 if(s.factory.includes('with_pool!(FeatureOrder)')) e.push('Legacy Order restored direct SQLite construction')
 if(!desc.includes('descriptor!("order",Business,ModuleActivation::Always,NONE,Order)')) e.push('Legacy Order descriptor must be storage-neutral')
 for(const t of [
  'node scripts/check-r4-p8-legacy-order-authority.test.mjs',
  'node scripts/check-r4-p8-legacy-order-authority.mjs',
  'live_pg18_lifecycle_write_preserves_version_history_outbox_and_scope',
  'live_pg18_reservation_write_preserves_atomic_batch_outbox_and_scope',
  'live_pg18_order_commands_preserve_draft_collision_version_scope_and_preview',
 ])
  if(!s.workflow.includes(t)) e.push('Exact-head Legacy Order qualification missing: '+t)
 return e
}
function main(){const e=validateLegacyOrderSnapshot(collectLegacyOrderSnapshot());if(e.length){console.error('R4-P8 Legacy Order authority gate failed:');for(const x of e)console.error('- '+x);process.exitCode=1}else console.log('R4-P8 Legacy Order authority gate passed.')}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))main()
