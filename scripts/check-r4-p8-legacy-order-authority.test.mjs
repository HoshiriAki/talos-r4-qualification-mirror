#!/usr/bin/env node
import assert from 'node:assert/strict'
import {collectLegacyOrderSnapshot,validateLegacyOrderSnapshot} from './check-r4-p8-legacy-order-authority.mjs'
function expect(name,mutate,needle){const s=structuredClone(collectLegacyOrderSnapshot());mutate(s);const e=validateLegacyOrderSnapshot(s);assert.ok(e.some(x=>x.includes(needle)),name+': '+JSON.stringify(e))}
assert.deepEqual(validateLegacyOrderSnapshot(collectLegacyOrderSnapshot()),[])
expect('factory sqlite rollback',s=>s.factory=s.factory.replace('OrderLifecycleCompatibilityModule::new(\n            repository_provider.clone(),\n        )','with_pool!(FeatureOrder)'),'direct SQLite')
expect('descriptor sqlite rollback',s=>s.desc=s.desc.replace('descriptor!("order", Business, ModuleActivation::Always, NONE, Order)','descriptor!("order", Business, ModuleActivation::Always, SQLITE, Order)'),'descriptor')
expect('generic create resurrected',s=>s.app=s.app.replace('"create_order" | "update_order" | "delete_order" => Err(','"create_order" => self.old_create(payload, ctx),\n            "update_order" | "delete_order" => Err('),'create_order')
expect('draft update bypasses repository',s=>s.app=s.app.replace('.order_commands()\n            .update_draft(','.legacy_sqlite()\n            .update_draft('),'order_commands')
console.log('R4-P8 Legacy Order authority mutation tests passed.')
